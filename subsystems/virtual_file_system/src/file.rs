use super::VirtualFileSystem;
use crate::{Error, ItemStatic, Result, SynchronousFile, poll};
use alloc::{vec, vec::Vec};
use embedded_io_async::ErrorType;
use exported_file_system::{AccessFlags, ControlCommand, CreateFlags, Permissions};
use file_system::{Context, Flags, Path, Position, Size, StateFlags, Statistics};
use task::TaskIdentifier;
use task::block_on;
use users::{GroupIdentifier, UserIdentifier};

/// File structure.
///
/// This structure is used to represent a file in the virtual file system.
/// This is a wrapper around the virtual file system file identifier.
#[derive(Debug)]
pub struct File<'a>(SynchronousFile<'a>);

impl<'a> File<'a> {
    pub fn as_synchronous_file_mut(&mut self) -> &mut SynchronousFile<'a> {
        &mut self.0
    }
}

impl ErrorType for File<'_> {
    type Error = Error;
}

impl embedded_io_async::Write for File<'_> {
    async fn write(&mut self, buf: &[u8]) -> core::result::Result<usize, Self::Error> {
        File::write(self, buf).await
    }

    async fn flush(&mut self) -> core::result::Result<(), Self::Error> {
        File::flush(self).await
    }
}

impl core::fmt::Write for File<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if let Err(e) = block_on(self.write(s.as_bytes())) {
            log::error!("Error writing string to file: {}", e);
            return Err(core::fmt::Error);
        }
        Ok(())
    }
}

impl<'a> File<'a> {
    fn nonblocking(&self) -> bool {
        self.0.flags.get_state().contains(StateFlags::NonBlocking)
    }

    pub(crate) fn new(
        virtual_file_system: &'a VirtualFileSystem,
        item: ItemStatic,
        flags: Flags,
        context: Context,
    ) -> Self {
        Self(SynchronousFile::new(
            virtual_file_system,
            item,
            0,
            flags,
            context,
        ))
    }

    pub(crate) fn with_authority(mut self, authority: task::TaskAuthority<'static>) -> Self {
        self.0.authority = Some(authority);
        self
    }

    pub async fn open(
        virtual_file_system: &'a VirtualFileSystem,
        task: task::TaskIdentifier,
        path: impl AsRef<Path>,
        flags: Flags,
    ) -> Result<Self> {
        let file_identifier = virtual_file_system.open(&path, flags, task).await?;

        Ok(file_identifier)
    }

    pub async fn create_unnamed_pipe(
        file_system: &'a VirtualFileSystem,
        size: usize,
        status: StateFlags,
    ) -> Result<(Self, Self)> {
        file_system.create_unnamed_pipe(size, status).await
    }

    pub async fn set_position(&mut self, position: &Position) -> Result<Size> {
        poll(|| self.0.set_position(position)).await
    }

    // - Operations

    pub async fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        if self.nonblocking() {
            return self.0.write(buffer);
        }
        crate::operation::write(&mut self.0, buffer).await
    }

    /// Fill the entire slice, returning an I/O error on premature EOF.
    /// Close explicitly on success or failure; preserve the I/O error if close
    /// also fails.
    pub async fn read_slice_from_path(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        buffer: &mut [u8],
    ) -> Result<()> {
        let mut file = File::open(
            virtual_file_system,
            task,
            path,
            Flags::new(AccessFlags::Read, None, None),
        )
        .await?;

        let result = async {
            let mut offset = 0;
            while offset < buffer.len() {
                let count = file.read(&mut buffer[offset..]).await?;
                if count == 0 {
                    return Err(Error::FileSystem(file_system::Error::InputOutput));
                }
                offset += count;
            }
            Ok(())
        }
        .await;
        let close = file.close(virtual_file_system).await;
        result.and(close)
    }

    pub async fn read_from_path(
        virtual_file_system: &VirtualFileSystem,
        task: TaskIdentifier,
        path: impl AsRef<Path>,
        buffer: &mut Vec<u8>,
    ) -> Result<()> {
        buffer.clear();

        let mut file = File::open(
            virtual_file_system,
            task,
            path,
            Flags::new(AccessFlags::Read, None, None),
        )
        .await?;

        let result = file.read_to_end(buffer, 256).await.map(|_| ());
        let close = file.close(virtual_file_system).await;
        result.and(close)
    }

    /// Write the entire buffer, retrying short writes and rejecting zero progress.
    /// Close explicitly on success or failure.
    pub async fn write_to_path(
        virtual_file_system: &VirtualFileSystem,
        task: task::TaskIdentifier,
        path: impl AsRef<Path>,
        buffer: &[u8],
    ) -> Result<()> {
        let mut file = File::open(
            virtual_file_system,
            task,
            path,
            Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
        )
        .await?;

        let result = async {
            let mut offset = 0;
            while offset < buffer.len() {
                let count = file.write(&buffer[offset..]).await?;
                if count == 0 {
                    return Err(Error::FailedToWrite);
                }
                offset += count;
            }
            Ok(())
        }
        .await;
        let close = file.close(virtual_file_system).await;
        result.and(close)
    }

    pub async fn write_line(&mut self, buffer: &[u8]) -> Result<usize> {
        let mut written = 0;
        for part in [buffer, b"\n".as_slice()] {
            let mut offset = 0;
            while offset < part.len() {
                match self.write(&part[offset..]).await {
                    Ok(0) => return Err(Error::FailedToWrite),
                    Ok(count) => {
                        offset += count;
                        written += count;
                    }
                    Err(
                        Error::FileSystem(file_system::Error::RessourceBusy) | Error::RessourceBusy,
                    ) if self.nonblocking() && written > 0 => return Ok(written),
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(written)
    }

    pub async fn display_content<W: embedded_io_async::Write>(
        &mut self,
        w: &mut W,
        buffer_size: usize,
    ) -> Result<()> {
        let mut buffer = vec![0u8; buffer_size];
        loop {
            let bytes_read = self.read(&mut buffer).await?;

            if bytes_read == 0 {
                break;
            }

            w.write(&buffer[..bytes_read])
                .await
                .map_err(|_| Error::FailedToWrite)?;
        }

        Ok(())
    }

    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        if self.nonblocking() {
            return self.0.read(buffer);
        }
        crate::operation::read(&mut self.0, buffer).await
    }

    pub async fn read_until(&mut self, buffer: &mut [u8], delimiter: &[u8]) -> Result<usize> {
        if self.nonblocking() {
            return self.0.read_until(buffer, delimiter);
        }
        crate::operation::read_until(&mut self.0, buffer, delimiter).await
    }

    pub async fn read_to_end(&mut self, buffer: &mut Vec<u8>, chunk_size: usize) -> Result<usize> {
        if chunk_size == 0 {
            return Err(Error::FileSystem(file_system::Error::InvalidParameter));
        }
        let mut chunk = vec![0; chunk_size];
        let mut total = 0;
        loop {
            match self.read(&mut chunk).await {
                Ok(0) => return Ok(total),
                Ok(count) => {
                    buffer.extend_from_slice(&chunk[..count]);
                    total += count;
                }
                Err(
                    Error::FileSystem(file_system::Error::RessourceBusy) | Error::RessourceBusy,
                ) if self.nonblocking() && total > 0 => return Ok(total),
                Err(error) => return Err(error),
            }
        }
    }

    pub async fn flush(&mut self) -> Result<()> {
        if self.nonblocking() {
            return self.0.flush();
        }
        crate::operation::flush(&mut self.0).await
    }

    pub async fn duplicate(&self) -> Result<Self> {
        Ok(Self(poll(|| self.0.duplicate()).await?))
    }

    pub async fn get_statistics(&mut self) -> Result<Statistics> {
        poll(|| self.0.get_statistics()).await
    }

    pub async fn set_owner(
        &mut self,
        user: Option<UserIdentifier>,
        group: Option<GroupIdentifier>,
    ) -> Result<()> {
        poll(|| self.0.set_owner(user, group)).await
    }

    pub async fn set_permissions(&mut self, permissions: Permissions) -> Result<()> {
        poll(|| self.0.set_permissions(permissions)).await
    }

    pub async fn control<C>(&mut self, command: C, argument: &C::Input) -> Result<C::Output>
    where
        C: ControlCommand,
        C::Output: Default,
    {
        if self.nonblocking() {
            return self.0.control(command, argument);
        }
        crate::operation::control(&mut self.0, command, argument).await
    }

    pub fn get_access(&self) -> Result<AccessFlags> {
        Ok(self.0.flags.get_access())
    }

    pub async fn close(self, virtual_file_system: &VirtualFileSystem) -> crate::Result<()> {
        self.0.close(virtual_file_system)
    }

    pub fn into_synchronous_file(self) -> SynchronousFile<'a> {
        self.0
    }
}

pub struct FileControlIterator<'a, C> {
    file: &'a mut File<'a>,
    get_command: C,
    index: usize,
    count: usize,
}

impl<'a, C> FileControlIterator<'a, C>
where
    C: ControlCommand<Input = usize>,
    C::Output: Default,
{
    pub async fn new<Cc>(file: &'a mut File<'a>, count_command: Cc, get_command: C) -> Result<Self>
    where
        Cc: ControlCommand<Input = (), Output = usize>,
    {
        let count: usize = file.control(count_command, &()).await?;

        Ok(Self {
            file,
            get_command,
            index: 0,
            count,
        })
    }

    pub async fn next(&mut self) -> Result<Option<C::Output>> {
        if self.index >= self.count {
            return Ok(None);
        }

        let result = self.file.control(self.get_command, &self.index).await?;

        self.index += 1;

        Ok(Some(result))
    }
}
