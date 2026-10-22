#[unsafe(no_mangle)]
pub extern "C" fn __install() {
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all("/configuration/shared/applications/shortcuts")?;
        std::fs::write(
            "/configuration/shared/applications/shortcuts/calculator.json",
            application_metadata::shortcut("calculator").unwrap(),
        )
    })();
    if let Err(error) = result {
        eprintln!("Installation failed: {error}");
        std::process::exit(1);
    }
}
