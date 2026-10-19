use std::env;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Architecture {
    X86,
    X86_64,
    Arm,
    Aarch64,
    Xtensa,
    RiscV,
    WASM32,
    WASM64,
}

impl Architecture {
    pub fn get() -> Architecture {
        Architecture::from(Self::get_raw())
    }

    pub fn get_raw() -> String {
        env::var("CARGO_CFG_TARGET_ARCH").unwrap()
    }
}

impl From<String> for Architecture {
    fn from(s: String) -> Self {
        match s.as_str() {
            "x86" => Architecture::X86,
            "x86_64" => Architecture::X86_64,
            "arm" => Architecture::Arm,
            "aarch64" => Architecture::Aarch64,
            "xtensa" => Architecture::Xtensa,
            "riscv" => Architecture::RiscV,
            "wasm32" => Architecture::WASM32,
            "wasm64" => Architecture::WASM64,
            _ => panic!("Unknown architecture type : {s}"),
        }
    }
}
