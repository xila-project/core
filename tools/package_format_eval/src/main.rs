use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("generate") => {
            let mut output = None;
            let mut seed = 0_u64;
            while let Some(argument) = args.next() {
                match argument.as_str() {
                    "--output" => output = args.next().map(PathBuf::from),
                    "--seed" => {
                        seed = args
                            .next()
                            .ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "missing seed",
                                )
                            })?
                            .parse()
                            .map_err(|_| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "invalid seed",
                                )
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
            let written_to = package_format_eval::write_fixtures(&output, seed)?;
            println!(
                "wrote {} fixtures to {}",
                package_format_eval::fixtures(seed)?.len(),
                written_to.display()
            );
            Ok(())
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "usage: package_format_eval generate --output DIR [--seed N]",
        )),
    }
}
