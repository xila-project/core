use crate::{
    define_wasi_module,
    host::{
        execution::{PendingCall, Suspend, clock_value},
        store::GlobalStore,
        translation::{WasmUsize, borrow_memory, get_memory},
        wasi::poll_memory::ClockPoll,
    },
};
use wasmi::Caller;

define_wasi_module! {
    module: "wasi_snapshot_preview1";
    fn poll_oneoff(caller:Caller<GlobalStore>, subscriptions:WasmUsize, events:WasmUsize,
        count:WasmUsize,written:WasmUsize)->Result<i32,wasmi::Error> {
        let mut caller=caller;
        let Some(memory)=get_memory(&caller) else {return Ok(21)};
        let manager=caller.data().context.time_manager;
        let poll=match ClockPoll::prepare(&borrow_memory(&memory,&mut caller),subscriptions,events,count,written,|id| clock_value(manager,id)) {
            Ok(poll)=>poll,Err(error)=>return Ok(error as i32),
        };
        caller.data_mut().pending=Some(PendingCall::ClockPoll {poll,memory});
        Err(wasmi::Error::host(Suspend))
    }
}
