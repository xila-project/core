//! `task::sleep` on `wasm32-unknown-unknown`, in headless Chrome.
//!
//! The `wasm-timer` clock behind `embassy-time/wasm` is floored to whole
//! milliseconds and re-armed `setTimeout(0)`, which Chromium clamps to 4 ms, so
//! every timer paid about 4 ms: a 2 ms sleep took 4 to 6 ms and a 10 ms sleep
//! about 14 ms.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

use core::time::Duration;
use embassy_executor::Executor;
use js_sys::{Date, Function, Reflect};
use portable_atomic::{AtomicBool, AtomicU32, Ordering};
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const SAMPLES: u32 = 20;
const SHORT_MILLISECONDS: u64 = 2;
const LONG_MILLISECONDS: u64 = 10;
const LONG_LIMIT_MICROSECONDS: u32 = 12_500;

static FINISHED: AtomicBool = AtomicBool::new(false);
static SHORT_TOTAL_MICROSECONDS: AtomicU32 = AtomicU32::new(0);
static LONG_TOTAL_MICROSECONDS: AtomicU32 = AtomicU32::new(0);
static EARLY_WAKES: AtomicU32 = AtomicU32::new(0);

/// `performance.now()` in milliseconds: finer than `Date.now()`.
fn now() -> f64 {
    let performance = Reflect::get(&js_sys::global(), &"performance".into()).unwrap();
    let now: Function = Reflect::get(&performance, &"now".into())
        .unwrap()
        .unchecked_into();
    now.call0(&performance).unwrap().as_f64().unwrap()
}

/// Average cost of `sleep(milliseconds)` over `SAMPLES` calls, in microseconds.
async fn measure(milliseconds: u64) -> u32 {
    let mut total = 0.0;
    for _ in 0..SAMPLES {
        let start = now();
        task::sleep(Duration::from_millis(milliseconds)).await;
        let elapsed = now() - start;
        // A wait must never end before its duration (allow for the resolution
        // the browser gives `performance.now`).
        if elapsed < milliseconds as f64 - 0.2 {
            EARLY_WAKES.fetch_add(1, Ordering::SeqCst);
        }
        total += elapsed;
    }
    (total / f64::from(SAMPLES) * 1_000.0) as u32
}

#[embassy_executor::task]
async fn sleeper() {
    SHORT_TOTAL_MICROSECONDS.store(measure(SHORT_MILLISECONDS).await, Ordering::SeqCst);
    LONG_TOTAL_MICROSECONDS.store(measure(LONG_MILLISECONDS).await, Ordering::SeqCst);
    FINISHED.store(true, Ordering::SeqCst);
}

#[wasm_bindgen_test]
async fn sleeps_overshoot_by_far_less_than_the_timer_clamp() {
    // `embassy_time::Timer` only works inside an Embassy task.
    let executor: &'static mut Executor = Box::leak(Box::new(Executor::new()));
    executor.start(|spawner| spawner.spawn(sleeper().unwrap()));

    let start = Date::now();
    while !FINISHED.load(Ordering::SeqCst) {
        assert!(Date::now() - start < 10_000.0, "the sleeps never finished");
        wasm_bindgen_futures::JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
            let set_timeout: Function = Reflect::get(&js_sys::global(), &"setTimeout".into())
                .unwrap()
                .unchecked_into();
            set_timeout
                .call2(&wasm_bindgen::JsValue::NULL, &resolve, &0.into())
                .unwrap();
        }))
        .await
        .unwrap();
    }

    assert_eq!(EARLY_WAKES.load(Ordering::SeqCst), 0, "a sleep ended early");

    // The old driver measured 4 to 6 ms for the short sleep and 13.5 ms for the
    // long one; the new one about 3.7 and 10.5. The limit sits between them, wide
    // enough for a busy runner, and only the long sleep tells the two apart: the
    // short one is dominated by the browser's own timer latency.
    let short = SHORT_TOTAL_MICROSECONDS.load(Ordering::SeqCst);
    let long = LONG_TOTAL_MICROSECONDS.load(Ordering::SeqCst);
    assert!(
        long < LONG_LIMIT_MICROSECONDS,
        "sleep(10 ms) took {long} us on average (sleep(2 ms): {short} us)"
    );
}
