use file_system::{ControlCommand, define_command};

use crate::{Area, Point};

define_command!(SET_DRAWING_AREA, Write, b'D', 0x01, Area, ());
define_command!(GET_RESOLUTION, Read, b'D', 0x02, (), Point);
define_command!(WAS_RESIZED, Read, b'D', 0x03, (), bool);
