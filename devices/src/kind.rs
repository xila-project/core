//! Registry of VFS control command kind bytes (`file_system::define_command!`).
//!
//! Each device class uses a distinct 1-byte kind identifier encoded into
//! [`file_system::ControlCommandIdentifier`]. Existing kind bytes are part of
//! the stable VFS control ABI and must not be changed.
//!
//! | Kind byte | Hex    | Device class        | Owning module / crate                    |
//! |-----------|--------|---------------------|------------------------------------------|
//! | `b'A'`    | `0x41` | Audio               | `device::audio` / `audio`                |
//! | `b'B'`    | `0x42` | Block device        | `file_system::block_device`              |
//! | `b'D'`    | `0x44` | Screen / display    | `device::screen` / `graphics`            |
//! | `b'E'`    | `0x45` | Executable          | `executable`                             |
//! | `b'F'`    | `0x46` | Firmware update     | `firmware_update::device`                |
//! | `b'H'`    | `0x48` | Hash accelerator    | `device::hash`                           |
//! | `b'N'`    | `0x4E` | Neural network      | `device::neural_network` / `drivers_neural_network` |
//! | `b'P'`    | `0x50` | System power        | `device::power` / `drivers_peripherals::power` |
//! | `b'T'`    | `0x54` | Terminal / character| `file_system::character_device`          |
//! | `b'L'`    | `0x4C` | Bluetooth LE        | `device::bluetooth` / `bluetooth`       |
//! | `b'a'`    | `0x61` | ADC                 | `device::adc` / `drivers_peripherals::adc`|
//! | `b'g'`    | `0x67` | GPIO                | `device::gpio` / `drivers_peripherals::gpio` |
//! | `b'h'`    | `0x68` | Human interface emulation | `device::human_interface` / `drivers_peripherals::human_interface` |
//! | `b'i'`    | `0x69` | I2C                 | `device::i2c` / `drivers_peripherals::i2c`|
//! | `b'n'`    | `0x6E` | Network interface   | `network`                                |
//! | `b'p'`    | `0x70` | PWM                 | `device::pwm` / `drivers_peripherals::pwm`|
//! | `b'r'`    | `0x72` | Real-time clock     | `device::real_time_clock` / `drivers_peripherals::real_time_clock` |
//! | `b's'`    | `0x73` | SPI                 | `device::spi` / `drivers_peripherals::spi`|
//! | `b'u'`    | `0x75` | UART                | `device::uart` / `drivers_peripherals::uart` |
//! | `b'w'`    | `0x77` | Wi-Fi               | `network`                                |

/// Audio playback and stream control (`b'A'`).
pub const AUDIO: u8 = b'A';

/// Generic block device geometry queries (`b'B'`).
///
/// Defined directly in `file_system::block_device` as a literal to avoid a
/// dependency cycle (`device` depends on `file_system`).
pub const BLOCK_DEVICE: u8 = b'B';

/// Screen / display control and screenshot requests (`b'D'`).
pub const SCREEN: u8 = b'D';

/// Native executable launch entry point query (`b'E'`).
pub const EXECUTABLE: u8 = b'E';

/// Firmware update transaction control (`b'F'`).
pub const FIRMWARE_UPDATE: u8 = b'F';

/// Hardware or software cryptographic hash device (`b'H'`).
pub const HASH: u8 = b'H';

/// Bluetooth Low Energy host.
pub const BLUETOOTH: u8 = b'L';

/// Neural network inference device (`b'N'`).
pub const NEURAL_NETWORK: u8 = b'N';

/// System power actions: power off, restart, sleep (`b'P'`).
pub const POWER: u8 = b'P';

/// Terminal capability query and line discipline attributes on character
/// devices (`b'T'`).
///
/// Defined directly in `file_system::character_device` as a literal to avoid a
/// dependency cycle (`device` depends on `file_system`).
pub const TERMINAL: u8 = b'T';

/// Analog-to-digital converter (`b'a'`).
pub const ADC: u8 = b'a';

/// General-purpose input/output line (`b'g'`).
pub const GPIO: u8 = b'g';

/// Human interface emulation device, acting as a keyboard, media remote and mouse toward
/// the connected computer (`b'h'`).
pub const HUMAN_INTERFACE: u8 = b'h';

/// Inter-Integrated Circuit bus endpoint (`b'i'`).
pub const I2C: u8 = b'i';

/// Network interface configuration and routing (`b'n'`).
pub const NETWORK: u8 = b'n';

/// Pulse-width modulation channel (`b'p'`).
pub const PWM: u8 = b'p';

/// Real-time clock: wall-clock time (UTC) kept across resets and power-off (`b'r'`).
pub const REAL_TIME_CLOCK: u8 = b'r';

/// Serial Peripheral Interface endpoint (`b's'`).
pub const SPI: u8 = b's';

/// Universal Asynchronous Receiver-Transmitter endpoint (`b'u'`).
pub const UART: u8 = b'u';

/// Wi-Fi radio, scan, and client association control (`b'w'`).
pub const WIFI: u8 = b'w';

/// All registered VFS control command kind bytes.
pub const ALL: &[u8] = &[
    AUDIO,
    BLOCK_DEVICE,
    SCREEN,
    EXECUTABLE,
    FIRMWARE_UPDATE,
    HASH,
    BLUETOOTH,
    NEURAL_NETWORK,
    POWER,
    TERMINAL,
    ADC,
    GPIO,
    HUMAN_INTERFACE,
    I2C,
    NETWORK,
    PWM,
    REAL_TIME_CLOCK,
    SPI,
    UART,
    WIFI,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_registered_kinds_are_distinct() {
        for (i, &left) in ALL.iter().enumerate() {
            for &right in &ALL[i + 1..] {
                assert_ne!(
                    left, right,
                    "duplicate control command kind byte: 0x{left:02x} ('{}')",
                    left as char
                );
            }
        }
    }
}
