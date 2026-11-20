//! Browser macrotask yield for `wasm32-unknown-unknown`.
//!
//! Embassy's wasm pender wakes the executor through `Promise.then`, a microtask,
//! so a self-waking task never lets the browser run timers, input or rendering.
//! A `MessageChannel` message is a real macrotask but, unlike `setTimeout`, is
//! not clamped to a minimum delay (nested timers wait at least 4 ms), so a
//! yield costs well under a millisecond.
//!
//! The channel is created on first use. Scopes without `MessageChannel` (the
//! `AudioWorkletGlobalScope`, which also lacks `window` and `setTimeout`) can
//! link this crate and never pay for it; if a yield is requested there anyway, it
//! degrades to a zero-delay timer, then to a microtask.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
let channel = null;
const waiting = [];

function receive() {
    const resolve = waiting.shift();
    // Node keeps the process alive while a port is referenced: only hold it
    // while a yield is pending.
    if (waiting.length === 0 && channel.port1.unref) channel.port1.unref();
    resolve();
}

function open() {
    if (typeof MessageChannel !== 'function') return false;
    try {
        const created = new MessageChannel();
        created.port1.onmessage = receive;
        if (created.port1.unref) created.port1.unref();
        channel = created;
        return true;
    } catch (_) {
        return false;
    }
}

export function macrotask() {
    return new Promise((resolve) => {
        if (channel === null && !open()) channel = false;
        if (channel) {
            if (waiting.length === 0 && channel.port1.ref) channel.port1.ref();
            waiting.push(resolve);
            channel.port2.postMessage(0);
        } else if (typeof setTimeout === 'function') {
            setTimeout(resolve, 0);
        } else {
            resolve();
        }
    });
}
"#)]
extern "C" {
    fn macrotask() -> js_sys::Promise;
}

/// Resolve in a later browser task, after pending timers, input and rendering
/// work that is already due.
pub(crate) async fn yield_now() {
    // The promise never rejects.
    let _ = wasm_bindgen_futures::JsFuture::from(macrotask()).await;
}
