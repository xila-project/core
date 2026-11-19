#[cfg(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    stackful_float,
    stackful_xtensa_fp
))]
use core::arch::asm;

#[cfg(target_arch = "x86_64")]
pub(super) struct Environment {
    mxcsr: u32,
    x87: u16,
}

#[cfg(target_arch = "x86_64")]
impl Environment {
    pub(super) fn read() -> Self {
        let mut state = Self {
            mxcsr: 0x1f80,
            x87: 0x037f,
        };
        unsafe {
            asm!(
                "stmxcsr [{mxcsr}]",
                "fnstcw [{x87}]",
                mxcsr = in(reg) &mut state.mxcsr,
                x87 = in(reg) &mut state.x87,
                options(nostack, preserves_flags),
            );
        }
        state
    }

    pub(super) fn write(&self) {
        unsafe {
            asm!(
                "ldmxcsr [{mxcsr}]",
                "fldcw [{x87}]",
                mxcsr = in(reg) &self.mxcsr,
                x87 = in(reg) &self.x87,
                options(nostack, preserves_flags, readonly),
            );
        }
    }

    pub(super) fn initialize() {
        Self {
            mxcsr: 0x1f80,
            x87: 0x037f,
        }
        .write();
    }
}

#[cfg(target_arch = "aarch64")]
pub(super) struct Environment {
    fpcr: u64,
    fpsr: u64,
}

#[cfg(target_arch = "aarch64")]
impl Environment {
    pub(super) fn read() -> Self {
        let (fpcr, fpsr);
        unsafe {
            asm!(
                "mrs {fpcr}, fpcr",
                "mrs {fpsr}, fpsr",
                fpcr = out(reg) fpcr,
                fpsr = out(reg) fpsr,
                options(nostack, preserves_flags),
            );
        }
        Self { fpcr, fpsr }
    }

    pub(super) fn write(&self) {
        unsafe {
            asm!(
                "msr fpcr, {fpcr}",
                "msr fpsr, {fpsr}",
                fpcr = in(reg) self.fpcr,
                fpsr = in(reg) self.fpsr,
                options(nostack, preserves_flags),
            );
        }
    }

    pub(super) fn initialize() {
        Self { fpcr: 0, fpsr: 0 }.write();
    }
}

#[cfg(all(any(target_arch = "arm", target_arch = "riscv32"), stackful_float))]
pub(super) struct Environment(u32);

#[cfg(all(any(target_arch = "arm", target_arch = "riscv32"), stackful_float))]
impl Environment {
    pub(super) fn read() -> Self {
        let flags: u32;
        unsafe {
            #[cfg(target_arch = "arm")]
            asm!("vmrs {flags}, fpscr", flags = out(reg) flags, options(nostack, preserves_flags));
            #[cfg(target_arch = "riscv32")]
            asm!("frcsr {flags}", flags = out(reg) flags, options(nostack));
        }
        Self(flags)
    }

    pub(super) fn write(&self) {
        unsafe {
            #[cfg(target_arch = "arm")]
            asm!("vmsr fpscr, {flags}", flags = in(reg) self.0, options(nostack, preserves_flags));
            #[cfg(target_arch = "riscv32")]
            asm!("fscsr {flags}", flags = in(reg) self.0, options(nostack));
        }
    }

    pub(super) fn initialize() {
        Self(0).write();
    }
}

#[cfg(all(any(target_arch = "arm", target_arch = "riscv32"), not(stackful_float)))]
pub(super) struct Environment;

#[cfg(all(any(target_arch = "arm", target_arch = "riscv32"), not(stackful_float)))]
impl Environment {
    pub(super) fn read() -> Self {
        Self
    }

    pub(super) fn write(&self) {}

    pub(super) fn initialize() {}
}

// ESP32 and ESP32-S3 single-precision FPU: rounding mode and enables in FCR,
// sticky flags in FSR. The ESP32-S2 has no FPU.
#[cfg(stackful_xtensa_fp)]
pub(super) struct Environment {
    fcr: u32,
    fsr: u32,
}

#[cfg(stackful_xtensa_fp)]
impl Environment {
    pub(super) fn read() -> Self {
        let (fcr, fsr);
        unsafe {
            asm!(
                "rur.fcr {fcr}",
                "rur.fsr {fsr}",
                fcr = out(reg) fcr,
                fsr = out(reg) fsr,
                options(nostack, preserves_flags),
            );
        }
        Self { fcr, fsr }
    }

    pub(super) fn write(&self) {
        unsafe {
            asm!(
                "wur.fcr {fcr}",
                "wur.fsr {fsr}",
                fcr = in(reg) self.fcr,
                fsr = in(reg) self.fsr,
                options(nostack, preserves_flags),
            );
        }
    }

    pub(super) fn initialize() {
        Self { fcr: 0, fsr: 0 }.write();
    }
}

#[cfg(all(target_arch = "xtensa", not(stackful_xtensa_fp)))]
pub(super) struct Environment;

#[cfg(all(target_arch = "xtensa", not(stackful_xtensa_fp)))]
impl Environment {
    pub(super) fn read() -> Self {
        Self
    }

    pub(super) fn write(&self) {}

    pub(super) fn initialize() {}
}
