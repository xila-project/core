use core::{
    future::poll_fn,
    task::{Context, Poll},
};

use crate::{
    Duration, Error, IpAddress, IpEndpoint, IpListenEndpoint, Port, Result, SocketContext,
};
use embassy_futures::block_on;
use smoltcp::socket::tcp;

#[repr(transparent)]
pub struct TcpSocket {
    context: SocketContext,
}

impl TcpSocket {
    pub(crate) fn new(context: SocketContext) -> Self {
        Self { context }
    }
    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&tcp::Socket<'static>) -> R,
    {
        self.context.with(f).await
    }

    pub async fn with_mutable<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut tcp::Socket<'static>) -> R,
    {
        self.context.with_mutable(f).await
    }

    pub fn poll_with<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&tcp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with(context, f)
    }

    pub fn poll_with_mutable<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&mut tcp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with_mutable(context, f)
    }

    pub async fn set_timeout(&mut self, timeout: Option<Duration>) {
        let timeout = timeout.map(Duration::into_smoltcp);

        self.with_mutable(|socket| socket.set_timeout(timeout))
            .await
    }

    pub async fn accept(
        &mut self,
        address: Option<impl Into<IpAddress>>,
        port: impl Into<Port>,
    ) -> Result<()> {
        let endpoint = IpListenEndpoint::new(address.map(Into::into), port.into()).into_smoltcp();

        self.with_mutable(|s| {
            if s.state() == tcp::State::Closed {
                s.listen(endpoint).map_err(|e| match e {
                    tcp::ListenError::InvalidState => Error::InvalidState,
                    tcp::ListenError::Unaddressable => Error::InvalidPort,
                })
            } else {
                Ok(())
            }
        })
        .await?;

        poll_fn(|cx| {
            self.poll_with_mutable(cx, |s, cx| match s.state() {
                tcp::State::Listen | tcp::State::SynSent | tcp::State::SynReceived => {
                    s.register_send_waker(cx.waker());
                    Poll::Pending
                }
                _ => Poll::Ready(Ok(())),
            })
        })
        .await
    }

    pub async fn connect(
        &mut self,
        address: impl Into<IpAddress>,
        port: impl Into<Port>,
    ) -> Result<()> {
        let endpoint = IpEndpoint::new(address.into(), port.into()).into_smoltcp();

        self.context
            .stack
            .with_mutable(|stack| {
                let local_port = stack.get_next_port().into_inner();

                let socket: &mut tcp::Socket<'static> = stack.sockets.get_mut(self.context.handle);

                match socket.connect(stack.interface.context(), endpoint, local_port) {
                    Ok(()) => Ok(()),
                    Err(tcp::ConnectError::InvalidState) => Err(Error::InvalidState),
                    Err(tcp::ConnectError::Unaddressable) => Err(Error::NoRoute),
                }
            })
            .await?;

        poll_fn(|cx| {
            self.poll_with_mutable(cx, |socket, cx| match socket.state() {
                tcp::State::Closed | tcp::State::TimeWait => {
                    Poll::Ready(Err(Error::ConnectionReset))
                }
                tcp::State::Listen => unreachable!(),
                tcp::State::SynSent | tcp::State::SynReceived => {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
                _ => Poll::Ready(Ok(())),
            })
        })
        .await
    }

    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        poll_fn(|cx| {
            self.poll_with_mutable(cx, |s, cx| match s.recv_slice(buffer) {
                Ok(0) if buffer.is_empty() => Poll::Ready(Ok(0)),
                Ok(0) if !s.may_recv() => Poll::Ready(Ok(0)),
                Ok(0) => {
                    s.register_recv_waker(cx.waker());
                    Poll::Pending
                }
                Ok(n) => Poll::Ready(Ok(n)),
                Err(tcp::RecvError::Finished) => Poll::Ready(Ok(0)),
                Err(tcp::RecvError::InvalidState) => Poll::Ready(Err(Error::ConnectionReset)),
            })
        })
        .await
    }

    pub async fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        poll_fn(|cx| {
            self.poll_with_mutable(cx, |s, cx| match s.send_slice(buffer) {
                Ok(0) => {
                    s.register_send_waker(cx.waker());
                    Poll::Pending
                }
                Ok(n) => Poll::Ready(Ok(n)),
                Err(tcp::SendError::InvalidState) => Poll::Ready(Err(Error::ConnectionReset)),
            })
        })
        .await
    }

    pub async fn flush(&mut self) -> Result<()> {
        poll_fn(|cx| {
            self.poll_with_mutable(cx, |s, cx| {
                let data_pending = (s.send_queue() > 0) && s.state() != tcp::State::Closed;
                let fin_pending = matches!(
                    s.state(),
                    tcp::State::FinWait1 | tcp::State::Closing | tcp::State::LastAck
                );
                let rst_pending = s.state() == tcp::State::Closed && s.remote_endpoint().is_some();

                if data_pending || fin_pending || rst_pending {
                    s.register_send_waker(cx.waker());
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            })
        })
        .await
    }

    pub async fn close(&mut self) {
        self.context.closed = true;
        self.with_mutable(tcp::Socket::close).await
    }

    pub async fn close_forced(&mut self) {
        self.context.closed = true;
        self.with_mutable(tcp::Socket::abort).await
    }

    pub async fn get_read_capacity(&self) -> usize {
        self.with(tcp::Socket::recv_capacity).await
    }

    pub async fn get_write_capacity(&self) -> usize {
        self.with(tcp::Socket::send_capacity).await
    }

    pub async fn get_write_queue_size(&self) -> usize {
        self.with(tcp::Socket::send_queue).await
    }

    pub async fn get_read_queue_size(&self) -> usize {
        self.with(tcp::Socket::recv_queue).await
    }

    pub async fn get_local_endpoint(&self) -> Result<Option<(IpAddress, Port)>> {
        let endpoint = self.with(tcp::Socket::local_endpoint).await;

        Ok(endpoint.map(|e| (IpAddress::from_smoltcp(&e.addr), Port::from_inner(e.port))))
    }

    pub async fn get_remote_endpoint(&self) -> Result<Option<(IpAddress, Port)>> {
        let endpoint = self.with(tcp::Socket::remote_endpoint).await;

        Ok(endpoint.map(|e| (IpAddress::from_smoltcp(&e.addr), Port::from_inner(e.port))))
    }

    pub async fn set_keep_alive(&mut self, keep_alive: Option<Duration>) {
        let keep_alive = keep_alive.map(Duration::into_smoltcp);
        self.with_mutable(|socket| socket.set_keep_alive(keep_alive))
            .await
    }

    pub async fn set_hop_limit(&mut self, hop_limit: Option<u8>) {
        self.with_mutable(|socket| socket.set_hop_limit(hop_limit))
            .await
    }

    pub async fn can_read(&self) -> bool {
        self.with(tcp::Socket::can_recv).await
    }

    pub async fn can_write(&self) -> bool {
        self.with(tcp::Socket::can_send).await
    }

    pub async fn may_read(&self) -> bool {
        self.with(tcp::Socket::may_recv).await
    }

    pub async fn may_write(&self) -> bool {
        self.with(tcp::Socket::may_send).await
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        if self.context.closed {
            return;
        }
        log::warning!("TCP socket dropped without being closed. Forcing closure.");
        block_on(self.with_mutable(tcp::Socket::close));
    }
}

#[cfg(test)]
mod tests {

    extern crate std;

    use synchronization::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

    use crate::tests::initialize;

    use super::*;

    static TEST_MUTEX: Mutex<CriticalSectionRawMutex, ()> = Mutex::new(());

    #[task::test]
    async fn test_tcp_connect() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let port = Port::from_inner(51001);
        use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
        let server_ready = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let connection_established = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let client_done = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let server_ready_clone = server_ready.clone();
        let connection_established_clone = connection_established.clone();
        let client_done_clone = client_done.clone();
        let mut listener = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create listener socket");
        let task_manager = crate::tests::task_manager();
        let current_task = task_manager.get_current_task_identifier().await;
        let (listen_task, _) = task_manager
            .spawn(current_task, "TCP Listen Task", None, move |_| async move {
                server_ready_clone.signal(());
                let accept_result = listener.accept(Some([127, 0, 0, 1]), port).await;
                if accept_result.is_err() {
                    return;
                }
                connection_established_clone.signal(());
                client_done_clone.wait().await;
                listener.close_forced().await;
            })
            .await
            .unwrap();
        for _ in 0..5 {
            task::sleep(Duration::from_milliseconds(20)).await;
        }
        server_ready.wait().await;
        task::sleep(Duration::from_milliseconds(200)).await;
        let mut client = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create client socket");
        let connect_result = client.connect([127, 0, 0, 1], port).await;
        if connect_result.is_err() {
            return;
        }
        connection_established.wait().await;
        let _endpoint = client.get_local_endpoint().await;
        task::sleep(Duration::from_milliseconds(100)).await;
        client.close_forced().await;
        client_done.signal(());
        listen_task.join().await;
    }

    #[task::test]
    async fn test_tcp_send_receive() {
        use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let port = Port::from_inner(51002);
        let mut listener = network_manager
            .new_tcp_socket(2048, 2048, None)
            .await
            .expect("Failed to create listener");
        let server_ready = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let server_ready_clone = server_ready.clone();
        let task_manager = crate::tests::task_manager();
        let current_task = task_manager.get_current_task_identifier().await;
        let (_server_task, _) = task_manager
            .spawn(current_task, "TCP Server Task", None, move |_| async move {
                server_ready_clone.signal(());
                let accept_result = listener.accept(Some([127, 0, 0, 1]), port).await;
                if accept_result.is_err() {
                    return;
                }
                let mut buffer = [0u8; 1024];
                match listener.read(&mut buffer).await {
                    Ok(size) => {
                        assert_eq!(&buffer[..size], b"Hello, TCP!", "Received data mismatch");
                    }
                    Err(_) => {
                        return;
                    }
                }
                let response = b"Hello back!";
                if let Err(_) = listener.write(response).await {
                    return;
                }
                if let Err(_) = listener.flush().await {
                    return;
                }
                task::sleep(Duration::from_milliseconds(100)).await;
                listener.close_forced().await;
            })
            .await
            .unwrap();
        server_ready.wait().await;
        let mut client = network_manager
            .new_tcp_socket(2048, 2048, None)
            .await
            .expect("Failed to create client");
        let connect_result = client.connect([127, 0, 0, 1], port).await;
        if connect_result.is_err() {
            return;
        }
        task::sleep(Duration::from_milliseconds(100)).await;
        if let Err(_) = client.write(b"Hello, TCP!").await {
            return;
        }
        if let Err(_) = client.flush().await {
            return;
        }
        let mut response_buffer = [0u8; 1024];
        match client.read(&mut response_buffer).await {
            Ok(size) => {
                assert_eq!(
                    &response_buffer[..size],
                    b"Hello back!",
                    "Response data mismatch"
                );
            }
            Err(_) => {
                return;
            }
        }
        client.close_forced().await;
    }

    #[task::test]
    async fn test_tcp_endpoints() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let mut socket = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create socket");
        let local = socket
            .get_local_endpoint()
            .await
            .expect("Failed to get local endpoint");
        let remote = socket
            .get_remote_endpoint()
            .await
            .expect("Failed to get remote endpoint");
        assert!(
            local.is_none(),
            "TCP endpoint | Local endpoint should be None before connection"
        );
        assert!(
            remote.is_none(),
            "Remote endpoint should be None before connection"
        );
        let port = Port::from_inner(51003);
        let mut listener = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create listener");
        use synchronization::{Arc, blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
        let server_ready = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let connection_ready = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let endpoints_checked = Arc::new(Signal::<CriticalSectionRawMutex, ()>::new());
        let server_ready_clone = server_ready.clone();
        let connection_ready_clone = connection_ready.clone();
        let endpoints_checked_clone = endpoints_checked.clone();
        let task_manager = crate::tests::task_manager();
        let current_task = task_manager.get_current_task_identifier().await;
        let (listen_task, _) = task_manager
            .spawn(
                current_task,
                "TCP Endpoint Listen",
                None,
                move |_| async move {
                    server_ready_clone.signal(());
                    let accept_result = listener.accept(Some([127, 0, 0, 1]), port).await;
                    if accept_result.is_err() {
                        return;
                    }
                    connection_ready_clone.signal(());
                    endpoints_checked_clone.wait().await;
                    task::sleep(Duration::from_milliseconds(100)).await;
                    listener.close_forced().await;
                },
            )
            .await
            .unwrap();
        for _ in 0..5 {
            task::sleep(Duration::from_milliseconds(20)).await;
        }
        server_ready.wait().await;
        task::sleep(Duration::from_milliseconds(200)).await;
        let connect_result = socket.connect([127, 0, 0, 1], port).await;
        if connect_result.is_err() {
            return;
        }
        task::sleep(Duration::from_milliseconds(50)).await;
        connection_ready.wait().await;
        task::sleep(Duration::from_milliseconds(50)).await;
        let local = socket
            .get_local_endpoint()
            .await
            .expect("Failed to get local endpoint");
        let remote = socket
            .get_remote_endpoint()
            .await
            .expect("Failed to get remote endpoint");
        assert!(
            local.is_some(),
            "Local endpoint should be set after connection"
        );
        assert!(
            remote.is_some(),
            "Remote endpoint should be set after connection"
        );
        if let Some((addr, p)) = remote {
            assert_eq!(
                addr,
                IpAddress::from([127, 0, 0, 1]),
                "Remote address mismatch"
            );
            assert_eq!(p, port, "Remote port mismatch");
        }
        endpoints_checked.signal(());
        task::sleep(Duration::from_milliseconds(100)).await;
        socket.close_forced().await;
        listen_task.join().await;
    }

    #[task::test]
    async fn test_tcp_capacities() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let tx_buffer = 2048;
        let rx_buffer = 1024;
        let mut socket = network_manager
            .new_tcp_socket(tx_buffer, rx_buffer, None)
            .await
            .expect("Failed to create socket");
        let read_cap = socket.get_read_capacity().await;
        let write_cap = socket.get_write_capacity().await;
        let read_queue = socket.get_read_queue_size().await;
        let write_queue = socket.get_write_queue_size().await;
        assert_eq!(read_cap, rx_buffer, "Read capacity mismatch");
        assert_eq!(write_cap, tx_buffer, "Write capacity mismatch");
        assert_eq!(read_queue, 0, "Read queue should be empty initially");
        assert_eq!(write_queue, 0, "Write queue should be empty initially");
        socket.close_forced().await;
    }

    #[task::test]
    async fn test_tcp_flush() {
        let _lock = TEST_MUTEX.lock().await;
        let network_manager = initialize().await;
        let port = Port::from_inner(51004);
        let mut listener = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create listener");
        let task_manager = crate::tests::task_manager();
        let current_task = task_manager.get_current_task_identifier().await;
        let (_server_task, _) = task_manager
            .spawn(
                current_task,
                "TCP Flush Server",
                None,
                move |_| async move {
                    let accept_result = listener.accept(Some([127, 0, 0, 1]), port).await;
                    if accept_result.is_err() {
                        return;
                    }
                    let mut buffer = [0u8; 1024];
                    let _ = listener.read(&mut buffer).await;
                    task::sleep(Duration::from_milliseconds(200)).await;
                    listener.close_forced().await;
                },
            )
            .await
            .unwrap();
        task::sleep(Duration::from_milliseconds(300)).await;
        let mut client = network_manager
            .new_tcp_socket(1024, 1024, None)
            .await
            .expect("Failed to create client");
        let connect_result = client.connect([127, 0, 0, 1], port).await;
        if connect_result.is_err() {
            return;
        }
        task::sleep(Duration::from_milliseconds(100)).await;
        let write_result = client.write(b"Test data").await;
        if write_result.is_err() {
            return;
        }
        if let Err(_) = client.flush().await {
            return;
        }
        task::sleep(Duration::from_milliseconds(200)).await;
        client.close_forced().await;
    }
}
