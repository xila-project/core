use std::{
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use debug_protocol::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};

const MAX_TRANSPORT_RESPONSE_BYTES: usize =
    MAX_RESPONSE_BYTES + debug_protocol::MAX_SCREENSHOT_BYTES;

pub(crate) struct PendingRequest {
    pub bytes: Vec<u8>,
    pub response: Sender<Vec<u8>>,
}

pub struct ControlServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    endpoint: PathBuf,
}

impl ControlServer {
    pub(crate) fn start(sender: SyncSender<PendingRequest>) -> Result<Self, String> {
        let endpoint = endpoint_path()?;
        let listener = bind_private_endpoint(&endpoint)?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Failed to configure local endpoint: {error}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("xila-debug-control".into())
            .spawn(move || serve(listener, sender, thread_stop))
            .map_err(|error| format!("Failed to start debug transport: {error}"))?;

        Ok(Self {
            stop,
            thread: Some(thread),
            endpoint,
        })
    }

    pub fn endpoint(&self) -> &std::path::Path {
        &self.endpoint
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.endpoint);
    }
}

pub fn endpoint_path() -> Result<PathBuf, String> {
    let uid = unsafe { libc::geteuid() };
    let directory = std::env::temp_dir().join(format!("xila-debug-{uid}"));
    match fs::create_dir(&directory) {
        Ok(()) => fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("Failed to secure runtime directory: {error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(&directory)
                .map_err(|error| format!("Failed to inspect runtime directory: {error}"))?;
            if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
                return Err("Debug runtime directory is not private to the current user.".into());
            }
        }
        Err(error) => return Err(format!("Failed to create debug runtime directory: {error}")),
    }

    let endpoint = std::env::var_os("XILA_UI_CONTROL_ENDPOINT")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("control.sock"));
    if endpoint.parent() != Some(directory.as_path()) {
        return Err(
            "XILA_UI_CONTROL_ENDPOINT must be inside the private runtime directory.".into(),
        );
    }
    Ok(endpoint)
}

fn bind_private_endpoint(endpoint: &std::path::Path) -> Result<UnixListener, String> {
    if let Ok(metadata) = fs::symlink_metadata(endpoint) {
        let owner = metadata.uid();
        if !metadata.file_type().is_socket() || owner != unsafe { libc::geteuid() } {
            return Err("Refusing to replace an unowned or non-socket endpoint.".into());
        }
        match UnixStream::connect(endpoint) {
            Ok(_) => return Err("A debug control endpoint is already active.".into()),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                fs::remove_file(endpoint)
                    .map_err(|error| format!("Failed to remove stale owned endpoint: {error}"))?;
            }
            Err(error) => return Err(format!("Cannot verify existing endpoint: {error}")),
        }
    }

    let listener = UnixListener::bind(endpoint)
        .map_err(|error| format!("Failed to bind debug endpoint: {error}"))?;
    fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Failed to secure debug endpoint: {error}"))?;
    Ok(listener)
}

fn serve(listener: UnixListener, sender: SyncSender<PendingRequest>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => serve_request(stream, &sender, &stop),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                log::error!("Native debug endpoint failed: {error}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn serve_request(mut stream: UnixStream, sender: &SyncSender<PendingRequest>, stop: &AtomicBool) {
    if stream.set_nonblocking(true).is_err() {
        return;
    }
    let mut bytes = Vec::with_capacity(1024);
    let deadline = Instant::now() + Duration::from_secs(5);
    let read = loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        if Instant::now() >= deadline {
            break Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Timed out waiting for a request.",
            ));
        }
        let mut chunk = [0; 1024];
        match stream.read(&mut chunk) {
            Ok(0) => break Ok(bytes.len()),
            Ok(length) => {
                bytes.extend_from_slice(&chunk[..length]);
                if bytes.len() > MAX_REQUEST_BYTES {
                    break Ok(bytes.len());
                }
                if bytes.contains(&b'\n') {
                    break Ok(bytes.len());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => break Err(error),
        }
    };

    match read {
        Ok(length) if length <= MAX_REQUEST_BYTES => {
            let (response_sender, response_receiver): (Sender<Vec<u8>>, Receiver<Vec<u8>>) =
                std::sync::mpsc::channel();
            if sender
                .send(PendingRequest {
                    bytes,
                    response: response_sender,
                })
                .is_err()
            {
                write_response(
                    &mut stream,
                    b"{\"ok\":false,\"error\":{\"code\":\"debug_app_unavailable\",\"message\":\"Debug app is not running.\"}}\n",
                    stop,
                );
                return;
            }
            let response_deadline = std::time::Instant::now() + Duration::from_secs(30);
            let response = loop {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let remaining =
                    response_deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break Err(std::sync::mpsc::RecvTimeoutError::Timeout);
                }
                match response_receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    result => break result,
                }
            };
            match response {
                Ok(response) if response.len() <= MAX_TRANSPORT_RESPONSE_BYTES => {
                    write_response(&mut stream, &response, stop);
                }
                Ok(_) => {
                    write_response(
                        &mut stream,
                        b"{\"ok\":false,\"error\":{\"code\":\"response_too_large\",\"message\":\"Response exceeds the 64 KiB limit.\"}}\n",
                        stop,
                    );
                }
                Err(_) => {
                    write_response(
                        &mut stream,
                        b"{\"ok\":false,\"error\":{\"code\":\"debug_app_timeout\",\"message\":\"Debug app did not respond within 30 seconds.\"}}\n",
                        stop,
                    );
                }
            }
        }
        Ok(_) => {
            write_response(
                &mut stream,
                b"{\"ok\":false,\"error\":{\"code\":\"request_too_large\",\"message\":\"Request exceeds the 16 KiB limit.\"}}\n",
                stop,
            );
        }
        Err(error) => {
            let message = format!(
                "{{\"ok\":false,\"error\":{{\"code\":\"ipc_read_failed\",\"message\":{:?}}}}}\n",
                error.to_string()
            );
            write_response(&mut stream, message.as_bytes(), stop);
        }
    }
}

fn write_response(stream: &mut UnixStream, response: &[u8], stop: &AtomicBool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut offset = 0;
    while offset < response.len() {
        if stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return;
        }
        match stream.write(&response[offset..]) {
            Ok(0) => return,
            Ok(length) => offset += length,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_directory_is_private_and_stable() {
        let endpoint = endpoint_path().unwrap();
        let directory = endpoint.parent().unwrap();
        let metadata = fs::metadata(directory).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o700);
        assert!(
            directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("xila-debug-")
        );
    }
}
