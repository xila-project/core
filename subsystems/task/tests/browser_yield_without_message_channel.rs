//! `task::yield_now` where the browser exposes no `MessageChannel`, or nothing
//! else either, as in an `AudioWorkletGlobalScope`.
//!
//! The channel is chosen on first use and kept, so each scenario needs its own
//! test binary (one browser page): this file covers the missing channel, the
//! next one the missing timers.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use js_sys::{Date, Reflect};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
async fn yields_fall_back_to_a_zero_delay_timer() {
    let global = js_sys::global();
    assert!(Reflect::delete_property(&global, &"MessageChannel".into()).unwrap());
    assert!(!Reflect::has(&global, &"MessageChannel".into()).unwrap());

    let start = Date::now();
    for _ in 0..20 {
        task::yield_now().await;
    }
    // Zero-delay timers: each call takes at most a few milliseconds.
    assert!(Date::now() - start < 1_000.0);
}
