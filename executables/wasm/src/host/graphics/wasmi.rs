use super::{GraphicsCall, GraphicsError, dispatch};
use crate::host::{
    store::GlobalStore,
    translation::{GuestMemory, WasmUsize, get_memory},
};
use wasmi::{Caller, Linker};

#[allow(clippy::too_many_arguments)]
fn call(
    mut caller: Caller<'_, GlobalStore>,
    function: i32,
    a: WasmUsize,
    b: WasmUsize,
    c: WasmUsize,
    d: WasmUsize,
    e: WasmUsize,
    f: WasmUsize,
    g: WasmUsize,
    count: i32,
    result: WasmUsize,
) -> i32 {
    let Some(memory) = get_memory(&caller) else {
        return GraphicsError::InvalidMemory as i32;
    };
    let context_manager = caller.data().context.graphics_manager;
    let state_manager = caller.data().graphics.manager;
    if !matches!((context_manager, state_manager), (Some(context), Some(state)) if core::ptr::eq(context, state))
    {
        return GraphicsError::Unavailable as i32;
    }
    let (bytes, store) = memory.data_and_store_mut(&mut caller);
    store.graphics.file_system = Some((store.context.virtual_file_system, store.wasi.task));
    dispatch(
        &mut store.graphics,
        &mut GuestMemory::new(bytes),
        GraphicsCall {
            function,
            arguments: [a, b, c, d, e, f, g],
            count,
            result,
        },
    )
    .map_or_else(|error| error as i32, |()| 0)
}

pub fn add_to_linker(linker: &mut Linker<GlobalStore>) -> crate::host::error::Result<()> {
    linker.func_wrap("host", "xila_graphics_call", call)?;
    Ok(())
}
