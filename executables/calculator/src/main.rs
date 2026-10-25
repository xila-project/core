embedded_i18n::track_locales!();

#[cfg(any(target_arch = "wasm32", test))]
mod engine;
#[cfg(any(target_arch = "wasm32", test))]
mod install;

#[cfg(target_arch = "wasm32")]
mod interface;

#[cfg(target_arch = "wasm32")]
fn main() {
    let mut calculator_gui = interface::Interface::new().unwrap();
    unsafe {
        calculator_gui.run().unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!(
        "{}",
        embedded_i18n::translate!("errors.requires_wasm_runtime")
    );
}
