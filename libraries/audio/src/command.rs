//! The `b'A'` device protocol. Configuration is fixed for an open stream.
//! `GET_WRITABLE` returns free bytes. Volume is 0..=100 percent, applied
//! when rendering. STOP pauses on the next callback and retains queued PCM.

use file_system::{ControlCommand, define_command};

use crate::Configuration;

define_command!(SET_CONFIGURATION, Write, device::kind::AUDIO, 0x01, Configuration, ());
define_command!(GET_CONFIGURATION, Read, device::kind::AUDIO, 0x02, (), Configuration);
define_command!(GET_WRITABLE, Read, device::kind::AUDIO, 0x03, (), u32);
define_command!(SET_VOLUME, Write, device::kind::AUDIO, 0x04, u8, ());
define_command!(GET_VOLUME, Read, device::kind::AUDIO, 0x05, (), u8);
define_command!(START, Write, device::kind::AUDIO, 0x06, (), ());
define_command!(STOP, Write, device::kind::AUDIO, 0x07, (), ());
