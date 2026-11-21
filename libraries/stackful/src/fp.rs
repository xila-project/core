//! Floating-point control state. corosensei preserves the callee-saved data registers
//! of the target's ABI, but not the control registers: rounding mode, exception
//! enables and sticky flags. Each side of a switch restores its own, so a rounding
//! mode set by one coroutine never leaks into the executor or another coroutine.
#[cfg(stackful_fp_control)]
use core::arch::asm;

/// Control registers of the targets that have them; unused values are zero.
#[derive(Clone, Copy)]
pub(crate) struct Environment([usize; 2]);

impl Environment {
    /// The state a new coroutine starts with: the ABI's default control word.
    pub(crate) const INITIAL: Self = Self(if cfg!(target_arch = "x86_64") {
        [0x1f80, 0x037f]
    } else {
        [0, 0]
    });
}

#[cfg(not(stackful_fp_control))]
impl Environment {
    pub(crate) fn read() -> Self {
        Self::INITIAL
    }

    pub(crate) fn write(self) {
        let _ = self.0;
    }
}

#[cfg(stackful_fp_control)]
impl Environment {
    /// Read the running context's state.
    #[inline]
    pub(crate) fn read() -> Self {
        let (a, b): (usize, usize);
        // SAFETY: only reads the floating-point control registers.
        unsafe {
            #[cfg(target_arch = "x86_64")]
            {
                let (mut mxcsr, mut x87) = (0u32, 0u16);
                asm!(
                    "stmxcsr [{mxcsr}]",
                    "fnstcw [{x87}]",
                    mxcsr = in(reg) &mut mxcsr,
                    x87 = in(reg) &mut x87,
                    options(nostack, preserves_flags),
                );
                (a, b) = (mxcsr as usize, x87 as usize);
            }
            #[cfg(target_arch = "aarch64")]
            asm!("mrs {a}, fpcr", "mrs {b}, fpsr", a = out(reg) a, b = out(reg) b, options(nostack, preserves_flags));
            #[cfg(target_arch = "arm")]
            {
                asm!("vmrs {a}, fpscr", a = out(reg) a, options(nostack, preserves_flags));
                b = 0;
            }
            #[cfg(target_arch = "riscv32")]
            {
                asm!("frcsr {a}", a = out(reg) a, options(nostack));
                b = 0;
            }
            #[cfg(target_arch = "xtensa")]
            asm!("rur.fcr {a}", "rur.fsr {b}", a = out(reg) a, b = out(reg) b, options(nostack, preserves_flags));
        }
        Self([a, b])
    }

    /// Make this state the running context's.
    #[inline]
    pub(crate) fn write(self) {
        let [a, b] = self.0;
        // SAFETY: restores a state read earlier, or the ABI default.
        unsafe {
            #[cfg(target_arch = "x86_64")]
            asm!(
                "ldmxcsr [{mxcsr}]",
                "fldcw [{x87}]",
                mxcsr = in(reg) &(a as u32),
                x87 = in(reg) &(b as u16),
                options(nostack, preserves_flags, readonly),
            );
            #[cfg(target_arch = "aarch64")]
            asm!("msr fpcr, {a}", "msr fpsr, {b}", a = in(reg) a, b = in(reg) b, options(nostack, preserves_flags));
            #[cfg(target_arch = "arm")]
            asm!("vmsr fpscr, {a}", a = in(reg) a, options(nostack, preserves_flags));
            #[cfg(target_arch = "riscv32")]
            asm!("fscsr {a}", a = in(reg) a, options(nostack));
            #[cfg(target_arch = "xtensa")]
            asm!("wur.fcr {a}", "wur.fsr {b}", a = in(reg) a, b = in(reg) b, options(nostack, preserves_flags));
        }
        #[cfg(any(target_arch = "arm", target_arch = "riscv32"))]
        let _ = b;
    }
}
