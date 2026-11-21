# Stackful native runtime

`xila-stackful` (`stackful`) runs synchronous native code as executor-affine
coroutines. It is `no_std` + `alloc`; Linux alone uses std thread-local storage.
Blocking calls in `xila-abi-application` and `xila-virtual-file-system` route
through `suspend_on` on supported targets, suspending the coroutine instead of
busy-polling the executor.

```rust,no_run
use stackful::{Coroutine, Stack, suspend_on};
# async fn example() {
// SAFETY: audit the native call graph's stack bound and forbid unwinding.
let coroutine = unsafe {
    Coroutine::new(Stack::new(16 * 1024), || {
        suspend_on(async { 42 })
    })
};
let join = coroutine.join_handle();
let result = stackful::drive(coroutine).await;
assert_eq!(result, 42);
assert_eq!(join.await, Ok(42));
# }
```

`Coroutine` implements `Future<Output = i32>`; `resume(context)` is its polling
operation. `suspend_on` pins a wrapper future on the native stack, erases its
lifetime while suspended, and returns its output after the driver polls it to
completion. Each Pending propagates immediately to the executor; wakeups use
its real waker. There is no polling loop on Pending. Calls through arbitrarily
nested synchronous functions work. Future poll methods must remain asynchronous:
do not call `suspend_on` inside a future's poll method. Outside native execution,
`suspend_on` forwards to `task::block_on` (the existing busy-polling fallback).

Each coroutine control block also carries `TLS_SLOTS` (4) inline `usize`
coroutine-local storage slots, zero-initialized on creation with no extra heap
allocation. `tls_get(slot)` and `tls_set(slot, value)` isolate per-coroutine
state (for example future libc `errno`, `strtok`, or `rand` pointers) without
coupling it to an OS thread. Calling either function outside a coroutine returns
`Err(TlsError::NotInCoroutine)`; out-of-range indices return
`Err(TlsError::InvalidSlot)`.

`exit(status)` ends the running coroutine at once, as if its entry had returned
`status`: the driver and the join observer see the status, and the stack and
control block are released as for a normal return. It abandons the frames between
the call and the entry without running destructors, like a `longjmp` that never
lands, so it is unsafe: any owned resource live in those frames (an allocation, an open
file, a lock guard), including the entry closure's captures, is leaked. Release
such resources first, or keep them outside the coroutine stack. The native ABI's
`xila_process_exit` does the first. Calling it outside a coroutine panics.

A single `JoinHandle` observes completion and implements a future returning
`Result<i32, Cancelled>`; it can be awaited or passed to `suspend_on`. A second
live observer is rejected because the control block stores one waker.

`spawn(manager, parent, name, stack, entry)` uses the calling task's registered
spawner explicitly; it must never choose the manager's load-balanced default.
It returns Core's task join handle and identifier. All handles and the driver
are !Send/!Sync. Linux permits separate executor threads, each with its own current
pointer. Bare metal currently permits **one executor core only**, with current
pointer access protected by short critical sections; interrupts remain enabled
while native code runs. Never invoke coroutine APIs from an ISR. Dual-core MCU
execution needs a per-core current-pointer hook before it is supported.
The platform supplies the allocator, critical-section implementation, and time
and executor drivers as usual.

## Safety and lifetime

Construction is unsafe: native stack usage must fit the supplied stack, including
closure storage, callees and any interrupt frames sharing its SP. `Stack::new`
rounds requests up to 16 bytes with a default minimum of `DEFAULT_MIN_STACK_SIZE`
(4096) usable bytes. For shallow call chains on RAM-constrained devices,
`Stack::with_min_size(size, min_usable)` allows configuring the minimum usable
size down to `MIN_STACK_SIZE` (2048 bytes). Smaller stacks reduce per-coroutine
RAM overhead at the cost of much tighter headroom for nested callees, register
save frames, and bare-metal interrupt frames sharing the stack; callers must
audit the full call graph before lowering the minimum below 4 KiB. Regardless of
the configured minimum, a separate 64-byte painted guard lies
below the usable area. Guard corruption is checked before and after every switch
and terminates via a non-unwinding panic boundary. A canary cannot prevent an
out-of-bounds write or detect an overflow that skips its guard; this is
detection, not memory protection. Do not treat it as a safe sandbox for
untrusted C. A protected guard page/MPU region is a future platform extension.

The heap control and stack never move while executing, even if the small driver
handle moves between polls on its original thread. Only raw pointers to
interior-mutable fields cross the switch. The suspended future remains alive and
pinned until it completes. It is never polled concurrently with native execution.
No references to an exclusively borrowed control block cross the switch.

Native code must not unwind across a switch. The C-ABI entry boundary aborts if
an entry panic escapes on Linux; embedded applications should use panic=abort.
Panics from Future::poll happen on the driver stack, whose normal Drop cancels any
suspended native stack, as described next. Native destructors that panic also abort
at the boundary. An unstarted entry is dropped normally. A finished entry frees its
stack normally. Native code must never longjmp across these boundaries.

### Cancellation

Dropping a started, unfinished driver cancels the coroutine. It wakes the join
observer with `Cancelled`, drops the pending future in place if that future was passed
to `suspend_on_cancellable` (the destructor runs on the executor, while the stack it is
pinned on is intact), and then discards the suspended frames with corosensei's
`force_reset`: nothing is unwound and no destructor runs on them. The stack is freed
with the control block, as soon as the single join observer, if any, is gone. Before
this release the stack was leaked instead, because a destructor running on a suspended
stack might hold a pointer into it. A freed stack is sound only under the contract of
the unsafe `Coroutine::new`:

* **No owner lives only in a frame across a suspension.** An open file, an allocation
  the host must release, a lock guard: a frame that holds one leaks it, since its
  destructor never runs. Keep such an object where the host owns it and can release it
  without running the program's code, or in a future passed to `suspend_on_cancellable`.
  The native ABI keeps everything a program opens in a per-process registry that its
  host state owns and releases (see `xila-abi-application`), and its few suspended
  futures that own a resource are cancellable.
* **Nothing outside the stack keeps a pointer into it** once the call that is suspended
  has returned or been cancelled. The ABI's one structure that does, the wait queue of a
  mutex or condition variable, links nodes that live in the suspended calls, but its
  heads live in the process's own memory, which goes away with the process. A
  registration made by address and never dereferenced, such as the I/O readiness hub's
  key, is harmless: its entry holds a waker, which is woken once and discarded.

The destructors that do not run are the price: an owner that was left in a frame leaks,
as it did before, but the stack no longer leaks with it. A cancellation that lands while a
close waits for a lock loses that one close.

Prefer driving to completion. A process that exits with suspended worker threads
cancels them when its driver ends, so their stacks are freed too.

## Switch implementation and targets

The context switch is [corosensei](https://github.com/Amanieu/corosensei) 0.3.4
(MIT OR Apache-2.0) with all default features disabled, so it is `no_std` and
needs no allocator or unwinding. Core depends on the `xila` branch of the
`AlixANNERAUD/corosensei` fork, pinned by revision. The branch adds the Xtensa
backend ([Amanieu/corosensei#79](https://github.com/Amanieu/corosensei/issues/79))
and the Cortex-M `d8`-`d15` clobber fix
([#80](https://github.com/Amanieu/corosensei/pull/80): upstream's `q4`-`q7` have no
effect on targets without NEON). Xila owns no assembly for the switch, only
`src/fp.rs`. Core does not publish to crates.io, so the git dependency blocks
nothing today.

| Architecture | Targets | Switched by corosensei | Xila adds |
| --- | --- | --- | --- |
| x86_64 | Linux | callee-saved integer registers, SP | MXCSR, x87 control word |
| AArch64 | Linux, bare metal | x19-x30, d8-d15, SP | FPCR, FPSR |
| Arm | Cortex-M `thumbv6m`, `thumbv7em`, `thumbv8m.main` | r4-r11, d8-d15 on VFP targets, SP | FPSCR on hard-float targets |
| RV32 | `riscv32imc`, `riscv32imac`, `riscv32imafc` | s0-s11, fs0-fs11 with F, SP | FCSR with F |
| Xtensa | ESP32, ESP32-S2, ESP32-S3 (`xtensa-*-none-elf`) | windowed ABI: all live register windows are spilled to their stacks, then SP is swapped | FCR, FSR on ESP32 and ESP32-S3 |

* The "Xila adds" column is `src/fp.rs`. corosensei saves the data registers of
  the ABI but not the floating-point control registers, so without it a rounding
  mode set by a coroutine would leak into the executor and the other coroutines.
  New coroutines start with the ABI's default control state.
* Cortex-M switches are thread-mode calls, never exception returns; platform
  exception handlers own exception frames and interrupt state. Soft-float Arm and
  RV32 targets save no FP state. Native code built for another float ABI than the
  Rust target (for example softfp C on `thumbv7em-none-eabi`) is only covered when
  the build enables the FPU registers with `-C target-feature=+vfp4` (or the
  matching feature): the compiler then treats `d8`-`d15` as clobbered across the
  switch. Rust 1.97 accepts `vfp4` with an "unstable feature" warning. The former
  `fpu` Cargo feature is gone.
* Xtensa needs the esp-rs toolchain (`espup`, channel `+esp`) and
  `-Zbuild-std=core,alloc`; all Xtensa targets are `panic = "abort"`. Every switch
  spills the live register windows (a `call12` chain and one window underflow),
  so it costs more as the call depth grows. The ESP32-S2 has no atomic
  compare-and-swap: build `xila-task` with `--cfg portable_atomic_unsafe_assume_single_core`,
  as for the other chips without CAS (see the Core readme). The platform must
  enable the FPU coprocessor (`CPENABLE` bit 0) before the first interrupt on ESP32
  and ESP32-S3, and every coroutine stack must also hold the interrupt frames of
  an interrupt taken while it runs: `xtensa-lx-rt` pushes 256 bytes per interrupt
  level on the interrupted stack. Never resume a coroutine from an interrupt or
  exception handler. (`xtensa-lx-rt` also faults on its FPU save if `CPENABLE` is
  clear, as in Espressif's QEMU; that concerns any esp-hal application.)
* The interrupt mask belongs to the CPU, not to the coroutine: the switch does not
  save `PS.INTLEVEL` (Xtensa), `PRIMASK`/`BASEPRI` (Cortex-M) or `mstatus.MIE` (RISC-V).
  Suspending while holding a critical section would run the executor with
  interrupts masked, and the coroutine's own unmasking later would be lost. Never
  call `suspend_on` inside `critical_section::with`.
* The entry closure is copied to the top of the stack by corosensei, which caps
  its captures at 1024 bytes. Keep larger state behind a pointer or `Box`.
* Wasm: deliberate compile-time error; no native stack switching.

### Guard contract

corosensei's `Stack` trait asks for a guard page and at least 4 KiB of usable
memory (`MIN_STACK_SIZE`), but corosensei itself never checks either with a custom
stack; only its built-in stacks use them. Xila's stacks are heap blocks without a
guard page, on every target (microcontrollers have no MMU to provide one), and may
be as small as 2 KiB. The stack handed to corosensei is therefore a private wrapper
(`stack::Backing`) and the guarantee comes from the unsafe `Coroutine::new` instead:
the caller bounds all native stack use (the entry, its callees and interrupt
frames), and the 64-byte painted guard detects damage when the coroutine suspends,
resumes or finishes. This is detection, not protection.

Host tests cover real Embassy executor interleaving, signal wakeups and poll
counts, nested synchronous calls, async/native joins, results, thread isolation,
coroutine-local TLS slot isolation, 2 KiB minimum stacks, canary corruption
detection, fallback and cancellation. The allocation-budget test asserts
exactly two runtime allocations and zero allocations over 100 suspend/resume
cycles (and zero allocations for TLS reads/writes). Generic waker
implementations may allocate themselves. The task-manager adapter adds the
manager's existing pool, metadata and join allocations (including its current
leaked task-pool behavior).

On x86_64: `Stack` is 24 bytes; `Coroutine` and `JoinHandle` are each 8 bytes;
the shared control allocation is 184 bytes including `Rc` counts. On 32-bit
targets (`thumbv7em-none-eabihf`, `riscv32imac-unknown-none-elf`) `Control` is 88
bytes and the allocation 96 bytes, the same as with the former hand-written
switch. Each stack adds 64 guard bytes plus up to 15 rounding bytes; corosensei's
initial frame and the entry closure occupy usable stack space. On Xtensa a
coroutine that yields once paints 320 bytes of its stack (windowed frames reserve
a register save area each). `footprint_and_switch_timing` measures two switches
plus runtime work per suspend/resume round trip; run it in release mode. On
x86_64 that is about 85 ns, against about 125 ns for the former hand-written
switch: corosensei keeps `call` and `ret` paired, so the return-stack predictor
stays correct. Under Espressif's QEMU the Xtensa round trip is 534 (ESP32) and 568
(ESP32-S3) `CCOUNT` ticks, which are not cycle counts.

## ABI integration status and remaining decisions

Synchronous VFS calls that bridge async operations and the native application
ABI use `suspend_on` on supported targets. On Xtensa, `xila-stackful` builds
and runs, but the application ABI, the native loader and the VFS do not enable
it yet, so ESP32 targets keep the busy-polling fallback. The synchronous VFS polling
adapters still return resource-busy on pending device I/O. The application ABI
waits cooperatively for blocking file I/O when called from a coroutine.

Remaining work for later milestones: native thread creation must construct and
spawn a driver on the owning executor, set a bounded stack size and resource
policy, and map integer results/join state to the C ABI. Decide detach/cancel
semantics before exposing thread termination; forced cancellation discards the frames
(see Cancellation). Interrupt/signal delivery, foreign callbacks and libc TLS slot
assignments need an explicit policy. Mutex/condition-variable adapters need
real async wait queues, atomic unlock plus wait registration, wake-one/wake-all
semantics and cancellation handling; recursive locks and ownership must
identify coroutines. Per-core current state remains a prerequisite for multi-core (core#682).
