//! Fixed-layout Wi-Fi control payloads. Byte-backed state values deliberately
//! accept every bit pattern: validate at the driver boundary before use.
use core::fmt;
use file_system::{Error, Result};

macro_rules! byte_value {
    ($name:ident { $($constant:ident = $value:expr),* $(,)? }) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name(pub u8);
        impl $name { $(pub const $constant: Self = Self($value);)* }
    };
}
byte_value!(WifiAuthentication { OPEN = 0, WPA2_PERSONAL = 1, WPA3_PERSONAL = 2, WPA2_WPA3_PERSONAL = 3, UNSUPPORTED = 255 });
byte_value!(WifiRadioState { OFF = 0, ON = 1 });
byte_value!(WifiScanPhase { IDLE = 0, SCANNING = 1, COMPLETED = 2, FAILED = 3 });
byte_value!(WifiConnectionState { DISCONNECTED = 0, CONNECTING = 1, CONNECTED = 2, DISCONNECTING = 3 });
byte_value!(WifiConnectionErrorKind { NONE = 0, AUTHENTICATION = 1, ACCESS_POINT_NOT_FOUND = 2, ASSOCIATION = 3, TIMEOUT = 4, UNSUPPORTED_CONFIGURATION = 5, RADIO = 6, DRIVER = 7 });

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiSsid {
    pub bytes: [u8; 32],
    pub length: u8,
}
impl WifiSsid {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > 32 {
            return Err(Error::InvalidParameter);
        }
        let mut ssid = Self::default();
        ssid.bytes[..bytes.len()].copy_from_slice(bytes);
        ssid.length = bytes.len() as u8;
        Ok(ssid)
    }
    pub fn as_bytes(&self) -> Result<&[u8]> {
        if !(1..=32).contains(&self.length) {
            return Err(Error::InvalidParameter);
        }
        Ok(&self.bytes[..self.length as usize])
    }
}

/// Never printed or returned by status commands. Clear caller-owned copies too.
#[repr(C)]
#[derive(Clone, Eq, PartialEq)]
pub struct WifiPassword {
    pub bytes: [u8; 64],
    pub length: u8,
}
impl Default for WifiPassword {
    fn default() -> Self {
        Self {
            bytes: [0; 64],
            length: 0,
        }
    }
}
impl fmt::Debug for WifiPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl WifiPassword {
    pub fn new(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 64 {
            return Err(Error::InvalidParameter);
        }
        let mut result = Self::default();
        result.bytes[..bytes.len()].copy_from_slice(bytes);
        result.length = bytes.len() as u8;
        Ok(result)
    }
    pub fn as_bytes(&self) -> Result<&[u8]> {
        self.bytes
            .get(..self.length as usize)
            .ok_or(Error::InvalidParameter)
    }
    pub fn clear(&mut self) {
        // Volatile stores prevent dead-store elimination of this owned buffer.
        for byte in &mut self.bytes {
            unsafe {
                core::ptr::write_volatile(byte, 0);
            }
        }
        self.length = 0;
    }
}
impl Drop for WifiPassword {
    fn drop(&mut self) {
        self.clear();
    }
}

#[repr(C)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WifiClientConfiguration {
    pub ssid: WifiSsid,
    pub credential: WifiPassword,
    pub authentication: WifiAuthentication,
    pub bssid: [u8; 6],
    /// Zero means unspecified.
    pub channel: u8,
    /// 0 or 1. Kept as a byte for raw control payload validity.
    pub use_bssid: u8,
}
impl WifiClientConfiguration {
    pub fn new(ssid: &[u8], authentication: WifiAuthentication, secret: &[u8]) -> Result<Self> {
        let result = Self {
            ssid: WifiSsid::new(ssid)?,
            credential: WifiPassword::new(secret)?,
            authentication,
            ..Self::default()
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<()> {
        self.ssid.as_bytes()?;
        if self.use_bssid > 1 || self.channel > 196 {
            return Err(Error::InvalidParameter);
        }
        let secret = self.credential.as_bytes()?;
        let valid = match self.authentication {
            WifiAuthentication::OPEN => secret.is_empty(),
            WifiAuthentication::WPA2_PERSONAL | WifiAuthentication::WPA2_WPA3_PERSONAL => {
                ((8..=63).contains(&secret.len()) && secret.iter().all(|b| (32..=126).contains(b)))
                    || (secret.len() == 64 && secret.iter().all(u8::is_ascii_hexdigit))
            }
            WifiAuthentication::WPA3_PERSONAL => {
                (1..=63).contains(&secret.len())
                    && core::str::from_utf8(secret).is_ok()
                    && !secret.contains(&0)
            }
            _ => return Err(Error::UnsupportedOperation),
        };
        if valid {
            Ok(())
        } else {
            Err(Error::InvalidParameter)
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiAccessPoint {
    pub ssid: WifiSsid,
    /// All zero when unavailable.
    pub bssid: [u8; 6],
    /// Zero when unavailable.
    pub channel: u8,
    /// dBm; i8::MIN means unavailable.
    pub rssi: i8,
    pub authentication: WifiAuthentication,
}
/// Connection metadata never contains credentials.
pub type WifiConnectionInfo = WifiAccessPoint;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiCapabilities {
    pub station: u8,
    pub access_point: u8,
    /// Bit n advertises WifiAuthentication(n); extended methods are not advertised.
    pub authentication_mask: u8,
    pub maximum_scan_results: u8,
}
impl WifiCapabilities {
    pub fn supports(&self, authentication: WifiAuthentication) -> bool {
        authentication.0 < 8 && self.authentication_mask & (1 << authentication.0) != 0
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WifiScanConfiguration {
    /// Zero means all channels.
    pub channel: u8,
    pub passive: u8,
    pub result_limit: u16,
}
impl Default for WifiScanConfiguration {
    fn default() -> Self {
        Self {
            channel: 0,
            passive: 0,
            result_limit: 16,
        }
    }
}
impl WifiScanConfiguration {
    pub fn validate(&self) -> Result<()> {
        if self.channel > 196 || self.passive > 1 || self.result_limit == 0 {
            Err(Error::InvalidParameter)
        } else {
            Ok(())
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiScanState {
    /// Generation of the latest completed scan; unchanged while scanning.
    pub generation: u32,
    pub phase: WifiScanPhase,
    pub reserved: [u8; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiScanResultCount {
    pub generation: u32,
    pub count: u16,
    pub reserved: [u8; 2],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WifiScanResultRequest {
    pub generation: u32,
    pub index: u16,
    pub reserved: [u8; 2],
}
