embedded_i18n::track_locales!();

#[cfg(target_arch = "wasm32")]
mod interface;
#[cfg(any(target_arch = "wasm32", test))]
mod storage;

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn __install() {
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all("/configuration/shared/shortcuts")?;
        std::fs::write(
            "/configuration/shared/shortcuts/pixel_painter.json",
            application_metadata::shortcut("pixel_painter").unwrap(),
        )
    })();
    if let Err(error) = result {
        eprintln!("Pixel Painter installation failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    if let Err(error) = interface::run() {
        eprintln!("Pixel Painter graphics error: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Pixel Painter is a Xila Wasm application. Build for wasm32-wasip1.");
}
