use core::{
    future::poll_fn,
    task::{Context, Poll},
};

use embassy_futures::block_on;
use smoltcp::socket::udp;

use crate::{Error, IpAddress, Port, Result, SocketContext, UdpMetadata};

pub struct UdpSocket {
    context: SocketContext,
}

impl UdpSocket {
    pub(crate) fn new(context: SocketContext) -> Self {
        Self { context }
    }

    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&udp::Socket<'static>) -> R,
    {
        self.context.with(f).await
    }

    pub async fn with_mutable<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut udp::Socket<'static>) -> R,
    {
        self.context.with_mutable(f).await
    }

    pub fn poll_with<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&udp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with(context, f)
    }

    pub fn poll_with_mutable<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&mut udp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with_mutable(context, f)
    }

    pub async fn bind(&mut self, port: Port) -> Result<()> {
        let port = port.into_inner();

        self.with_mutable(|socket| socket.bind(port)).await?;

        Ok(())
    }

    pub fn poll_send_to(
        &mut self,
        context: &mut Context<'_>,
        buffer: &[u8],
        metadata: &UdpMetadata,
    ) -> Poll<Result<()>> {
        log::debug!("UDP poll_send_to: sending {} bytes", buffer.len());
        self.poll_with_mutable(context, |socket, cx| {
            let capacity = socket.payload_send_capacity();
            log::debug!("UDP send capacity: {}, needed: {}", capacity, buffer.len());

            if capacity < buffer.len() {
                log::warning!(
                    "UDP send buffer too small: capacity={}, needed={}",
                    capacity,
                    buffer.len()
                );
                return Poll::Ready(Err(Error::PacketTooLarge));
            }

            let metadata = metadata.to_smoltcp();

            match socket.send_slice(buffer, metadata) {
                Ok(()) => {
                    log::debug!("UDP send_slice succeeded");
                    Poll::Ready(Ok(()))
                }
                Err(udp::SendError::BufferFull) => {
                    log::information!("UDP send buffer full, registering waker");
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
                Err(udp::SendError::Unaddressable) => {
                    if socket.endpoint().port == 0 {
                        log::error!("UDP send failed: socket not bound");
                        Poll::Ready(Err(Error::SocketNotBound))
                    } else {
                        log::error!("UDP send failed: no route");
                        Poll::Ready(Err(Error::NoRoute))
                    }
                }
            }
        })
    }

    pub fn poll_receive_from(
        &self,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<(usize, UdpMetadata)>> {
        log::debug!("UDP poll_receive_from: buffer size={}", buffer.len());
        self.poll_with_mutable(context, |socket, cx| {
            log::debug!(
                "UDP recv: checking socket for data (can_recv={}, buffered={})",
                socket.can_recv(),
                socket.recv_queue()
            );
            match socket.recv_slice(buffer) {
                Ok((n, meta)) => {
                    log::information!("UDP received {} bytes", n);
                    Poll::Ready(Ok((n, UdpMetadata::from_smoltcp(&meta))))
                }
                Err(udp::RecvError::Truncated) => {
                    log::warning!("UDP receive truncated");
                    Poll::Ready(Err(Error::Truncated))
                }
                Err(udp::RecvError::Exhausted) => {
                    log::information!(
                        "UDP receive buffer exhausted (can_recv={}, buffered={})",
                        socket.can_recv(),
                        socket.recv_queue()
                    );
                    socket.register_recv_waker(cx.waker());
                    log::information!("UDP receive waker registered");
                    Poll::Pending
                }
            }
        })
    }

    pub async fn write_to(&mut self, buffer: &[u8], metadata: &UdpMetadata) -> Result<()> {
        log::debug!(
            "UDP write_to: starting async send of {} bytes",
            buffer.len()
        );
        let result = poll_fn(|cx| self.poll_send_to(cx, buffer, metadata)).await;
        log::debug!("UDP write_to: completed with result {:?}", result.is_ok());
        result
    }

    pub async fn read_from(&self, buffer: &mut [u8]) -> Result<(usize, UdpMetadata)> {
        poll_fn(|cx| {
            log::information!("Polling UDP read");
            let r = self.poll_receive_from(cx, buffer);
            log::information!("UDP read poll completed");
            r
        })
        .await
    }

    pub fn flush(&mut self) -> impl Future<Output = ()> + '_ {
        poll_fn(|cx| {
            self.poll_with_mutable(cx, |socket, cx| {
                if socket.can_send() {
                    Poll::Ready(())
                } else {
                    socket.register_send_waker(cx.waker());
                    Poll::Pending
                }
            })
        })
    }

    pub async fn close(mut self) {
        self.context.closed = true;
        self.with_mutable(|s| {
            log::information!("Closing UDP socket : {:?}", s.endpoint());
            udp::Socket::close(s);
            log::information!("UDP socket closed");
        })
        .await;
    }

    pub async fn get_endpoint(&self) -> Result<(Option<IpAddress>, Port)> {
        let endpoint = self.with(udp::Socket::endpoint).await;

        let ip_address = endpoint.addr.as_ref().map(IpAddress::from_smoltcp);
        let port = Port::from_inner(endpoint.port);

        Ok((ip_address, port))
    }

    pub async fn get_packet_read_capacity(&self) -> usize {
        self.with(udp::Socket::packet_recv_capacity).await
    }

    pub async fn get_packet_write_capacity(&self) -> usize {
        self.with(udp::Socket::packet_send_capacity).await
    }

    pub async fn get_payload_read_capacity(&self) -> usize {
        self.with(udp::Socket::payload_recv_capacity).await
    }

    pub async fn get_payload_write_capacity(&self) -> usize {
        self.with(udp::Socket::payload_send_capacity).await
    }

    pub async fn set_hop_limit(&mut self, hop_limit: Option<u8>) {
        self.with_mutable(|socket| socket.set_hop_limit(hop_limit))
            .await
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        if self.context.closed {
            return;
        }
        log::warning!("UDP socket dropped without being closed. Forcing closure.");
        block_on(self.with_mutable(udp::Socket::close));
    }
}

#[cfg(test)]
mod tests {

    extern crate std;

    use crate::tests::initialize;

    use super::*;
    use smoltcp::phy::PacketMeta;

    #[task::test]
    async fn test_udp_bind() {
        let network_manager = initialize().await;

        let mut socket = network_manager
            .new_udp_socket(1024, 1024, 10, 10, None)
            .await
            .expect("Failed to create UDP socket");

        let port = Port::from_inner(10001);
        let result = socket.bind(port).await;

        assert!(result.is_ok(), "Failed to bind UDP socket");

        let (_ip, bound_port) = socket.get_endpoint().await.expect("Failed to get endpoint");
        assert_eq!(bound_port, port, "Port mismatch");

        socket.close().await;
    }

    #[task::test]
    async fn test_udp_send_receive() {
        let network_manager = initialize().await;

        // Create sender socket
        let mut sender = network_manager
            .new_udp_socket(1024, 1024, 10, 10, None)
            .await
            .expect("Failed to create sender socket");

        // Create receiver socket
        let mut receiver = network_manager
            .new_udp_socket(65535, 65535, 10, 10, None)
            .await
            .expect("Failed to create receiver socket");

        // Bind receiver to a specific port
        let receiver_port = Port::from_inner(10003);
        receiver
            .bind(receiver_port)
            .await
            .expect("Failed to bind receiver");

        // Prepare test data
        let test_data = b"Hello, UDP!";

        let remote_ip: IpAddress = [127, 0, 0, 1].into();

        let metadata = UdpMetadata::new(remote_ip, receiver_port, None, PacketMeta::default());

        sender
            .bind(Port::from_inner(10002))
            .await
            .expect("Failed to bind sender");

        log::information!("Sending data");

        // Send data
        let send_result = sender.write_to(test_data, &metadata).await;
        assert_eq!(send_result, Ok(()));

        log::information!("Data sent, waiting to receive...");

        // Receive data
        let mut buffer = [0u8; 1024];
        let receive_result = receiver.read_from(&mut buffer).await;

        log::information!("Data received");

        if let Ok((size, _recv_metadata)) = receive_result {
            assert_eq!(size, test_data.len(), "Received data size mismatch");
            assert_eq!(&buffer[..size], test_data, "Received data mismatch");
        }

        sender.close().await;
        receiver.close().await;
    }

    #[task::test]
    async fn test_udp_endpoint() {
        let network_manager = initialize().await;

        let mut socket = network_manager
            .new_udp_socket(1024, 1024, 10, 10, None)
            .await
            .expect("Failed to create UDP socket");

        // Before binding, endpoint should have port 0
        let (_ip, port) = socket
            .get_endpoint()
            .await
            .expect("Failed to get initial endpoint");
        assert_eq!(port.into_inner(), 0, "Initial port should be 0");

        // After binding, endpoint should have the bound port
        let bind_port = Port::from_inner(10004);
        socket.bind(bind_port).await.expect("Failed to bind");

        let (_, bound_port) = socket
            .get_endpoint()
            .await
            .expect("Failed to get bound endpoint");
        assert_eq!(bound_port, bind_port, "Bound port mismatch");

        socket.close().await;
    }

    #[task::test]
    async fn test_udp_capacities() {
        let network_manager = initialize().await;

        let tx_buffer = 2048;
        let rx_buffer = 1024;
        let rx_meta = 15;
        let tx_meta = 20;

        let socket = network_manager
            .new_udp_socket(tx_buffer, rx_buffer, rx_meta, tx_meta, None)
            .await
            .expect("Failed to create UDP socket");

        let packet_read_cap = socket.get_packet_read_capacity().await;
        let packet_write_cap = socket.get_packet_write_capacity().await;
        let payload_read_cap = socket.get_payload_read_capacity().await;
        let payload_write_cap = socket.get_payload_write_capacity().await;

        assert_eq!(packet_read_cap, rx_meta, "Packet read capacity mismatch");
        assert_eq!(packet_write_cap, tx_meta, "Packet write capacity mismatch");
        assert_eq!(
            payload_read_cap, rx_buffer,
            "Payload read capacity mismatch"
        );
        assert_eq!(
            payload_write_cap, tx_buffer,
            "Payload write capacity mismatch"
        );

        socket.close().await;
    }
}
