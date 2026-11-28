//! RISC-V ELF build attributes, as defined by the RISC-V psABI.
use crate::Error;

pub(crate) fn check(bytes: &[u8], features: &str) -> Result<(), Error> {
    let architecture = architecture(bytes).ok_or(Error::InvalidRiscvAttributes)?;
    let architecture = architecture
        .strip_prefix(b"rv32")
        .ok_or(Error::IncompatibleRiscvArchitecture)?;
    for (index, extension) in architecture.split(|&byte| byte == b'_').enumerate() {
        let name = extension_name(extension).ok_or(Error::InvalidRiscvAttributes)?;
        if (index == 0) != matches!(name, b"i" | b"e") {
            return Err(Error::InvalidRiscvAttributes);
        }
        if !features
            .split(',')
            .any(|feature| feature.as_bytes() == name)
        {
            return Err(Error::IncompatibleRiscvArchitecture);
        }
    }
    Ok(())
}

/// Strip the required major and minor version. Digits inside names such as
/// `zve32x` are part of the name. Compare extension names independently of version.
fn extension_name(extension: &[u8]) -> Option<&[u8]> {
    let version_separator = extension.iter().rposition(|&byte| byte == b'p')?;
    let (major_and_name, minor) = extension.split_at(version_separator);
    let name_end = major_and_name
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())?
        + 1;
    let name = &major_and_name[..name_end];
    if name_end == major_and_name.len()
        || minor.len() == 1
        || !minor[1..].iter().all(u8::is_ascii_digit)
        || !name[0].is_ascii_lowercase()
        || !name
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return None;
    }
    Some(name)
}

fn architecture(bytes: &[u8]) -> Option<&[u8]> {
    let mut vendors = bytes.strip_prefix(b"A")?;
    let mut architecture = None;
    while !vendors.is_empty() {
        let mut vendor = block(&mut vendors, 0)?;
        if string(&mut vendor)? != b"riscv" {
            continue;
        }
        while !vendor.is_empty() {
            let previous = vendor.len();
            let kind = integer(&mut vendor)?;
            let tag_size = previous - vendor.len();
            let mut attributes = block(&mut vendor, tag_size)?;
            if kind != 1 {
                continue;
            }
            while !attributes.is_empty() {
                let tag = integer(&mut attributes)?;
                if tag % 2 == 0 {
                    integer(&mut attributes)?;
                } else {
                    let value = string(&mut attributes)?;
                    if tag == 5 && architecture.replace(value).is_some() {
                        return None;
                    }
                }
            }
        }
    }
    architecture
}

/// A length includes its own four bytes and, for file attributes, the tag.
fn block<'a>(bytes: &mut &'a [u8], tag_size: usize) -> Option<&'a [u8]> {
    let (length, _) = bytes.split_at_checked(4)?;
    let length = u32::from_le_bytes(length.try_into().ok()?) as usize;
    let (block, rest) = bytes.split_at_checked(length.checked_sub(tag_size)?)?;
    *bytes = rest;
    block.get(4..)
}

fn string<'a>(bytes: &mut &'a [u8]) -> Option<&'a [u8]> {
    let end = bytes.iter().position(|&byte| byte == 0)?;
    let (string, rest) = bytes.split_at(end);
    *bytes = &rest[1..];
    Some(string)
}

fn integer(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}
