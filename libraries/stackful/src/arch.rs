//! ABI callee-saved context only; each switch is an ordinary external call.
//! SP must be the thread-mode SP. Interrupt/exception state is platform-owned.
use core::arch::naked_asm;

#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "sub rsp, 8",
        "stmxcsr [rsp]",
        "fnstcw [rsp + 4]",
        "mov [rdi], rsp",
        "mov rsp, [rsi]",
        "ldmxcsr [rsp]",
        "fldcw [rsp + 4]",
        "add rsp, 8",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );
}
#[cfg(target_arch = "x86_64")]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 72;
    unsafe {
        (sp as *mut u8).write_bytes(0, 72);
        ((sp + 56) as *mut usize).write(entry);
        (sp as *mut u32).write(0x1f80);
        ((sp + 4) as *mut u16).write(0x37f);
    }
    sp
}

#[cfg(target_arch = "aarch64")]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "sub sp, sp, #176",
        "stp x19, x20, [sp, #0]",
        "stp x21, x22, [sp, #16]",
        "stp x23, x24, [sp, #32]",
        "stp x25, x26, [sp, #48]",
        "stp x27, x28, [sp, #64]",
        "stp x29, x30, [sp, #80]",
        "stp d8, d9, [sp, #96]",
        "stp d10, d11, [sp, #112]",
        "stp d12, d13, [sp, #128]",
        "stp d14, d15, [sp, #144]",
        "mrs x2, fpcr",
        "mrs x3, fpsr",
        "stp x2, x3, [sp, #160]",
        "mov x2, sp",
        "str x2, [x0]",
        "ldr x2, [x1]",
        "mov sp, x2",
        "ldp x2, x3, [sp, #160]",
        "msr fpcr, x2",
        "msr fpsr, x3",
        "ldp d8, d9, [sp, #96]",
        "ldp d10, d11, [sp, #112]",
        "ldp d12, d13, [sp, #128]",
        "ldp d14, d15, [sp, #144]",
        "ldp x19, x20, [sp, #0]",
        "ldp x21, x22, [sp, #16]",
        "ldp x23, x24, [sp, #32]",
        "ldp x25, x26, [sp, #48]",
        "ldp x27, x28, [sp, #64]",
        "ldp x29, x30, [sp, #80]",
        "add sp, sp, #176",
        "ret",
    );
}
#[cfg(target_arch = "aarch64")]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 176;
    unsafe {
        (sp as *mut u8).write_bytes(0, 176);
        ((sp + 88) as *mut usize).write(entry);
    }
    sp
}

#[cfg(all(target_arch = "arm", not(stackful_float)))]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "push {{r4-r7, lr}}",
        "mov r4, r8",
        "mov r5, r9",
        "mov r6, r10",
        "mov r7, r11",
        "push {{r4-r7}}",
        "sub sp, #4",
        "mov r2, sp",
        "str r2, [r0]",
        "ldr r2, [r1]",
        "mov sp, r2",
        "add sp, #4",
        "pop {{r4-r7}}",
        "mov r8, r4",
        "mov r9, r5",
        "mov r10, r6",
        "mov r11, r7",
        "pop {{r4-r7, pc}}",
    );
}
#[cfg(all(target_arch = "arm", not(stackful_float)))]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 40;
    unsafe {
        (sp as *mut u8).write_bytes(0, 40);
        ((sp + 36) as *mut usize).write(entry);
    }
    sp
}

#[cfg(all(target_arch = "arm", stackful_float))]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "push {{r4-r7, lr}}",
        "mov r4, r8",
        "mov r5, r9",
        "mov r6, r10",
        "mov r7, r11",
        "push {{r4-r7}}",
        "sub sp, #4",
        "vpush {{d8-d15}}",
        "vmrs r2, fpscr",
        "push {{r2, r3}}",
        "mov r2, sp",
        "str r2, [r0]",
        "ldr r2, [r1]",
        "mov sp, r2",
        "pop {{r2, r3}}",
        "vmsr fpscr, r2",
        "vpop {{d8-d15}}",
        "add sp, #4",
        "pop {{r4-r7}}",
        "mov r8, r4",
        "mov r9, r5",
        "mov r10, r6",
        "mov r11, r7",
        "pop {{r4-r7, pc}}",
    );
}
#[cfg(all(target_arch = "arm", stackful_float))]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 112;
    unsafe {
        (sp as *mut u8).write_bytes(0, 112);
        ((sp + 108) as *mut usize).write(entry);
    }
    sp
}

#[cfg(all(target_arch = "riscv32", not(stackful_float)))]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "addi sp, sp, -64",
        "sw ra, 0(sp)",
        "sw s0, 4(sp)",
        "sw s1, 8(sp)",
        "sw s2, 12(sp)",
        "sw s3, 16(sp)",
        "sw s4, 20(sp)",
        "sw s5, 24(sp)",
        "sw s6, 28(sp)",
        "sw s7, 32(sp)",
        "sw s8, 36(sp)",
        "sw s9, 40(sp)",
        "sw s10, 44(sp)",
        "sw s11, 48(sp)",
        "sw sp, 0(a0)",
        "lw sp, 0(a1)",
        "lw ra, 0(sp)",
        "lw s0, 4(sp)",
        "lw s1, 8(sp)",
        "lw s2, 12(sp)",
        "lw s3, 16(sp)",
        "lw s4, 20(sp)",
        "lw s5, 24(sp)",
        "lw s6, 28(sp)",
        "lw s7, 32(sp)",
        "lw s8, 36(sp)",
        "lw s9, 40(sp)",
        "lw s10, 44(sp)",
        "lw s11, 48(sp)",
        "addi sp, sp, 64",
        "ret",
    );
}
#[cfg(all(target_arch = "riscv32", not(stackful_float)))]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 64;
    unsafe {
        (sp as *mut u8).write_bytes(0, 64);
        ((sp + 0) as *mut usize).write(entry);
    }
    sp
}

#[cfg(all(target_arch = "riscv32", stackful_float))]
#[unsafe(naked)]
pub unsafe extern "C" fn switch(_from: *mut usize, _to: *const usize) {
    naked_asm!(
        "addi sp, sp, -128",
        "sw ra, 0(sp)",
        "sw s0, 4(sp)",
        "sw s1, 8(sp)",
        "sw s2, 12(sp)",
        "sw s3, 16(sp)",
        "sw s4, 20(sp)",
        "sw s5, 24(sp)",
        "sw s6, 28(sp)",
        "sw s7, 32(sp)",
        "sw s8, 36(sp)",
        "sw s9, 40(sp)",
        "sw s10, 44(sp)",
        "sw s11, 48(sp)",
        "fsw fs0, 64(sp)",
        "fsw fs1, 68(sp)",
        "fsw fs2, 72(sp)",
        "fsw fs3, 76(sp)",
        "fsw fs4, 80(sp)",
        "fsw fs5, 84(sp)",
        "fsw fs6, 88(sp)",
        "fsw fs7, 92(sp)",
        "fsw fs8, 96(sp)",
        "fsw fs9, 100(sp)",
        "fsw fs10, 104(sp)",
        "fsw fs11, 108(sp)",
        "frcsr t0",
        "sw t0, 112(sp)",
        "sw sp, 0(a0)",
        "lw sp, 0(a1)",
        "flw fs0, 64(sp)",
        "flw fs1, 68(sp)",
        "flw fs2, 72(sp)",
        "flw fs3, 76(sp)",
        "flw fs4, 80(sp)",
        "flw fs5, 84(sp)",
        "flw fs6, 88(sp)",
        "flw fs7, 92(sp)",
        "flw fs8, 96(sp)",
        "flw fs9, 100(sp)",
        "flw fs10, 104(sp)",
        "flw fs11, 108(sp)",
        "lw t0, 112(sp)",
        "fscsr t0",
        "lw ra, 0(sp)",
        "lw s0, 4(sp)",
        "lw s1, 8(sp)",
        "lw s2, 12(sp)",
        "lw s3, 16(sp)",
        "lw s4, 20(sp)",
        "lw s5, 24(sp)",
        "lw s6, 28(sp)",
        "lw s7, 32(sp)",
        "lw s8, 36(sp)",
        "lw s9, 40(sp)",
        "lw s10, 44(sp)",
        "lw s11, 48(sp)",
        "addi sp, sp, 128",
        "ret",
    );
}
#[cfg(all(target_arch = "riscv32", stackful_float))]
pub unsafe fn initialize(top: usize, entry: usize) -> usize {
    let sp = top - 128;
    unsafe {
        (sp as *mut u8).write_bytes(0, 128);
        ((sp + 0) as *mut usize).write(entry);
    }
    sp
}
