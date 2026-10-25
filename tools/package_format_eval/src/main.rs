use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("evaluate") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "usage: package_format_eval evaluate --output DIR [--seed N]",
        ));
    }

    let mut output = None;
    let mut seed = 0_u64;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--output" => output = args.next().map(PathBuf::from),
            "--seed" => {
                seed = args
                    .next()
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing seed")
                    })?
                    .parse()
                    .map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid seed")
                    })?;
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("unknown argument: {argument}"),
                ));
            }
        }
    }
    let output = output.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "--output is required")
    })?;
    let archives = package_format_eval::write_evaluation(&output, seed)?;
    for archive in archives {
        println!("{}: {} bytes", archive.format.name(), archive.bytes.len());
    }
    println!("summary: {}", output.join("summary.csv").display());
    Ok(())
}
