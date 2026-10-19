use std::env;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Family {
    Unix,
    Windows,
    Wasm,
}

impl Family {
    pub fn get() -> Family {
        Family::from(Self::get_raw())
    }

    pub fn get_raw() -> String {
        env::var("CARGO_CFG_TARGET_FAMILY").unwrap()
    }
}

impl From<String> for Family {
    fn from(s: String) -> Self {
        match s.as_str() {
            "unix" => Family::Unix,
            "windows" => Family::Windows,
            "wasm" => Family::Wasm,
            _ => panic!("Unknown family type : {s}"),
        }
    }
}
