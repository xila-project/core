//! PWM device node control contract (`/devices/pwm/<name>`).
//!
//! # Control commands (kind `b'p'`)
//!
//! | Number | Command            | Direction | Input              | Output        | Effect                                        |
//! |--------|--------------------|-----------|--------------------|---------------|-----------------------------------------------|
//! | `0x00` | `GET_INFO`         | Read      | `PwmVersionAbi`    | `PwmInfoAbi`  | Returns channel count, max duty and period.   |
//! | `0x01` | `SET_FREQUENCY`    | Write     | `PwmFrequencyAbi`  | `PwmInfoAbi`  | Sets the channel period in hertz.             |
//! | `0x02` | `SET_DUTY_PERMILLE`| Write     | `PwmDutyPermilleAbi`| `PwmInfoAbi` | Sets duty as 0..=1000 per mille of max duty.  |
//!
//! Every struct starts with a `version` field equal to [`ABI_VERSION`] and
//! carries zeroed `reserved` fields. A wrong version, a non-zero reserved field
//! or an out-of-range value is `InvalidParameter`. A frequency the controller
//! cannot express, or a fixed-period controller, is `UnsupportedOperation`.

use file_system::{ControlCommand, define_command};

/// Version of every PWM control ABI struct in this module.
pub const ABI_VERSION: u16 = 1;

/// Largest value accepted by [`SET_DUTY_PERMILLE`].
pub const PERMILLE_FULL_SCALE: u16 = 1_000;

/// Carries only the ABI version and a reserved field. Input of [`GET_INFO`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmVersionAbi {
    pub version: u16,
    pub reserved: u16,
}

/// Channel description returned by every PWM command.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmInfoAbi {
    pub version: u16,
    /// Number of channels on the controller.
    pub channel_count: u16,
    /// Duty value that means 100 percent on this channel.
    pub max_duty_cycle: u16,
    pub reserved: u16,
    /// Current period of this channel in hertz.
    pub frequency_hz: u32,
}

/// Input of [`SET_FREQUENCY`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmFrequencyAbi {
    pub version: u16,
    pub reserved: u16,
    pub frequency_hz: u32,
}

/// Input of [`SET_DUTY_PERMILLE`]. `permille` is 0..=[`PERMILLE_FULL_SCALE`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PwmDutyPermilleAbi {
    pub version: u16,
    pub permille: u16,
}

define_command!(
    GET_INFO,
    Read,
    crate::kind::PWM,
    0x00,
    PwmVersionAbi,
    PwmInfoAbi
);
define_command!(
    SET_FREQUENCY,
    Write,
    crate::kind::PWM,
    0x01,
    PwmFrequencyAbi,
    PwmInfoAbi
);
define_command!(
    SET_DUTY_PERMILLE,
    Write,
    crate::kind::PWM,
    0x02,
    PwmDutyPermilleAbi,
    PwmInfoAbi
);
