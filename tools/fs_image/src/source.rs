//! Descriptor-relative source traversal: pathname changes cannot redirect an
//! import outside its opened root, or turn an opened entry into a FIFO.
use std::{fs::File, io, path::Path};

pub enum Entry {
    Directory(Directory),
    File(File),
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod native {
    use super::*;
    use std::{
        ffi::{CStr, CString},
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::OpenOptionsExt,
        },
    };

    pub struct Directory(File);

    impl Directory {
        pub fn open(path: &Path) -> io::Result<Self> {
            let file = File::options()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)?;
            Ok(Self(file))
        }

        pub fn open_child(&self, name: &str) -> io::Result<Entry> {
            let name = CString::new(name)?;
            // SAFETY: the descriptor and NUL-terminated component are valid
            // for this call. O_NOFOLLOW rejects substituted symlinks and
            // O_NONBLOCK prevents substituted FIFOs from blocking the build.
            let fd = unsafe {
                libc::openat(
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: openat returned a new owned descriptor exactly once.
            let file = unsafe { File::from_raw_fd(fd) };
            let kind = file.metadata()?.file_type();
            if kind.is_dir() {
                Ok(Entry::Directory(Self(file)))
            } else if kind.is_file() {
                Ok(Entry::File(file))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only regular files and directories can be imported",
                ))
            }
        }

        pub fn names(&self) -> io::Result<Vec<String>> {
            // SAFETY: duplicate a live descriptor; fdopendir owns only the
            // duplicate, leaving this Directory valid throughout traversal.
            let fd = unsafe { libc::fcntl(self.0.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let stream = unsafe { libc::fdopendir(fd) };
            if stream.is_null() {
                let error = io::Error::last_os_error();
                unsafe {
                    libc::close(fd);
                }
                return Err(error);
            }
            struct Stream(*mut libc::DIR);
            impl Drop for Stream {
                fn drop(&mut self) {
                    unsafe {
                        libc::closedir(self.0);
                    }
                }
            }
            let stream = Stream(stream);
            // A duplicated descriptor shares its directory offset. Rewind on
            // every scan so validation, population and verification see it.
            unsafe {
                libc::rewinddir(stream.0);
            }
            let mut names = Vec::new();
            loop {
                // SAFETY: errno is thread-local; readdir's pointer remains
                // valid until the next call on this exclusively owned stream.
                unsafe {
                    #[cfg(target_os = "linux")]
                    {
                        *libc::__errno_location() = 0;
                    }
                    #[cfg(target_os = "macos")]
                    {
                        *libc::__error() = 0;
                    }
                    let entry = libc::readdir(stream.0);
                    if entry.is_null() {
                        let error = io::Error::last_os_error();
                        if error.raw_os_error() != Some(0) {
                            return Err(error);
                        }
                        break;
                    }
                    let name = CStr::from_ptr((*entry).d_name.as_ptr())
                        .to_str()
                        .map_err(|_| {
                            io::Error::new(io::ErrorKind::InvalidInput, "file names must be UTF-8")
                        })?;
                    if !matches!(name, "." | "..") {
                        names.push(name.to_owned());
                    }
                }
            }
            names.sort();
            Ok(names)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use native::Directory;

// Avoid silently falling back to pathname-following traversal on platforms
// without the descriptor-relative implementation. Prebuilt images still work
// with the native runtime there; only host-side population is restricted.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub struct Directory;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Directory {
    pub fn open(_: &Path) -> io::Result<Self> {
        Err(unsupported())
    }
    pub fn open_child(&self, _: &str) -> io::Result<Entry> {
        Err(unsupported())
    }
    pub fn names(&self) -> io::Result<Vec<String>> {
        Err(unsupported())
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "image population requires Linux or macOS descriptor-relative filesystem APIs",
    )
}
