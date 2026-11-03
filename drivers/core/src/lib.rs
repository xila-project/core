mod null;

pub use null::*;

#[cfg(feature = "wifi_mock")]
pub mod wifi;
