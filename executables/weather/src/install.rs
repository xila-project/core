#[unsafe(no_mangle)]
pub extern "C" fn __install() {
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all("/configuration/shared/shortcuts")?;
        std::fs::write(
            "/configuration/shared/shortcuts/weather.json",
            application_metadata::shortcut("weather").unwrap(),
        )
    })();
    if let Err(error) = result {
        eprintln!("Installation failed: {error}");
        std::process::exit(1);
    }
}
