#![cfg(all(feature = "host", feature = "graphics"))]
extern crate abi_definitions;
drivers_std::memory::instantiate_global_allocator!();
#[path = "support/graphics.rs"]
mod support;

use wasm::{
    graphics::{Function, GraphicsCall, GraphicsState, dispatch},
    translation::{GuestMemory, GuestPointer, GuestValue, WasmUsize},
};
use xila::{graphics::lvgl, memory::ManagerTrait};

fn call(
    state: &mut GraphicsState,
    memory: &mut GuestMemory<'_>,
    function: Function,
    arguments: &[WasmUsize],
) -> WasmUsize {
    let mut words = [0; 7];
    words[..arguments.len()].copy_from_slice(arguments);
    dispatch(
        state,
        memory,
        GraphicsCall {
            function: function.id(),
            arguments: words,
            count: arguments.len() as i32,
            result: 1,
        },
    )
    .unwrap();
    memory.read(GuestPointer::new(1)).unwrap_or(0)
}

#[xila::task::test(task_path = xila::task)]
async fn deleted_maps_are_reclaimed_without_reusing_handles() {
    let _standard = testing::initialize(false, false).await;
    let manager = support::manager().await;
    let mut bytes = [0; 2048];
    bytes[1000..2000].fill(b'A');
    let mut memory = GuestMemory::new(&mut bytes);
    for i in 0..8 {
        memory
            .write(
                GuestPointer::new(400 + i * WasmUsize::SIZE as WasmUsize),
                1000 as WasmUsize,
            )
            .unwrap();
    }
    memory
        .write(
            GuestPointer::new(400 + 8 * WasmUsize::SIZE as WasmUsize),
            0 as WasmUsize,
        )
        .unwrap();
    for mode in ["explicit", "parent", "external"] {
        let mut state = GraphicsState::new(Some(manager));
        let root = call(&mut state, &mut memory, Function::WindowCreate, &[]);
        let root_pointer = manager.get_window_identifier(0).await.unwrap() as *mut lvgl::lv_obj_t;
        let mut previous = root;
        for _ in 0..16 {
            let parent = if mode == "parent" {
                call(&mut state, &mut memory, Function::ObjectCreate, &[root])
            } else {
                root
            };
            let matrix = call(
                &mut state,
                &mut memory,
                Function::ButtonmatrixCreate,
                &[parent],
            );
            assert!(matrix > previous);
            previous = matrix;
            call(
                &mut state,
                &mut memory,
                Function::ButtonmatrixSetMap,
                &[matrix, 400],
            );
            let with_map = __DRIVERS_STD_MEMORY_ALLOCATOR.get_used();
            if mode == "external" {
                let _lock = manager.synchronous_lock();
                unsafe { lvgl::lv_obj_delete(lvgl::lv_obj_get_child(root_pointer, 0)) };
            } else {
                call(
                    &mut state,
                    &mut memory,
                    Function::ObjectDelete,
                    &[if mode == "parent" { parent } else { matrix }],
                );
            }
            // Next dispatch reaps external deletions before allocating again.
            call(&mut state, &mut memory, Function::Percentage, &[100]);
            let reclaimed = with_map.saturating_sub(__DRIVERS_STD_MEMORY_ALLOCATOR.get_used());
            // Measure deletion against the same registry capacity: handles are
            // never reused, so tombstones can legitimately outgrow one map.
            // Eight 1000-byte strings, their NULs, and the empty sentinel must
            // be released, in addition to the native widget allocations.
            assert!(
                reclaimed >= 8009,
                "{mode} deletion reclaimed only {reclaimed} bytes"
            );
            for _ in 0..128 {
                call(
                    &mut state,
                    &mut memory,
                    Function::WindowPopEvent,
                    &[root, 101, 107],
                );
                if memory.read(GuestPointer::<u32>::new(101)) == Some(0) {
                    break;
                }
            }
        }
        drop(state);
        assert_eq!(manager.get_window_count().await.unwrap(), 0);
    }
}
