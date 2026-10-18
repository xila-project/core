//! Main-thread OPFS disk with a synchronous memory cache.
//!
//! OPFS synchronous access handles are worker-only. Reads and writes therefore
//! use one disk-sized cache; callers must await [`DriveDevice::sync`] to commit
//! it to OPFS. The synchronous `flush` operation does not provide durability.
//! Saves are atomic snapshots, serialized with at most one extra disk-sized
//! buffer in flight. A lifetime Web Lock prevents conflicting browser tabs.

#[cfg(target_feature = "atomics")]
compile_error!("The OPFS cache driver requires single-threaded browser WASM");

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use file_system::{
    ControlCommand, ControlCommandIdentifier, DirectBaseOperations, DirectBlockDevice, Error,
    MountOperations, Path, Position, Size,
    block_device::{GET_BLOCK_COUNT, GET_BLOCK_SIZE},
    mount::MutexMountWrapper,
};
use js_sys::Uint8Array;
use shared::AnyByLayout;
use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    DomException, File, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions,
    FileSystemWritableFileStream,
};

const BLOCK_SIZE: u32 = 512;

// A reservation can be released even if its Rust opening future is cancelled
// before the Web Locks callback runs. No JS promise is synchronously blocked.
#[wasm_bindgen(inline_js = "
const releasingDrives = new Map();
export function reserveDrive(name) {
    if (!globalThis.navigator?.storage?.getDirectory || !navigator.locks) {
        throw new DOMException('OPFS and Web Locks require a secure browser context', 'NotSupportedError');
    }
    let released = false;
    let unlock;
    let request;
    const ready = new Promise((resolve, reject) => {
        const acquire = () => {
            request = navigator.locks.request('xila-opfs:' + name, {ifAvailable: true}, lock => {
                if (released) {
                    reject(new DOMException('Opening was cancelled', 'AbortError'));
                    return;
                }
                if (!lock) {
                    reject(new DOMException('Disk is already open', 'NoModificationAllowedError'));
                    return;
                }
                return new Promise(release => {
                    unlock = release;
                    resolve();
                });
            }).catch(reject);
        };
        const previous = releasingDrives.get(name);
        if (previous) previous.then(acquire, acquire);
        else acquire();
    });
    // A cancelled Rust future may no longer observe rejection.
    ready.catch(() => {});
    return {ready, release() {
        released = true;
        if (request) {
            releasingDrives.set(name, request);
            request.then(() => {
                if (releasingDrives.get(name) === request) releasingDrives.delete(name);
            });
        }
        if (unlock) unlock();
    }};
}
")]
extern "C" {
    type Reservation;

    #[wasm_bindgen(catch, js_name = reserveDrive)]
    fn reserve_drive(name: &str) -> Result<Reservation, JsValue>;

    #[wasm_bindgen(method, getter)]
    fn ready(this: &Reservation) -> js_sys::Promise;

    #[wasm_bindgen(method)]
    fn release(this: &Reservation);
}

#[wasm_bindgen(module = "/src/devices/seed.mjs")]
extern "C" {
    #[wasm_bindgen(catch, js_name = importSeed)]
    async fn import_seed(url: &str, data: &Uint8Array) -> Result<JsValue, JsValue>;
}

struct Lease(Reservation);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn map_error(value: JsValue) -> Error {
    match value
        .dyn_ref::<DomException>()
        .map(DomException::name)
        .as_deref()
    {
        Some("QuotaExceededError") => Error::NoSpaceLeft,
        Some("NotAllowedError" | "SecurityError") => Error::PermissionDenied,
        Some("NotFoundError") => Error::NotFound,
        Some("NoModificationAllowedError" | "InvalidStateError") => Error::RessourceBusy,
        Some("NotSupportedError") => Error::UnsupportedOperation,
        Some("TypeMismatchError") => Error::InvalidFile,
        _ => Error::InputOutput,
    }
}

struct Inner {
    handle: FileSystemFileHandle,
    data: Uint8Array,
    generation: u64,
    saved_generation: u64,
    _lease: Lease,
}

// SAFETY: this module rejects atomics-enabled WASM; all JS objects and device
// operations stay on the single browser thread. Cache access is also guarded.
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

struct State {
    inner: MutexMountWrapper<CriticalSectionRawMutex, Inner>,
    saving: Mutex<CriticalSectionRawMutex, ()>,
    accepting_writes: AtomicBool,
}

impl State {
    async fn sync(&self) -> file_system::Result<()> {
        let _save = self.saving.lock().await;
        let (handle, snapshot, generation) = {
            let inner = self.inner.try_get()?;
            if inner.generation == inner.saved_generation {
                return Ok(());
            }
            (
                inner.handle.clone(),
                inner.data.slice(0, inner.data.length()),
                inner.generation,
            )
        };

        let stream = JsFuture::from(handle.create_writable())
            .await
            .map_err(map_error)?
            .dyn_into::<FileSystemWritableFileStream>()
            .map_err(map_error)?;

        let result = async {
            JsFuture::from(
                stream
                    .write_with_js_u8_array(&snapshot)
                    .map_err(map_error)?,
            )
            .await
            .map_err(map_error)?;
            // Closing, not writing, commits the replacement file atomically.
            JsFuture::from(stream.close()).await.map_err(map_error)?;
            Ok(())
        }
        .await;

        if result.is_err() {
            let _ = JsFuture::from(stream.abort()).await;
            return result;
        }

        // Later writes have a different generation and still need a save.
        self.inner.try_get()?.saved_generation = generation;
        Ok(())
    }
}

pub struct DriveDevice(Arc<State>, bool);

impl DriveDevice {
    /// Open a fixed-size OPFS disk. `name` must be a single filename and `size`
    /// a nonzero multiple of 512. A nonempty image must already have this size.
    /// Opening does not erase or resize an existing disk.
    pub async fn new(name: &Path, size: usize) -> file_system::Result<Self> {
        if size == 0 || !size.is_multiple_of(BLOCK_SIZE as usize) || size > isize::MAX as usize {
            return Err(Error::InvalidParameter);
        }
        let name = name.as_str();
        if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', '\0']) {
            return Err(Error::InvalidPath);
        }

        let lease = Lease(reserve_drive(name).map_err(map_error)?);
        JsFuture::from(lease.0.ready()).await.map_err(map_error)?;

        let storage = web_sys::window()
            .ok_or(Error::UnsupportedOperation)?
            .navigator()
            .storage();
        let directory = JsFuture::from(storage.get_directory())
            .await
            .map_err(map_error)?
            .dyn_into::<FileSystemDirectoryHandle>()
            .map_err(map_error)?;
        let options = FileSystemGetFileOptions::new();
        options.set_create(true);
        let handle = JsFuture::from(directory.get_file_handle_with_options(name, &options))
            .await
            .map_err(map_error)?
            .dyn_into::<FileSystemFileHandle>()
            .map_err(map_error)?;
        let file = JsFuture::from(handle.get_file())
            .await
            .map_err(map_error)?
            .dyn_into::<File>()
            .map_err(map_error)?;
        let is_new = file.size() == 0.0;
        let data = if is_new {
            Uint8Array::new_with_length(size as u32)
        } else {
            if file.size() != size as f64 {
                return Err(Error::InvalidParameter);
            }
            Uint8Array::new(
                &JsFuture::from(file.array_buffer())
                    .await
                    .map_err(map_error)?,
            )
        };

        Ok(Self(
            Arc::new(State {
                inner: MutexMountWrapper::new_mounted(Inner {
                    handle,
                    data,
                    generation: u64::from(is_new),
                    saved_generation: 0,
                    _lease: lease,
                }),
                saving: Mutex::new(()),
                accepting_writes: AtomicBool::new(true),
            }),
            is_new,
        ))
    }

    /// Import a reference image into an uncommitted cache only for an absent
    /// or zero-byte disk. The caller must validate/mount it before `sync`.
    /// Existing disks never fetch a reference image.
    pub async fn new_seeded(name: &Path, size: usize, seed_url: &str) -> file_system::Result<Self> {
        let drive = Self::new(name, size).await?;
        if drive.is_new() {
            let data = drive.0.inner.try_get()?.data.clone();
            import_seed(seed_url, &data).await.map_err(map_error)?;
        }
        Ok(drive)
    }

    /// Abandon the cache without committing it and release the disk lease.
    /// Intended for startup failures before the filesystem becomes usable.
    /// Unlike `unmount`, this explicitly discards unsaved changes. A running
    /// save must finish first; this never races an OPFS commit.
    pub fn discard_unsaved(&self) -> file_system::Result<()> {
        let _save = self.0.saving.try_lock().map_err(|_| Error::RessourceBusy)?;
        self.0.inner.unmount()
    }

    /// Whether the disk had no committed image when it was opened. This stays
    /// unchanged after saves so boot code can distinguish initial formatting
    /// from mounting an existing installation, even if its metadata is invalid.
    pub const fn is_new(&self) -> bool {
        self.1
    }

    /// Commit a consistent cache snapshot to OPFS, or return a storage error.
    /// Writes made during this save remain dirty for the next call. Saves keep
    /// running if the waiting future is cancelled, retaining the lease and save
    /// mutex until completion. Dropping the device does not save dirty data.
    pub async fn sync(&self) -> file_system::Result<()> {
        let completion =
            Arc::new(Signal::<CriticalSectionRawMutex, file_system::Result<()>>::new());
        let state = self.0.clone();
        let result = completion.clone();
        spawn_local(async move {
            result.signal(state.sync().await);
        });
        completion.wait().await
    }

    /// Temporarily reject writes so the browser host can safely save and
    /// release this disk before switching to another image.
    pub fn stop_writes(&self) {
        self.0.accepting_writes.store(false, Ordering::Release);
    }

    /// Resume writes when a requested disk switch could not be saved.
    pub fn resume_writes(&self) {
        self.0.accepting_writes.store(true, Ordering::Release);
    }
}

impl DirectBaseOperations for DriveDevice {
    fn read(&self, buffer: &mut [u8], position: Size) -> file_system::Result<usize> {
        let inner = self.0.inner.try_get()?;
        let position = u32::try_from(position).map_err(|_| Error::InvalidParameter)?;
        let size = inner.data.length();
        if position > size {
            return Err(Error::InvalidParameter);
        }
        let count = buffer.len().min((size - position) as usize);
        inner
            .data
            .subarray(position, position + count as u32)
            .copy_to(&mut buffer[..count]);
        Ok(count)
    }

    fn write(&self, buffer: &[u8], position: Size) -> file_system::Result<usize> {
        if !self.0.accepting_writes.load(Ordering::Acquire) {
            return Err(Error::RessourceBusy);
        }
        let mut inner = self.0.inner.try_get()?;
        let position = u32::try_from(position).map_err(|_| Error::InvalidParameter)?;
        let size = inner.data.length();
        if position > size {
            return Err(Error::InvalidParameter);
        }
        let count = buffer.len().min((size - position) as usize);
        if count != 0 {
            inner
                .data
                .subarray(position, position + count as u32)
                .copy_from(&buffer[..count]);
            inner.generation = inner.generation.wrapping_add(1);
        }
        Ok(count)
    }

    fn set_position(&self, current: Size, position: &Position) -> file_system::Result<Size> {
        let inner = self.0.inner.try_get()?;
        file_system::block_device::set_position(current, position, inner.data.length() as Size)
    }

    /// Cache-only flush. OPFS durability requires awaiting `sync()`.
    fn flush(&self) -> file_system::Result<()> {
        let _inner = self.0.inner.try_get()?;
        Ok(())
    }

    fn control(
        &self,
        command: ControlCommandIdentifier,
        _: &AnyByLayout,
        output: &mut AnyByLayout,
    ) -> file_system::Result<()> {
        let inner = self.0.inner.try_get()?;
        match command {
            GET_BLOCK_SIZE::IDENTIFIER => *GET_BLOCK_SIZE::cast_output(output)? = BLOCK_SIZE,
            GET_BLOCK_COUNT::IDENTIFIER => {
                *GET_BLOCK_COUNT::cast_output(output)? = inner.data.length() / BLOCK_SIZE;
            }
            _ => return Err(Error::UnsupportedOperation),
        }
        Ok(())
    }
}

impl MountOperations for DriveDevice {
    /// Unmount only a clean cache when no save is running. Await `sync()` first.
    fn unmount(&self) -> file_system::Result<()> {
        let _save = self.0.saving.try_lock().map_err(|_| Error::RessourceBusy)?;
        {
            let inner = self.0.inner.try_get()?;
            if inner.generation != inner.saved_generation {
                return Err(Error::RessourceBusy);
            }
        }
        self.0.inner.unmount()
    }
}

impl DirectBlockDevice for DriveDevice {}
