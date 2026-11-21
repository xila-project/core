//! Target-specific C compiler flags derived from the Rust target triple.

/// Derives `-march=` and `-mabi=` flags for RISC-V targets from the Rust target
/// triple and `CARGO_CFG_TARGET_ABI`.
///
/// The `cc` crate translates bare-metal Rust triples such as
/// `riscv32imafc-unknown-none-elf` to `riscv32-unknown-none-elf` for Clang and
/// defaults bare-metal GNU RISC-V builds to `-mabi=ilp32`. Passing explicit
/// architecture and ABI flags keeps generated font objects aligned with Rust's
/// floating-point calling convention.
pub fn riscv_compiler_flags(target: &str, target_abi: &str) -> Option<(String, String)> {
    let architecture = target.split('-').next()?;
    let (width, extensions) = if let Some(rest) = architecture.strip_prefix("riscv32") {
        (32u8, rest)
    } else {
        let rest = architecture.strip_prefix("riscv64")?;
        (64u8, rest)
    };

    if extensions.is_empty()
        || !matches!(extensions.as_bytes()[0], b'i' | b'e' | b'g')
        || !extensions
            .bytes()
            .all(|byte| matches!(byte, b'i' | b'e' | b'm' | b'a' | b'f' | b'd' | b'c' | b'g'))
    {
        return None;
    }

    let abi = match target_abi {
        "ilp32" | "ilp32e" | "ilp32f" | "ilp32d" | "lp64" | "lp64f" | "lp64d" => target_abi,
        _ if width == 32 => {
            if extensions.starts_with('e') {
                "ilp32e"
            } else if extensions.contains('g') || extensions.contains('d') {
                "ilp32d"
            } else if extensions.contains('f') {
                "ilp32f"
            } else {
                "ilp32"
            }
        }
        _ => {
            if extensions.contains('g') || extensions.contains('d') {
                "lp64d"
            } else if extensions.contains('f') {
                "lp64f"
            } else {
                "lp64"
            }
        }
    };

    Some((
        format!("-march=rv{width}{extensions}"),
        format!("-mabi={abi}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::riscv_compiler_flags;

    #[test]
    fn derives_hard_float_and_soft_float_riscv_flags_from_target_triple() {
        assert_eq!(
            riscv_compiler_flags("riscv32imafc-unknown-none-elf", ""),
            Some(("-march=rv32imafc".into(), "-mabi=ilp32f".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv32imafc-esp-espidf", ""),
            Some(("-march=rv32imafc".into(), "-mabi=ilp32f".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv32imac-unknown-none-elf", ""),
            Some(("-march=rv32imac".into(), "-mabi=ilp32".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv32imc-unknown-none-elf", ""),
            Some(("-march=rv32imc".into(), "-mabi=ilp32".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv32emc-unknown-none-elf", "ilp32e"),
            Some(("-march=rv32emc".into(), "-mabi=ilp32e".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv32gc-unknown-linux-gnu", ""),
            Some(("-march=rv32gc".into(), "-mabi=ilp32d".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv64imac-unknown-none-elf", ""),
            Some(("-march=rv64imac".into(), "-mabi=lp64".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv64gc-unknown-none-elf", ""),
            Some(("-march=rv64gc".into(), "-mabi=lp64d".into()))
        );
    }

    #[test]
    fn preserves_explicit_target_abi_and_ignores_non_standard_or_non_riscv_targets() {
        assert_eq!(
            riscv_compiler_flags("riscv32imafc-unknown-none-elf", "ilp32f"),
            Some(("-march=rv32imafc".into(), "-mabi=ilp32f".into()))
        );
        assert_eq!(
            riscv_compiler_flags("riscv64a23-unknown-linux-gnu", ""),
            None
        );
        assert_eq!(riscv_compiler_flags("riscv32-wrs-vxworks", ""), None);
        assert_eq!(
            riscv_compiler_flags("thumbv7em-none-eabihf", "eabihf"),
            None
        );
        assert_eq!(riscv_compiler_flags("x86_64-unknown-linux-gnu", ""), None);
    }
}
