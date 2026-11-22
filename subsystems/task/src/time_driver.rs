//! Embassy time driver for `wasm32-unknown-unknown`.
//!
//! It replaces the one behind `embassy-time/wasm`, which measures time with
//! `wasm-timer` (an unmaintained crate whose clock is floored to whole
//! milliseconds) and re-arms with `setTimeout(microseconds / 1000)`, which
//! floors to `setTimeout(0)` and so ends up clamped to 4 ms by Chromium. Every
//! timer therefore paid about 4 ms.
//!
//! This driver reads `performance.now()` in microseconds and rounds the delay
//! of `setTimeout` up to the next millisecond, so a timer never fires early and
//! never waits for a clamped re-arm. The browser's own resolution is the only
//! remaining error.
//!
//! Without `setTimeout` (the `AudioWorkletGlobalScope`) no timer can ever fire:
//! [`task::sleep`](crate::sleep) does not complete there, while
//! [`task::yield_now`](crate::yield_now) still works.

use embassy_time_driver::Driver;
use embassy_time_queue_utils::Queue;
use std::sync::Mutex;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
// `performance.now()` starts at the page load; `Date.now()` stands in where a
// scope has no `performance`. Keep the clock monotonic either way.
const source = typeof performance === 'object' && performance !== null && typeof performance.now === 'function'
    ? () => performance.now()
    : () => Date.now();
const origin = source();
let last = 0;
let pending = null;

export function now_microseconds() {
    last = Math.max(last, Math.floor((source() - origin) * 1000));
    return last;
}

// Replace the pending timer. The token stays here: `setTimeout` returns a
// number in browsers but an object in Node.
export function arm(callback, milliseconds) {
    if (pending !== null) clearTimeout(pending);
    pending = null;
    if (typeof setTimeout !== 'function') return;
    pending = setTimeout(() => {
        pending = null;
        callback();
    }, milliseconds);
}
"#)]
extern "C" {
    fn now_microseconds() -> f64;
    fn arm(callback: &Closure<dyn FnMut()>, milliseconds: u32);
}

/// The longest delay `setTimeout` accepts (a signed 32-bit number of
/// milliseconds); a longer wait is simply armed again when this one elapses.
const MAXIMUM_DELAY_MILLISECONDS: u64 = i32::MAX as u64;

struct Inner {
    queue: Queue,
    closure: Option<Closure<dyn FnMut()>>,
}

struct TimeDriver {
    inner: Mutex<Inner>,
}

// The browser runs this crate on one thread, and the closure is only touched
// while holding the lock.
unsafe impl Send for Inner {}

embassy_time_driver::time_driver_impl!(static DRIVER: TimeDriver = TimeDriver {
    inner: Mutex::new(Inner {
        queue: Queue::new(),
        closure: None,
    }),
});

fn now() -> u64 {
    now_microseconds() as u64
}

/// How long to wait, in whole milliseconds and rounded up, until `timestamp`.
fn delay_milliseconds(timestamp: u64, now: u64) -> u32 {
    let milliseconds = (timestamp - now).div_ceil(1_000);
    milliseconds.min(MAXIMUM_DELAY_MILLISECONDS) as u32
}

impl Inner {
    /// Wake every due timer and arm the browser timer for the next one.
    fn dispatch(&mut self) {
        let now = now();
        let next = self.queue.next_expiration(now);
        // `u64::MAX`: nothing is scheduled. A timer that is still armed then only
        // runs this function once more.
        if next != u64::MAX {
            let closure = self.closure.get_or_insert_with(|| Closure::new(dispatch));
            arm(closure, delay_milliseconds(next, now));
        }
    }
}

impl Driver for TimeDriver {
    fn now(&self) -> u64 {
        now()
    }

    fn schedule_wake(&self, at: u64, waker: &core::task::Waker) {
        let mut inner = self.inner.lock().unwrap();
        if inner.queue.schedule_wake(at, waker) {
            inner.dispatch();
        }
    }
}

fn dispatch() {
    DRIVER.inner.lock().unwrap().dispatch();
}
