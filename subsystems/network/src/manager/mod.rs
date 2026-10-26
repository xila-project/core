mod context;
mod device;
mod runner;
mod stack;

use core::future::poll_fn;

use alloc::{vec, vec::Vec};
pub use context::*;
use file_system::{DirectCharacterDevice, Path};
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
    DnsQueryKind, Error, IcmpSocket, IpAddress, Result, TcpSocket, UdpSocket,
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
            .map_err(Error::FailedToGenerateSeed)?;
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
            let stack_name = stack.with(|s| s.name.clone()).await;

            if stack_name.as_str() == name {
                return Some(stack.clone());
            }
        }

        None
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
            return Err(Error::DuplicateIdentifier);
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
            .map_err(Error::FailedToSpawnNetworkTask)?;

        let path = Path::NETWORK_DEVICES
            .join(Path::from_str(name))
            .ok_or(Error::InvalidIdentifier)?;

        let device = NetworkDevice::new(stack.clone());

        match self
            .virtual_file_system
            .create_directory(task, &Path::NETWORK_DEVICES)
            .await
        {
            Ok(_) => {}
            Err(virtual_file_system::Error::AlreadyExists) => {}
            Err(e) => return Err(Error::FailedToMountDevice(e)),
        };

        match self.virtual_file_system.remove(task, &path).await {
            Ok(_) => {}
            Err(virtual_file_system::Error::FileSystem(file_system::Error::NotFound)) => {}
            Err(e) => return Err(Error::FailedToMountDevice(e)),
        };

        self.virtual_file_system
            .mount_character_device(task, path, device)
            .await
            .map_err(Error::FailedToMountDevice)?;

        stacks.push(stack);

        Ok(())
    }

    pub async fn resolve(
        &self,
        host: &str,
        kind: DnsQueryKind,
        stop_on_first_match: bool,
        interface_name: Option<&str>,
    ) -> Result<Vec<IpAddress>> {
        if let Ok(host) = IpAddress::try_from(host) {
            return Ok(vec![host]);
        }

        let stacks = self.stacks.read().await;

        let stack = if let Some(name) = interface_name {
            Self::find_stack(&stacks, name)
                .await
                .ok_or(Error::NotFound)?
        } else {
            Self::find_first_available_stack(&stacks)
                .await
                .ok_or(Error::NotFound)?
        };

        let query_iterator = &[
            DnsQueryKind::A,
            DnsQueryKind::Aaaa,
            DnsQueryKind::Cname,
            DnsQueryKind::Soa,
            DnsQueryKind::Ns,
        ];

        let mut resolved_addresses = vec![];

        for query_kind in query_iterator {
            if !kind.contains(*query_kind) {
                continue;
            }

            let handle = stack
                .with_mutable(|s| s.start_dns_query(host, kind))
                .await?;

            let result = poll_fn(|cx| {
                stack
                    .poll_with_mutable(cx, |s, cx| s.get_dns_query_result(handle, Some(cx.waker())))
            })
            .await?;

            resolved_addresses.extend(result);

            if !resolved_addresses.is_empty() && stop_on_first_match {
                break;
            }
        }

        Ok(resolved_addresses)
    }

    pub async fn new_tcp_socket(
        &self,
        transmit_buffer_size: usize,
        receive_buffer_size: usize,
        interface_name: Option<&str>,
    ) -> Result<TcpSocket> {
        let stacks = self.stacks.read().await;

        let stack = if let Some(name) = interface_name {
            Self::find_stack(&stacks, name)
                .await
                .ok_or(Error::NotFound)?
        } else {
            Self::find_first_available_stack(&stacks)
                .await
                .ok_or(Error::NotFound)?
        };

        let send_buffer = tcp::SocketBuffer::new(vec![0u8; transmit_buffer_size]);
        let receive_buffer = tcp::SocketBuffer::new(vec![0u8; receive_buffer_size]);

        let socket = tcp::Socket::new(receive_buffer, send_buffer);
        let handle = stack.with_mutable(|s| s.add_socket(socket)).await;

        let context = SocketContext {
            handle,
            stack: stack.clone(),
            closed: false,
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

        let stack = if let Some(name) = interface_name {
            Self::find_stack(&stacks, name)
                .await
                .ok_or(Error::NotFound)?
        } else {
            Self::find_first_available_stack(&stacks)
                .await
                .ok_or(Error::NotFound)?
        };

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

        let stack = if let Some(name) = interface_name {
            Self::find_stack(&stacks, name)
                .await
                .ok_or(Error::NotFound)?
        } else {
            Self::find_first_available_stack(&stacks)
                .await
                .ok_or(Error::NotFound)?
        };

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
        };

        let socket = IcmpSocket::new(context);

        Ok(socket)
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    use core::{
        sync::atomic::{AtomicU64, Ordering},
        time::Duration,
    };

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
            time::Manager::with_monotonic_clock(&drivers_std::devices::TimeDevice, &clock).unwrap();
        assert_eq!(get_smoltcp_time(&manager).total_micros(), 10_123);
        clock.0.store(i64::MAX as u64 + 1, Ordering::Relaxed);
        assert_eq!(get_smoltcp_time(&manager).total_micros(), i64::MAX);
    }
}
