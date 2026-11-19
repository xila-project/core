// The registry crate requires a protected guard and at least 4 KiB. A local
// measurement-only patch must explicitly opt into Xila's audited stack bounds.
use corosensei::stack::StackPointer;

const _: () = corosensei::stack::XILA_AUDITED_HEAP_STACK_PROBE;

use super::{Stack, current};

#[path = "corosensei_fp.rs"]
mod fp;

pub(super) type Native = corosensei::Coroutine<(), (), (), AuditedStack>;
pub(super) type Yielder = corosensei::Yielder<(), ()>;

// Keep the patched stack contract private to the unsafe Xila constructor.
#[repr(transparent)]
pub(super) struct AuditedStack(Stack);

// SAFETY: for this patched crate only, Xila's Coroutine::new requires the
// caller to bound all stack use. The stack owns a stable 16-byte-aligned
// allocation and reports its bounds including the painted guard.
unsafe impl corosensei::stack::Stack for AuditedStack {
    fn base(&self) -> StackPointer {
        StackPointer::new(self.0.top()).unwrap()
    }

    fn limit(&self) -> StackPointer {
        StackPointer::new(self.0.guard().as_ptr() as usize).unwrap()
    }
}

pub(super) unsafe fn new<F: FnOnce() -> i32 + 'static>(stack: Stack, entry: F) -> Native {
    // Leave room for the coroutine root and its alignment padding before
    // corosensei writes the initial frame. Its own closure limit is 1024 B.
    let reservation = core::mem::size_of::<F>()
        .saturating_add(core::mem::align_of::<F>() - 1)
        .saturating_add(128);
    assert!(
        stack.usable_size().saturating_sub(reservation) >= 1024,
        "entry leaves too little stack"
    );
    Native::with_stack(AuditedStack(stack), move |yielder, ()| {
        // SAFETY: the driver sets CURRENT before entering. The yielder remains
        // valid until this invocation returns, including across suspension.
        let control = unsafe { &*current() };
        control.yielder.set(yielder);
        fp::Environment::initialize();
        let result = entry();
        // SAFETY: this is the running coroutine's control block.
        unsafe { super::finish(control, result) }
    })
}

/// corosensei clobbers the FP data registers but does not save the FP environment.
/// Keep each side's environment on its own stack over the transfer.
#[inline]
pub(super) fn preserving_fp(switch: impl FnOnce()) {
    let environment = fp::Environment::read();
    switch();
    environment.write();
}
