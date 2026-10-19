//! The `b'A'` device protocol. Configuration is fixed for an open stream.
//! `GET_WRITABLE` returns free bytes. Volume is 0..=100 percent, applied
//! when rendering. STOP pauses on the next callback and retains queued PCM.

use file_system::{ControlCommand, define_command};

use crate::Configuration;

define_command!(SET_CONFIGURATION, Write, b'A', 0x01, Configuration, ());
define_command!(GET_CONFIGURATION, Read, b'A', 0x02, (), Configuration);
define_command!(GET_WRITABLE, Read, b'A', 0x03, (), u32);
define_command!(SET_VOLUME, Write, b'A', 0x04, u8, ());
define_command!(GET_VOLUME, Read, b'A', 0x05, (), u8);
define_command!(START, Write, b'A', 0x06, (), ());
define_command!(STOP, Write, b'A', 0x07, (), ());
