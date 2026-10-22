use std::env;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Vendor {
    Espressif,
    Unknown,
}

impl Vendor {
    pub fn get() -> Vendor {
        Vendor::from(Self::get_raw())
    }

    pub fn get_raw() -> String {
        env::var("CARGO_CFG_TARGET_VENDOR").unwrap()
    }
}

impl From<String> for Vendor {
    fn from(s: String) -> Self {
        match s.as_str() {
            "espressif" => Vendor::Espressif,
            _ => Vendor::Unknown,
        }
    }
}
