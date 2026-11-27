//! The graphics call executor: one service behind every way a program reaches
//! the UI.
//!
//! A caller names a call by its catalog ID and passes up to
//! [`MAX_ARGUMENTS`] words. [`execute`] checks the call,
//! takes the graphics lock, resolves object handles through the caller's
//! [`State`], runs the call on LVGL and writes the result back. Whatever
//! differs between callers is behind [`Transport`]: how a pointer, string or
//! buffer word becomes memory. Wasm guests pass offsets into linear memory
//! ([`LinearMemory`]); native code passes host pointers.
//!
//! The executor owns what every caller needs and none should reimplement:
//!
//! * the graphics lock, held for exactly one call;
//! * the per-process object identity table in [`State`], deleted with the
//!   process;
//! * the event queue behind `window_pop_event`;
//! * the Xila extension calls (IDs from `0xf000`).
//!
//! The call table (the [`Function`] enum and its dispatch) is generated at
//! build time from the same catalog that generates guest bindings.

mod canvas;
mod error;
mod event_code;
mod events;
mod handlers;
mod marshal;
mod state;
pub(crate) mod transport;

pub use error::{Error, Result};
pub use event_code::{EVENT_CLOSE_REQUESTED, EventCode};
pub use state::State;
pub use transport::{LinearMemory, Transport, Word};

use state::Kind;
pub(crate) use state::drain_retired_states;

/// How a call reports its result: nothing, a pointer-sized handle, or a
/// 32-bit value, written at the call's `result` address.
pub(crate) enum ResultKind {
    None,
    Word,
    U32,
    I32,
}

/// One call. `arguments` beyond the function's count are ignored but must be
/// present; `count` must equal the function's argument count.
#[derive(Debug, Clone, Copy)]
pub struct Call {
    pub function: i32,
    pub arguments: [Word; MAX_ARGUMENTS],
    pub count: i32,
    pub result: Word,
}

include!(concat!(env!("OUT_DIR"), "/call_table.rs"));

/// Run one call for the process that owns `state`, blocking the thread while
/// another holder has the graphics lock. A caller on a cooperative executor
/// takes the lock itself and uses [`execute_locked`].
///
/// The result address is validated before anything runs, so a call never has
/// effects it cannot report.
pub fn execute(state: &mut State, transport: &mut dyn Transport, call: &Call) -> Result<()> {
    let (function, manager) = begin(state, transport, call)?;
    execute_in_session(
        &mut manager.synchronous_lock(),
        state,
        transport,
        call,
        function,
    )
}

/// Run a call after the caller has acquired the graphics lock.
pub fn execute_locked(
    guard: &mut crate::GraphicsGuard<'_>,
    state: &mut State,
    transport: &mut dyn Transport,
    call: &Call,
) -> Result<()> {
    let (function, _) = begin(state, transport, call)?;
    execute_in_session(guard, state, transport, call, function)
}

fn execute_in_session(
    guard: &mut crate::GraphicsGuard<'_>,
    state: &mut State,
    transport: &mut dyn Transport,
    call: &Call,
    function: Function,
) -> Result<()> {
    guard
        .with_lvgl(|access| {
            state.initialize(access)?;
            run(state, transport, call, function, access)
        })
        .map_err(|_| Error::NativeFailure)?
}

/// Check the call and find the manager it runs on.
fn begin(
    state: &State,
    transport: &mut dyn Transport,
    call: &Call,
) -> Result<(Function, &'static crate::Manager)> {
    let (function, argument_count, result_kind) =
        lookup(call.function).ok_or(Error::Unsupported)?;
    if argument_count != call.count {
        return Err(Error::InvalidArgumentsCount);
    }
    let manager = state.manager.ok_or(Error::Unavailable)?;
    match result_kind {
        ResultKind::None => (),
        ResultKind::Word => transport.validate(call.result, transport.word_size())?,
        ResultKind::U32 | ResultKind::I32 => transport.validate(call.result, size_of::<u32>())?,
    }
    Ok((function, manager))
}
