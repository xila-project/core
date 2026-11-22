//! `task::yield_now` where the browser exposes no `MessageChannel`, or nothing
//! else either, as in an `AudioWorkletGlobalScope`.
//!
//! The channel is chosen on first use and kept, so each scenario needs its own
//! test binary (one browser page): this file covers the missing channel, the
//! next one the missing timers.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

extern crate alloc;

use js_sys::{Function, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn yields_fall_back_to_a_zero_delay_timer() {
    let global = js_sys::global();
    assert!(Reflect::delete_property(&global, &"MessageChannel".into()).unwrap());
    assert!(!Reflect::has(&global, &"MessageChannel".into()).unwrap());

    // A microtask fallback also finishes quickly, but it would spin forever
    // here: only a real browser task lets a due timer run between yields.
    let fired = alloc::rc::Rc::new(core::cell::Cell::new(false));
    let flag = fired.clone();
    let callback = wasm_bindgen::closure::Closure::once_into_js(move || flag.set(true));
    let set_timeout: Function = Reflect::get(&global, &"setTimeout".into())
        .unwrap()
        .unchecked_into();
    set_timeout
        .call2(&JsValue::NULL, &callback, &0.into())
        .unwrap();

    let mut yields = 0_u32;
    while !fired.get() {
        assert!(yields < 100_000, "a due timer never ran between yields");
        task::yield_now().await;
        yields += 1;
    }
}
