# Stackful native runtime

`xila-stackful` (`stackful`) runs synchronous native code as executor-affine
coroutines. It is `no_std` + `alloc`; Linux alone uses std thread-local storage.
No application ABI callers are changed.

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
On targets without pointer atomics it calls the identical Embassy implementation
directly, because Core's current task manager cannot compile there.

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
and executor drivers as usual. The manager adapter is absent without pointer
atomics; the coroutine/driver/join primitives remain available on thumbv6m.

## Safety and lifetime

Construction is unsafe: native stack usage must fit the supplied stack, including
closure storage, callees and any interrupt frames sharing its SP. Requests are
rounded up to 16 bytes with a minimum of 4096 usable bytes; a separate 64-byte
painted guard lies below the usable area. Guard corruption is checked before and
after every switch and terminates via a non-unwinding panic boundary. A canary
cannot prevent an out-of-bounds write or detect an overflow that skips its guard;
this is detection, not memory protection. Do not treat it as a safe sandbox for
untrusted C. A protected guard page/MPU region is a future platform extension.

The heap control and stack never move while executing, even if the small driver
handle moves between polls on its original thread. Only raw pointers to
interior-mutable fields cross the switch. The suspended future remains alive and
pinned until it completes. It is never polled concurrently with native execution.
No references to an exclusively borrowed control block cross the switch.

Native code must not unwind across a switch. The C-ABI entry boundary aborts if
an entry panic escapes on Linux; embedded applications should use panic=abort.
Panics from Future::poll happen on the driver stack, whose normal Drop leaks any
suspended native stack. Native destructors that panic also abort at the boundary.
An unstarted entry is dropped normally. A finished entry frees its stack normally.
Dropping a started, unfinished driver deliberately leaks its stack and control
block, including live future/resources, and wakes its observer with Cancelled.
This avoids freeing live stack objects; there is no forced unwinding or safe
cancellation of arbitrary C code. Prefer driving to completion. Process exit can
reclaim leaked host memory. Native code must never longjmp across these boundaries.

## Switch implementation and targets

[corosensei 0.3.4](https://docs.rs/corosensei/0.3.4/corosensei/) was evaluated.
It supports no_std with all default features disabled and is MIT OR Apache-2.0;
its default allocator and unwind support require std. It supports ELF x86_64,
AArch64, ARM and RISC-V, but not Xtensa/Wasm. Its unsafe Stack trait explicitly
requires a protected guard page, which a painted embedded heap stack cannot
satisfy. We therefore use a small independent naked-assembly switch and do not
add that dependency or its allocator/unwinding machinery. No comparable linked
code-size measurement was made for corosensei because its stack contract excludes
this configuration.

* x86_64 Linux SysV: six saved integer registers, SP/return PC, MXCSR and x87
  control word. No red-zone data is live across the ordinary switch call.
* AArch64 Linux/bare metal: x19-x30, SP, d8-d15, FPCR/FPSR.
* Cortex-M thumbv6m, thumbv7em, thumbv8m.main: r4-r11, SP/LR. Thumb-1 compatible
  integer sequence; hard-float targets additionally save d8-d15 and FPSCR.
  Explicit +vfp/+fp-arm/+fpregs features or Cargo feature `fpu` on soft-float
  targets also enable the FP frame. Enable this for native C built with softfp;
  the target must enable the corresponding hardware instructions.
  Switches are thread-mode calls, never exception returns; platform exception
  handlers own exception frames and interrupt state.
* RV32IMAC/IMAFC: s0-s11, SP/ra; IMAFC additionally saves fs0-fs11 and FCSR.
  The build script accounts for Rust not exposing unstable FP features in cfg,
  using built-in target names plus explicit +f/-f flags. Custom target specs
  need an audit of feature selection. RV32D and other custom ABIs are not claimed.
* Xtensa ESP32/ESP32-S3: deliberate compile-time error. Windowed ABI requires
  spilling every live register window to its owning stack before switching,
  restoring window base/start and return state using a verified call-window
  protocol, preserving ABI callee state/SAR as applicable, and handling spill
  exceptions under interrupts. This needs target-specific assembly and hardware
  tests; an SP-only swap is incorrect. Call0 is not implemented either.
* Wasm: deliberate compile-time error; no native stack switching.

Host tests cover real Embassy executor interleaving, signal wakeups and poll
counts, nested synchronous calls, async/native joins, results, thread isolation,
canary corruption detection, fallback and cancellation. The allocation-budget
test asserts exactly two runtime allocations and zero allocations over 100
suspend/resume cycles. Generic waker implementations may allocate themselves.
The task-manager adapter adds the manager's existing pool, metadata and join
allocations (including its current leaked task-pool behavior).

On x86_64: Stack is 24 bytes; Coroutine and JoinHandle are each 8 bytes; the shared
control allocation is 136 bytes including Rc counts. Each stack adds 64 guard
bytes plus up to 15 rounding bytes; the closure and initial register frame occupy
usable stack space. The x86_64 switch is 51 bytes and has 21 instructions per direction, including
FP control save/restore. `footprint_and_switch_timing` reports end-to-end
suspend/resume cost with TLS, polling and guard scans; run it in release mode for
representative timings (154 ns per round trip observed on the development host).
This measures two switches plus runtime work, not just
assembly latency. FP/context frame sizes are 72 bytes initially on x86_64,
176 on AArch64, 40/112 on ARM, and 64/128 on RV32 (integer/FP).

## ABI integration decisions still needed

Native thread creation must construct and spawn a driver on the owning executor,
set a bounded stack size and resource policy, and map integer results/join state
to the C ABI. Decide detach/cancel semantics before exposing thread termination;
forced cancellation currently leaks. Thread-local application state should follow
the coroutine identity, not the executor's OS thread. Interrupt/signal delivery,
foreign callbacks and TLS libraries need an explicit policy.

Mutex/condition-variable adapters need real async wait queues, atomic unlock plus
wait registration, wake-one/wake-all semantics and cancellation handling. Existing
OS-thread blocking mutexes cannot be used on a cooperative executor. Recursive
locks and ownership must identify coroutines. Blocking I/O should wrap the VFS
polling future via suspend_on, preserve borrowed buffer lifetimes and propagate
errors/partial transfers. Never block the executor's OS thread or use the fallback
inside managed native calls. Per-core current state and the Xtensa windowed switch
remain prerequisites for the reference ESP32-S3 hardware.
