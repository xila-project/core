#[macro_export]
macro_rules! lock {
    ($body:block) => {{
        let _lock = graphics_manager.lock().await;
        let __result = { $body };
        ::core::mem::drop(_lock);
        __result
    }};
    ($manager:expr, $body:block) => {{
        let _lock = $manager.lock().await;
        let __result = { $body };
        ::core::mem::drop(_lock);
        __result
    }};
}
