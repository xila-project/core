pub const fn get_major_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_MAJOR"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse major version"),
    }
}

pub const fn get_minor_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_MINOR"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse minor version"),
    }
}

pub const fn get_patch_version() -> u8 {
    match u8::from_str_radix(env!("CARGO_PKG_VERSION_PATCH"), 10) {
        Ok(version) => version,
        Err(_) => panic!("Failed to parse patch version"),
    }
}

pub const fn get_version_string() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub const fn get_authors() -> &'static str {
    env!("CARGO_PKG_AUTHORS")
}

pub const fn get_license() -> &'static str {
    env!("CARGO_PKG_LICENSE")
}

pub const fn get_description() -> &'static str {
    env!("CARGO_PKG_DESCRIPTION")
}
