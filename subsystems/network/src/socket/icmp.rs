use crate::{Duration, Error, IpAddress, Port, Result, SocketContext};
use alloc::vec;
use core::{
    future::poll_fn,
    task::{Context, Poll},
};
use smoltcp::{socket::icmp, wire};

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum IcmpEndpoint {
    #[default]
    Unspecified,
    Identifier(u16),
    Udp((Option<IpAddress>, Port)),
}

impl IcmpEndpoint {
    pub const fn into_smoltcp(&self) -> icmp::Endpoint {
        match self {
            IcmpEndpoint::Unspecified => icmp::Endpoint::Unspecified,
            IcmpEndpoint::Identifier(id) => icmp::Endpoint::Ident(*id),
            IcmpEndpoint::Udp((addr_opt, port)) => {
                let addr = match addr_opt {
                    Some(a) => Some(a.into_smoltcp()),
                    None => None,
                };

                let endpoint = wire::IpListenEndpoint {
                    addr,
                    port: port.into_inner(),
                };

                icmp::Endpoint::Udp(endpoint)
            }
        }
    }
}

pub struct IcmpSocket {
    context: SocketContext,
}

impl IcmpSocket {
    pub(crate) fn new(context: SocketContext) -> Self {
        Self { context }
    }

    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&icmp::Socket<'static>) -> R,
    {
        self.context.with(f).await
    }

    pub async fn with_mutable<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut icmp::Socket<'static>) -> R,
    {
        self.context.with_mutable(f).await
    }

    pub fn poll_with<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&icmp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with(context, f)
    }

    pub fn poll_with_mutable<F, R>(&self, context: &mut Context<'_>, f: F) -> Poll<R>
    where
        F: FnOnce(&mut icmp::Socket, &mut Context<'_>) -> Poll<R>,
    {
        self.context.poll_with_mutable(context, f)
    }

    fn poll_send_to(
        &self,
        context: &mut Context<'_>,
        buffer: &[u8],
        remote_endpoint: &IpAddress,
    ) -> Poll<Result<()>> {
        self.poll_with_mutable(context, |socket, context| {
            let send_capacity_too_small = socket.payload_send_capacity() < buffer.len();
            if send_capacity_too_small {
                return Poll::Ready(Err(Error::PacketTooLarge));
            }

            let remote_endpoint = remote_endpoint.into_smoltcp();

            match socket.send_slice(buffer, remote_endpoint) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(icmp::SendError::BufferFull) => {
                    socket.register_send_waker(context.waker());
                    Poll::Pending
                }
                Err(icmp::SendError::Unaddressable) => {
                    if socket.is_open() {
                        Poll::Ready(Err(Error::NoRoute))
                    } else {
                        Poll::Ready(Err(Error::SocketNotBound))
                    }
                }
            }
        })
    }

    fn poll_receive_from(
        &self,
        context: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<Result<(usize, IpAddress)>> {
        self.poll_with_mutable(context, |socket, context| match socket.recv_slice(buffer) {
            Ok((size, remote_endpoint)) => {
                let remote_endpoint = IpAddress::from_smoltcp(&remote_endpoint);
                Poll::Ready(Ok((size, remote_endpoint)))
            }
            Err(icmp::RecvError::Truncated) => Poll::Ready(Err(Error::Truncated)),
            Err(icmp::RecvError::Exhausted) => {
                socket.register_recv_waker(context.waker());

                self.context.stack.wake_runner();
                Poll::Pending
            }
        })
    }

    pub async fn bind(&self, endpoint: IcmpEndpoint) -> Result<()> {
        let endpoint = endpoint.into_smoltcp();

        self.with_mutable(|socket: &mut icmp::Socket| socket.bind(endpoint))
            .await?;
        Ok(())
    }

    pub async fn can_write(&self) -> bool {
        self.with(icmp::Socket::can_send).await
    }

    pub async fn can_read(&self) -> bool {
        self.with(icmp::Socket::can_recv).await
    }

    pub async fn write_to(&self, buffer: &[u8], endpoint: impl Into<IpAddress>) -> Result<()> {
        let address: IpAddress = endpoint.into();

        poll_fn(|context| self.poll_send_to(context, buffer, &address)).await
    }

    pub async fn read_from(&self, buffer: &mut [u8]) -> Result<(usize, IpAddress)> {
        poll_fn(|context| self.poll_receive_from(context, buffer)).await
    }

    pub async fn read_from_with_timeout(
        &self,
        buffer: &mut [u8],
        timeout: impl Into<Duration>,
    ) -> Result<(usize, IpAddress)> {
        use embassy_futures::select::{Either, select};

        let receive = poll_fn(|context| self.poll_receive_from(context, buffer));
        let sleep = task::sleep(timeout.into());

        match select(receive, sleep).await {
            Either::First(result) => result,
            Either::Second(_) => Err(Error::TimedOut),
        }
    }

    /// Sends an ICMP echo request (ping) to the specified remote address and waits for a reply.
    /// Returns the round-trip time if successful.
    ///
    /// # Errors
    ///
    /// Returns an error if the ping request fails or times out.
    pub async fn ping(
        &self,
        remote_address: &IpAddress,
        sequence_number: u16,
        identifier: u16,
        timeout: Duration,
        payload_size: usize,
    ) -> Result<Duration> {
        use wire::{Icmpv4Packet, Icmpv4Repr, Icmpv6Packet, Icmpv6Repr};

        let mut echo_payload = vec![0u8; payload_size];
        let start_time = crate::get_smoltcp_time(self.context.stack.time_manager());

        let timestamp_millis = start_time.total_millis() as u64;
        echo_payload[0..8].copy_from_slice(&timestamp_millis.to_be_bytes());

        let mut stack_lock = self.context.stack.lock().await;

        let src_addr_v6 = if let IpAddress::IPv6(v6_addr) = remote_address {
            Some(
                stack_lock
                    .interface
                    .get_source_address_ipv6(&v6_addr.into_smoltcp()),
            )
        } else {
            None
        };

        let socket = stack_lock
            .sockets
            .get_mut::<icmp::Socket>(self.context.handle);

        let remote_endpoint = remote_address.into_smoltcp();

        let checksum_caps = smoltcp::phy::ChecksumCapabilities::default();

        match remote_address {
            IpAddress::IPv4(_) => {
                let icmp_repr = Icmpv4Repr::EchoRequest {
                    ident: identifier,
                    seq_no: sequence_number,
                    data: &echo_payload,
                };

                let icmp_payload = socket
                    .send(icmp_repr.buffer_len(), remote_endpoint)
                    .map_err(|e| match e {
                        icmp::SendError::BufferFull => Error::ResourceBusy,
                        icmp::SendError::Unaddressable => Error::NoRoute,
                    })?;

                let mut icmp_packet = Icmpv4Packet::new_unchecked(icmp_payload);
                icmp_repr.emit(&mut icmp_packet, &checksum_caps);
            }
            IpAddress::IPv6(v6_addr) => {
                let icmp_repr = Icmpv6Repr::EchoRequest {
                    ident: identifier,
                    seq_no: sequence_number,
                    data: &echo_payload,
                };

                let icmp_payload = socket
                    .send(icmp_repr.buffer_len(), remote_endpoint)
                    .map_err(|e| match e {
                        icmp::SendError::BufferFull => Error::ResourceBusy,
                        icmp::SendError::Unaddressable => Error::NoRoute,
                    })?;

                let src_addr = src_addr_v6.unwrap();
                let mut icmp_packet = Icmpv6Packet::new_unchecked(icmp_payload);
                icmp_repr.emit(
                    &src_addr,
                    &v6_addr.into_smoltcp(),
                    &mut icmp_packet,
                    &checksum_caps,
                );
            }
        }

        drop(stack_lock);

        self.context.stack.wake_runner();

        let timeout_end = start_time + timeout.into_smoltcp();

        loop {
            let now = crate::get_smoltcp_time(self.context.stack.time_manager());
            if now >= timeout_end {
                return Err(Error::TimedOut);
            }

            let mut recv_buffer = [0u8; 256];
            let result = self.read_from_with_timeout(&mut recv_buffer, timeout).await;

            match result {
                Ok((size, addr)) if addr == *remote_address => {
                    // Parse the received packet
                    let is_valid_reply = match remote_address {
                        IpAddress::IPv4(_) => {
                            if let Ok(packet) = Icmpv4Packet::new_checked(&recv_buffer[..size]) {
                                if let Ok(repr) = Icmpv4Repr::parse(&packet, &checksum_caps) {
                                    matches!(
                                        repr,
                                        Icmpv4Repr::EchoReply {
                                            ident: id,
                                            seq_no,
                                            ..
                                        } if id == identifier && seq_no == sequence_number
                                    )
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                        IpAddress::IPv6(v6_addr) => {
                            if let Ok(packet) = Icmpv6Packet::new_checked(&recv_buffer[..size]) {
                                let src_addr = self
                                    .context
                                    .stack
                                    .with_mutable(|s| s.get_source_ip_v6_address(*v6_addr))
                                    .await;

                                if let Ok(repr) = Icmpv6Repr::parse(
                                    &v6_addr.into_smoltcp(),
                                    &src_addr.into_smoltcp(),
                                    &packet,
                                    &checksum_caps,
                                ) {
                                    matches!(
                                        repr,
                                        Icmpv6Repr::EchoReply {
                                            ident: id,
                                            seq_no,
                                            ..
                                        } if id == identifier && seq_no == sequence_number
                                    )
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                    };

                    if is_valid_reply {
                        let end_time = crate::get_smoltcp_time(self.context.stack.time_manager());
                        let rtt = end_time - start_time;
                        return Ok(Duration::from_milliseconds(rtt.total_millis()));
                    }
                }
                Ok(_) => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    pub async fn flush(&self) -> () {
        poll_fn(|context| {
            self.poll_with_mutable(context, |socket, context| {
                if socket.send_queue() == 0 {
                    Poll::Ready(())
                } else {
                    socket.register_send_waker(context.waker());
                    Poll::Pending
                }
            })
        })
        .await
    }

    pub async fn is_open(&self) -> bool {
        self.with(icmp::Socket::is_open).await
    }

    pub async fn get_packet_read_capacity(&self) -> usize {
        self.with(icmp::Socket::packet_recv_capacity).await
    }

    pub async fn get_packet_write_capacity(&self) -> usize {
        self.with(icmp::Socket::packet_send_capacity).await
    }

    pub async fn get_payload_read_capacity(&self) -> usize {
        self.with(icmp::Socket::payload_recv_capacity).await
    }

    pub async fn get_payload_write_capacity(&self) -> usize {
        self.with(icmp::Socket::payload_send_capacity).await
    }

    pub async fn get_hop_limit(&self) -> Option<u8> {
        self.with(icmp::Socket::hop_limit).await
    }

    pub async fn set_hop_limit(&self, hop_limit: Option<u8>) -> () {
        self.with_mutable(|socket: &mut icmp::Socket| {
            socket.set_hop_limit(hop_limit);
        })
        .await
    }
}
