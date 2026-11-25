mod context;
mod device;
mod runner;
mod stack;

use core::future::poll_fn;

use alloc::{vec, vec::Vec};
pub use context::*;
use file_system::{DirectCharacterDevice, IntoIoErrorKind, Path};
pub use runner::*;
use smoltcp::{
    phy::Device,
    socket::{icmp, tcp, udp},
};
use synchronization::{
    Arc, blocking_mutex::raw::CriticalSectionRawMutex, rwlock::RwLock, signal::Signal,
};
use task::{SpawnerIdentifier, TaskIdentifier};
use virtual_file_system::VirtualFileSystem;

use crate::{
    DnsQueryKind, IcmpSocket, IoErrorKind, IpAddress, ResolveError, ResolveResult, Result,
    TcpSocket, UdpSocket,
    manager::{
        device::NetworkDevice,
        stack::{Stack, StackInner},
    },
};

pub fn get_smoltcp_time(time_manager: &time::Manager<'_>) -> smoltcp::time::Instant {
    // smoltcp stores signed microseconds. Saturate only at its representable
    // limit rather than wrapping into a negative timestamp.
    let micros = i64::try_from(time_manager.monotonic_now().as_micros()).unwrap_or(i64::MAX);
    smoltcp::time::Instant::from_micros(micros)
}

type StackList = Vec<Stack>;

pub struct Manager {
    task_manager: &'static task::Manager,
    virtual_file_system: &'static VirtualFileSystem,
    time_manager: &'static time::Manager<'static>,
    pub(crate) random_device: &'static dyn DirectCharacterDevice,
    pub(crate) stacks: RwLock<CriticalSectionRawMutex, StackList>,
}

#[cfg(test)]
pub(crate) static TEST_MANAGER: synchronization::once_lock::OnceLock<Manager> =
    synchronization::once_lock::OnceLock::new();

#[cfg(test)]
pub(crate) fn test_manager() -> &'static Manager {
    TEST_MANAGER
        .try_get()
        .expect("Network test manager not initialized")
}

pub fn initialize(
    task_manager: &'static task::Manager,
    virtual_file_system: &'static VirtualFileSystem,
    time_manager: &'static time::Manager<'static>,
    random_device: &'static dyn DirectCharacterDevice,
) -> Manager {
    Manager::new(
        task_manager,
        virtual_file_system,
        time_manager,
        random_device,
    )
}

impl Manager {
    pub fn new(
        task_manager: &'static task::Manager,
        virtual_file_system: &'static VirtualFileSystem,
        time_manager: &'static time::Manager<'static>,
        random_device: &'static dyn DirectCharacterDevice,
    ) -> Self {
        Manager {
            task_manager,
            virtual_file_system,
            time_manager,
            random_device,
            stacks: RwLock::new(Vec::new()),
        }
    }

    fn generate_seed(&self) -> Result<u64> {
        let mut buffer = [0u8; 8];
        self.random_device
            .read_sync(&mut buffer, 0)
            .map_err(|_| IoErrorKind::InputOutput)?;
        Ok(u64::from_le_bytes(buffer))
    }

    pub(crate) async fn find_first_available_stack(stacks: &StackList) -> Option<Stack> {
        for stack in stacks {
            let available = stack.with(|s| s.is_available()).await;

            if available {
                return Some(stack.clone());
            }
        }

        None
    }

    pub(crate) async fn find_stack(stacks: &StackList, name: &str) -> Option<Stack> {
        for stack in stacks {
            if stack.with(|s| s.name == name).await {
                return Some(stack.clone());
            }
        }

        None
    }

    /// Select the named interface, or the first available one.
    ///
    /// An unknown name is `DeviceUnavailable`; having no usable interface at all
    /// is `NetworkDown`, so callers can tell a typo from a missing network.
    async fn select_stack(stacks: &StackList, interface_name: Option<&str>) -> Result<Stack> {
        match interface_name {
            Some(name) => Self::find_stack(stacks, name)
                .await
                .ok_or(IoErrorKind::DeviceUnavailable),
            None => Self::find_first_available_stack(stacks)
                .await
                .ok_or(IoErrorKind::NetworkDown),
        }
    }

    pub async fn mount_interface(
        &self,
        task: TaskIdentifier,
        name: &str,
        mut device: impl Device + 'static,
        controller_device: impl DirectCharacterDevice + 'static,
        spawner: Option<SpawnerIdentifier>,
    ) -> Result<()> {
        let mut stacks = self.stacks.write().await;

        if Self::find_stack(&stacks, name).await.is_some() {
            return Err(IoErrorKind::AlreadyExists);
        }

        let random_seed = self.generate_seed()?;
        let now = get_smoltcp_time(self.time_manager);

        let stack_inner = StackInner::new(
            name,
            &mut device,
            controller_device,
            random_seed,
            now,
            self.time_manager,
        );

        // Create a wake signal for runner/stack communication
        let wake_signal: WakeSignal = Arc::new(Signal::new());

        let stack = Stack::new(stack_inner, wake_signal.clone());

        let mut runner = StackRunner::new(stack.clone(), device, wake_signal);

        self.task_manager
            .spawn(
                task,
                "Network Interface Runner",
                spawner,
                move |_| async move {
                    runner.run().await;
                },
            )
            .await
            .map_err(IntoIoErrorKind::into_io_error_kind)?;

        let path = Path::NETWORK_DEVICES
            .join(Path::from_str(name))
            .ok_or(IoErrorKind::InvalidArgument)?;

        let device = NetworkDevice::new(stack.clone());

        match self
            .virtual_file_system
            .create_directory(task, &Path::NETWORK_DEVICES)
            .await
        {
            Ok(_) => {}
            Err(IoErrorKind::AlreadyExists) => {}
            Err(e) => return Err(e),
        };

        match self.virtual_file_system.remove(task, &path).await {
            Ok(_) => {}
            Err(IoErrorKind::NotFound) => {}
            Err(e) => return Err(e),
        };

        self.virtual_file_system
            .mount_character_device(task, path, device)
            .await?;

        stacks.push(stack);

        Ok(())
    }

    pub async fn resolve(
        &self,
        host: &str,
        kind: DnsQueryKind,
        stop_on_first_match: bool,
        interface_name: Option<&str>,
    ) -> ResolveResult<Vec<IpAddress>> {
        if let Ok(host) = IpAddress::try_from(host) {
            return Ok(vec![host]);
        }

        let stacks = self.stacks.read().await;

        let stack =
            Self::select_stack(&stacks, interface_name)
                .await
                .map_err(|error| match error {
                    IoErrorKind::DeviceUnavailable => ResolveError::NoInterface,
                    _ => ResolveError::NetworkDown,
                })?;

        resolve_queries(kind, stop_on_first_match, async |query_kind| {
            let handle = stack
                .with_mutable(|s| s.start_dns_query(host, query_kind))
                .await?;

            poll_fn(|cx| {
                stack
                    .poll_with_mutable(cx, |s, cx| s.get_dns_query_result(handle, Some(cx.waker())))
            })
            .await
        })
        .await
    }

    /// Whether the selected interface, or the first available one, has at
    /// least one IP address: the signal that the network is up for clients.
    pub async fn has_ip_address(&self, interface_name: Option<&str>) -> bool {
        let stacks = self.stacks.read().await;
        match Self::select_stack(&stacks, interface_name).await {
            Ok(stack) => stack.with(|s| s.get_ip_addresses_count() > 0).await,
            Err(_) => false,
        }
    }

    pub async fn new_tcp_socket(
        &self,
        transmit_buffer_size: usize,
        receive_buffer_size: usize,
        interface_name: Option<&str>,
    ) -> Result<TcpSocket> {
        let stacks = self.stacks.read().await;

        let stack = Self::select_stack(&stacks, interface_name).await?;

        let send_buffer = tcp::SocketBuffer::new(vec![0u8; transmit_buffer_size]);
        let receive_buffer = tcp::SocketBuffer::new(vec![0u8; receive_buffer_size]);

        let socket = tcp::Socket::new(receive_buffer, send_buffer);
        let handle = stack.with_mutable(|s| s.add_socket(socket)).await;

        let context = SocketContext {
            handle,
            stack: stack.clone(),
            closed: false,
            was_connected: false,
        };

        Ok(TcpSocket::new(context))
    }

    pub async fn new_udp_socket(
        &self,
        transmit_buffer_size: usize,
        receive_buffer_size: usize,
        receive_meta_buffer_size: usize,
        transmit_meta_buffer_size: usize,
        interface_name: Option<&str>,
    ) -> Result<UdpSocket> {
        let stacks = self.stacks.read().await;

        let stack = Self::select_stack(&stacks, interface_name).await?;

        let receive_meta_buffer = udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; receive_meta_buffer_size],
            vec![0u8; receive_buffer_size],
        );
        let transmit_meta_buffer = udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; transmit_meta_buffer_size],
            vec![0u8; transmit_buffer_size],
        );

        let socket = udp::Socket::new(receive_meta_buffer, transmit_meta_buffer);
        let handle = stack.with_mutable(|s| s.add_socket(socket)).await;

        let context = SocketContext {
            handle,
            stack: stack.clone(),
            closed: false,
            was_connected: false,
        };

        Ok(UdpSocket::new(context))
    }

    pub async fn new_icmp_socket(
        &self,
        receive_buffer_size: usize,
        transmit_buffer_size: usize,
        receive_meta_buffer_size: usize,
        transmit_meta_buffer_size: usize,
        interface_name: Option<&str>,
    ) -> Result<IcmpSocket> {
        let stacks = self.stacks.read().await;

        let stack = Self::select_stack(&stacks, interface_name).await?;

        let receive_buffer = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; receive_meta_buffer_size],
            vec![0u8; receive_buffer_size],
        );
        let transmit_buffer = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; transmit_meta_buffer_size],
            vec![0u8; transmit_buffer_size],
        );

        let socket = icmp::Socket::new(receive_buffer, transmit_buffer);
        let handle = stack.with_mutable(|s| s.add_socket(socket)).await;

        let context = SocketContext {
            handle,
            stack: stack.clone(),
            closed: false,
            was_connected: false,
        };

        let socket = IcmpSocket::new(context);

        Ok(socket)
    }
}

async fn resolve_queries(
    kind: DnsQueryKind,
    stop_on_first_match: bool,
    mut query: impl AsyncFnMut(DnsQueryKind) -> ResolveResult<Vec<IpAddress>>,
) -> ResolveResult<Vec<IpAddress>> {
    let query_types = [
        DnsQueryKind::A,
        DnsQueryKind::Aaaa,
        DnsQueryKind::Cname,
        DnsQueryKind::Soa,
        DnsQueryKind::Ns,
    ];

    let mut resolved_addresses = vec![];
    let mut query_succeeded = false;
    let mut query_error = None;

    for query_kind in query_types {
        if !kind.contains(query_kind) {
            continue;
        }

        match query(query_kind).await {
            Ok(addresses) => {
                query_succeeded = true;
                resolved_addresses.extend(addresses);

                if stop_on_first_match && !resolved_addresses.is_empty() {
                    break;
                }
            }
            Err(error) => {
                // Prefer a specific cause over smoltcp's generic query failure.
                if query_error.is_none() || query_error == Some(ResolveError::Failure) {
                    query_error = Some(error);
                }
            }
        }
    }

    if !query_succeeded && let Some(error) = query_error {
        return Err(error);
    }

    Ok(resolved_addresses)
}

#[cfg(test)]
#[allow(
    clippy::disallowed_types,
    reason = "host-only unit tests use std atomics"
)]
mod clock_tests {
    extern crate std;

    use super::*;
    use core::time::Duration;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Clock(AtomicU64);
    impl time::MonotonicClock for Clock {
        fn now(&self) -> Duration {
            Duration::from_micros(self.0.load(Ordering::Relaxed))
        }
    }

    #[test]
    fn smoltcp_uses_monotonic_microseconds_and_saturates_without_rewinding() {
        let clock = Clock(AtomicU64::new(10_123));
        let manager =
            time::Manager::with_monotonic_clock(&testing_support::TimeDevice, &clock).unwrap();
        assert_eq!(get_smoltcp_time(&manager).total_micros(), 10_123);
        clock.0.store(i64::MAX as u64 + 1, Ordering::Relaxed);
        assert_eq!(get_smoltcp_time(&manager).total_micros(), i64::MAX);
    }
}

#[cfg(test)]
mod dns_tests {
    use embassy_futures::block_on;

    use super::*;

    #[test]
    fn resolves_aaaa_after_a_fails() {
        let ipv6 = IpAddress::from([0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        let mut queries = vec![];

        let result = block_on(resolve_queries(
            DnsQueryKind::A | DnsQueryKind::Aaaa,
            true,
            async |kind| {
                queries.push(kind);

                match kind {
                    DnsQueryKind::A => Err(ResolveError::Failure),
                    DnsQueryKind::Aaaa => Ok(vec![ipv6.clone()]),
                    _ => panic!("unexpected query: {kind:?}"),
                }
            },
        ));

        assert_eq!(queries, [DnsQueryKind::A, DnsQueryKind::Aaaa]);
        assert_eq!(result, Ok(vec![ipv6]));
    }

    #[test]
    fn stops_after_first_address_when_requested() {
        let ipv4 = IpAddress::from([192, 0, 2, 1]);
        let ipv6 = IpAddress::from([0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);

        for stop_on_first_match in [true, false] {
            let result = block_on(resolve_queries(
                DnsQueryKind::A | DnsQueryKind::Aaaa,
                stop_on_first_match,
                async |kind| match kind {
                    DnsQueryKind::A => Ok(vec![ipv4.clone()]),
                    DnsQueryKind::Aaaa => Ok(vec![ipv6.clone()]),
                    _ => panic!("unexpected query: {kind:?}"),
                },
            ));

            let expected = if stop_on_first_match {
                vec![ipv4.clone()]
            } else {
                vec![ipv4.clone(), ipv6.clone()]
            };
            assert_eq!(result, Ok(expected));
        }
    }

    #[test]
    fn successful_query_with_no_addresses_still_counts_as_success() {
        let result = block_on(resolve_queries(
            DnsQueryKind::A | DnsQueryKind::Aaaa,
            true,
            async |kind| match kind {
                DnsQueryKind::A => Ok(vec![]),
                DnsQueryKind::Aaaa => Err(ResolveError::Failure),
                _ => panic!("unexpected query: {kind:?}"),
            },
        ));

        assert_eq!(result, Ok(vec![]));
    }

    #[test]
    fn returns_specific_error_only_if_every_query_fails() {
        for errors in [
            [ResolveError::Failure, ResolveError::NameTooLong],
            [ResolveError::NameTooLong, ResolveError::Failure],
        ] {
            let result = block_on(resolve_queries(
                DnsQueryKind::A | DnsQueryKind::Aaaa,
                false,
                async |kind| match kind {
                    DnsQueryKind::A => Err(errors[0]),
                    DnsQueryKind::Aaaa => Err(errors[1]),
                    _ => panic!("unexpected query: {kind:?}"),
                },
            ));
            assert_eq!(result, Err(ResolveError::NameTooLong));
        }
    }

    #[test]
    fn an_unknown_interface_is_not_a_missing_network() {
        let stacks = StackList::new();
        assert_eq!(
            block_on(Manager::select_stack(&stacks, Some("wlan9"))).err(),
            Some(IoErrorKind::DeviceUnavailable)
        );
        assert_eq!(
            block_on(Manager::select_stack(&stacks, None)).err(),
            Some(IoErrorKind::NetworkDown)
        );
    }

    #[test]
    fn every_query_failing_with_the_generic_failure_stays_generic() {
        let result = block_on(resolve_queries(
            DnsQueryKind::A | DnsQueryKind::Aaaa,
            false,
            async |_| Err(ResolveError::Failure),
        ));
        // Never promoted to a confirmed negative answer without evidence.
        assert_eq!(result, Err(ResolveError::Failure));
    }
}
