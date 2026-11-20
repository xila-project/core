//! `task::yield_now` where neither `MessageChannel` nor `setTimeout` exists (an
//! `AudioWorkletGlobalScope`): it must resolve instead of hanging or throwing.
//! See `browser_yield_without_message_channel.rs` for why this is its own file.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use js_sys::Reflect;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn yields_resolve_without_message_channel_or_timers() {
    let global = js_sys::global();
    let set_timeout = Reflect::get(&global, &"setTimeout".into()).unwrap();
    assert!(Reflect::delete_property(&global, &"MessageChannel".into()).unwrap());
    // Keep the harness working: only hide the timer from the yield itself.
    Reflect::set(
        &global,
        &"setTimeout".into(),
        &wasm_bindgen::JsValue::UNDEFINED,
    )
    .unwrap();

    for _ in 0..20 {
        task::yield_now().await;
    }

    Reflect::set(&global, &"setTimeout".into(), &set_timeout).unwrap();
}
