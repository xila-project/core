#[cfg(target_arch = "wasm32")]
mod device;
#[cfg(target_arch = "wasm32")]
mod install;
#[cfg(target_arch = "wasm32")]
mod interface;

#[cfg(target_arch = "wasm32")]
fn main() {
    let mut interface = interface::Interface::new().unwrap();
    unsafe {
        interface.run().unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Build this executable for wasm32-wasip1 to run the Xila synthesizer.");
}
