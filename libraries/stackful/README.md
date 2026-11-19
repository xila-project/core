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
Panics from Future::poll happen on the driver stack, whose normal Drop leaks any
suspended native stack. Native destructors that panic also abort at the boundary.
An unstarted entry is dropped normally. A finished entry frees its stack normally.
Dropping a started, unfinished driver deliberately leaks its stack and control
block, including live future/resources, and wakes its observer with Cancelled.
This avoids freeing live stack objects; there is no forced unwinding or safe
cancellation of arbitrary C code. Prefer driving to completion. Process exit can
reclaim leaked host memory. Native code must never longjmp across these boundaries.

## Switch implementation and targets

The default backend uses naked assembly. The
[corosensei 0.3.4 probe](probe/README.md) keeps Xila's runtime and swaps the
switching backend for measurements. The crates.io release requires a protected
guard page and 4 KiB of usable stack. Xila's 64-byte painted guard and 2 KiB
minimum require a separate audited-stack contract. The probe also requires a
Cortex-M floating-point clobber patch. Normal builds and unsupported probe
targets use the existing backend. With the corosensei fork's Xtensa backend the
probe also builds and runs on ESP32 and ESP32-S3 (see the probe README).

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
* Xtensa ESP32/ESP32-S2/ESP32-S3: deliberate compile-time error without the
  corosensei probe (the probe uses the fork's Xtensa backend). Windowed ABI requires
  spilling every live register window to its owning stack before switching,
  restoring window base/start and return state using a verified call-window
  protocol, preserving ABI callee state/SAR as applicable, and handling spill
  exceptions under interrupts. This needs target-specific assembly and hardware
  tests; an SP-only swap is incorrect. Call0 is not implemented either.
* Wasm: deliberate compile-time error; no native stack switching.

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
the shared control allocation is 168 bytes including `Rc` counts (up from 136
bytes by 32 bytes for `4 * usize` inline TLS slots). On 32-bit ARM
(`thumbv7em-none-eabihf`), the shared control allocation is 88 bytes including
`Rc` counts (up from 72 bytes by 16 bytes for the 4 TLS slots; `Control` itself
grew from 64 to 80 bytes). Each stack adds 64 guard bytes plus up to 15
rounding bytes; the closure and initial register frame occupy usable stack
space. The x86_64 switch is 51 bytes and has 21 instructions per direction,
including FP control save/restore. `footprint_and_switch_timing` measures
two switches plus runtime work per suspend/resume round trip. Run it in release
mode for representative timings. FP/context frame sizes are 72 bytes initially on x86_64, 176
on AArch64, 40/112 on ARM, and 64/128 on RV32 (integer/FP).

## ABI integration status and remaining decisions

Synchronous VFS calls that bridge async operations and the native application
ABI use `suspend_on` on supported targets. The synchronous VFS polling
adapters still return resource-busy on pending device I/O. The application ABI
waits cooperatively for blocking file I/O when called from a coroutine.

Remaining work for later milestones: native thread creation must construct and
spawn a driver on the owning executor, set a bounded stack size and resource
policy, and map integer results/join state to the C ABI. Decide detach/cancel
semantics before exposing thread termination; forced cancellation currently
leaks. Interrupt/signal delivery, foreign callbacks and libc TLS slot
assignments need an explicit policy. Mutex/condition-variable adapters need
real async wait queues, atomic unlock plus wait registration, wake-one/wake-all
semantics and cancellation handling; recursive locks and ownership must
identify coroutines. Per-core current state and the Xtensa windowed switch
remain prerequisites for multi-core (core#682) and ESP32-S3 (core#648).
