use std::env;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum OperatingSystem {
    Windows,
    Linux,
    MacOS,
    EspIdf,
    WASI,
}

impl OperatingSystem {
    pub fn get() -> OperatingSystem {
        OperatingSystem::from(Self::get_raw())
    }

    pub fn get_raw() -> String {
        env::var("CARGO_CFG_TARGET_OS").unwrap()
    }
}

impl From<String> for OperatingSystem {
    fn from(s: String) -> Self {
        match s.as_str() {
            "windows" => OperatingSystem::Windows,
            "linux" => OperatingSystem::Linux,
            "macos" => OperatingSystem::MacOS,
            "espidf" => OperatingSystem::EspIdf,
            "wasi" => OperatingSystem::WASI,
            _ => panic!("Unknown operating system type : {s}"),
        }
    }
}
