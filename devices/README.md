# Device contracts

`devices/` (`xila-device`, imported as `device`) defines the application-visible VFS device class contracts: control command kind bytes, command identifiers (`file_system::define_command!`), and versioned `#[repr(C)]` ABI payload and configuration structures passed through `File::control`.

It is a dependency-light contract crate that depends only on `file_system` (and small pure crates when strictly needed), never on runtime subsystems, the VFS implementation, or hardware drivers.

Platform-independent VFS device adapters live in `drivers/`, hardware-facing controller traits live in [libraries](../libraries/README.md) (such as `peripherals`), and runtime-owned services live in [subsystems](../subsystems/README.md).
