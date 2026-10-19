mod architecture;
mod family;
mod operating_system;
mod vendor;

use std::env;

pub use architecture::*;
pub use family::*;
pub use operating_system::*;
pub use vendor::*;

#[derive(Debug, PartialEq)]
pub struct Target {
    architecture: Architecture,
    operating_system: OperatingSystem,
    family: Family,
    vendor: Vendor,
}

impl Target {
    pub fn get_architecture(&self) -> Architecture {
        self.architecture
    }

    pub fn get_operating_system(&self) -> OperatingSystem {
        self.operating_system
    }

    pub fn get_family(&self) -> Family {
        self.family
    }

    pub fn get_current() -> Target {
        Target {
            architecture: Architecture::from(env::var("CARGO_CFG_TARGET_ARCH").unwrap()),
            operating_system: OperatingSystem::from(env::var("CARGO_CFG_TARGET_OS").unwrap()),
            family: Family::from(env::var("CARGO_CFG_TARGET_FAMILY").unwrap()),
            vendor: Vendor::from(env::var("CARGO_CFG_TARGET_VENDOR").unwrap()),
        }
    }
}
