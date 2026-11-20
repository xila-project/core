//! `task::yield_now` on `wasm32-unknown-unknown`, in headless Chrome.
//!
//! The timer-backed yield it replaced took 10 ms plus a timer re-arm per call.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

extern crate alloc;

use core::time::Duration;
use embassy_executor::Executor;
use js_sys::{Date, Function, Reflect};
use portable_atomic::{AtomicBool, AtomicU32, Ordering};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const YIELDS: u32 = 200;

#[wasm_bindgen_test]
async fn consecutive_yields_are_far_faster_than_the_timer_yield() {
    // Warm up: the first call creates the channel.
    task::yield_now().await;

    let start = Date::now();
    for _ in 0..YIELDS {
        task::yield_now().await;
    }
    let elapsed = Date::now() - start;

    // The timer-backed yield needed at least 10 ms each: 2 s for 200 yields.
    // A macrotask takes well under a millisecond; leave room for a slow runner.
    assert!(
        elapsed < f64::from(YIELDS),
        "{YIELDS} yields took {elapsed} ms (the timer-backed yield took >= {} ms)",
        YIELDS * 10
    );
}

#[wasm_bindgen_test]
async fn a_yield_lets_due_timers_run() {
    // A microtask yield would spin forever: a due timer is a separate task.
    let fired = alloc::rc::Rc::new(core::cell::Cell::new(false));
    let flag = fired.clone();
    let callback = wasm_bindgen::closure::Closure::once_into_js(move || flag.set(true));
    let set_timeout: Function = Reflect::get(&js_sys::global(), &"setTimeout".into())
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

#[wasm_bindgen_test]
async fn concurrent_yields_all_complete() {
    async fn repeat() -> u32 {
        for _ in 0..10 {
            task::yield_now().await;
        }
        10
    }
    let (a, b, c, d) = embassy_futures::join::join4(repeat(), repeat(), repeat(), repeat()).await;
    assert_eq!(a + b + c + d, 40);
}

// `embassy_time::Timer` (and so `sleep` and `yield_throttled`) only works inside
// an Embassy task, which is how the Wasm runtime calls it.
static TIMED: AtomicBool = AtomicBool::new(false);
static THROTTLED_MS: AtomicU32 = AtomicU32::new(0);
static SLEEP_MS: AtomicU32 = AtomicU32::new(0);
static EXECUTOR_YIELD_MS: AtomicU32 = AtomicU32::new(0);

/// Wait one browser task without involving `task`.
async fn timeout_zero() {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let set_timeout: Function = Reflect::get(&js_sys::global(), &"setTimeout".into())
            .unwrap()
            .unchecked_into();
        set_timeout
            .call2(&JsValue::NULL, &resolve, &0.into())
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
}

fn elapsed_since(start: f64) -> u32 {
    (Date::now() - start) as u32
}

#[embassy_executor::task]
async fn timed() {
    let start = Date::now();
    task::yield_throttled().await;
    THROTTLED_MS.store(elapsed_since(start), Ordering::SeqCst);

    let start = Date::now();
    task::sleep(Duration::from_millis(30)).await;
    SLEEP_MS.store(elapsed_since(start), Ordering::SeqCst);

    let start = Date::now();
    for _ in 0..YIELDS {
        task::yield_now().await;
    }
    EXECUTOR_YIELD_MS.store(elapsed_since(start), Ordering::SeqCst);
    TIMED.store(true, Ordering::SeqCst);
}

#[wasm_bindgen_test]
async fn timers_and_yields_inside_an_embassy_task() {
    let executor: &'static mut Executor = Box::leak(Box::new(Executor::new()));
    executor.start(|spawner| spawner.spawn(timed().unwrap()));

    let start = Date::now();
    while !TIMED.load(Ordering::SeqCst) {
        assert!(
            Date::now() - start < 10_000.0,
            "the Embassy task never finished"
        );
        timeout_zero().await;
    }

    // The throttled yield sleeps 10 ms and a sleep takes at least its duration.
    assert!(THROTTLED_MS.load(Ordering::SeqCst) >= 9);
    assert!(SLEEP_MS.load(Ordering::SeqCst) >= 29);
    // Yielding from the executor stays far below the timer-backed 10 ms each.
    let yielded = EXECUTOR_YIELD_MS.load(Ordering::SeqCst);
    assert!(
        yielded < YIELDS,
        "{YIELDS} executor yields took {yielded} ms"
    );
}
