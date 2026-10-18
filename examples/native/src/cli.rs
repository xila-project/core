use getargs_derive::GetArgs;
use std::string::{String, ToString};

pub const MINIMUM_DISK_SIZE: u64 = (xila::file_system::mbr::Mbr::MINIMUM_START_BLOCK + 2)
    * xila::file_system::mbr::Mbr::SIZE as u64;

// The parser displays range expressions literally; keep its numeric bound and help in sync.
const _: () = assert!(MINIMUM_DISK_SIZE == 1049600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceMode {
    Graphical,
    Terminal,
}

pub struct Config {
    pub interface: InterfaceMode,
    pub networking: bool,
    pub resolution: (i16, i16),
    pub disk_path: String,
    pub seed_image: String,
    pub disk_size: u64,
    pub interface_name: String,
}

#[derive(GetArgs)]
#[command(
    name = "native_example",
    about = "Run the Xila native example.",
    version
)]
struct Arguments<'a> {
    /// Use the graphical interface (default).
    #[arg(short = 'g', long = "gui", conflicts_with = "terminal")]
    gui: bool,
    /// Use the terminal interface.
    #[arg(short = 't', long = "terminal")]
    terminal: bool,
    /// Enable networking (default).
    #[arg(short = 'n', long = "network", conflicts_with = "no_network")]
    network: bool,
    /// Disable networking.
    #[arg(short = 'N', long = "no-network")]
    no_network: bool,
    /// Display resolution; each dimension must be between 1 and 32767.
    #[arg(short = 'r', long = "resolution", default_value = "800x600", value_name = "WIDTHxHEIGHT", value_parser = parse_resolution)]
    resolution: (i16, i16),
    /// Backing disk image.
    #[arg(
        short = 'p',
        long = "disk-path",
        default_value = "./drive.img",
        value_name = "PATH",
        allow_hyphen_values = true
    )]
    disk_path: &'a str,
    /// Reference image imported only when the backing disk is absent.
    #[arg(
        long = "seed-image",
        default_value = "./seed.img",
        value_name = "PATH",
        allow_hyphen_values = true
    )]
    seed_image: &'a str,
    /// Capacity in bytes (minimum: 1049600 for the MBR and filesystem).
    #[arg(short = 's', long = "disk-size", default_value_t = 16 * 1024 * 1024, value_name = "BYTES", value_parser = 1049600..)]
    disk_size: u64,
    /// Host TUN/TAP interface.
    #[arg(short = 'i', long = "interface-name", default_value = "xila0", value_name = "NAME", value_parser = non_empty, allow_hyphen_values = true)]
    interface_name: &'a str,
}

fn parse_resolution(value: &str) -> Result<(i16, i16), ()> {
    let (width, height) = value.split_once('x').ok_or(())?;
    let width = width.parse::<i16>().map_err(|_| ())?;
    let height = height.parse::<i16>().map_err(|_| ())?;
    if width <= 0 || height <= 0 {
        return Err(());
    }
    Ok((width, height))
}

fn non_empty(value: &str) -> Result<&str, ()> {
    if value.is_empty() { Err(()) } else { Ok(value) }
}

pub fn parse<'a, I>(arguments: I) -> Result<Config, getargs_derive::Error<'a>>
where
    I: IntoIterator<Item = &'a str>,
{
    let arguments = Arguments::from_args(arguments.into_iter())?;

    Ok(Config {
        interface: if arguments.gui || !arguments.terminal {
            InterfaceMode::Graphical
        } else {
            InterfaceMode::Terminal
        },
        networking: arguments.network || !arguments.no_network,
        resolution: arguments.resolution,
        disk_path: arguments.disk_path.to_string(),
        seed_image: arguments.seed_image.to_string(),
        disk_size: arguments.disk_size,
        interface_name: arguments.interface_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{InterfaceMode, MINIMUM_DISK_SIZE, parse};

    #[test]
    fn defaults_preserve_the_current_example_setup() {
        let config = parse([] as [&str; 0]).unwrap();

        assert_eq!(config.interface, InterfaceMode::Graphical);
        assert!(config.networking);
        assert_eq!(config.resolution, (800, 600));
        assert_eq!(config.disk_path, "./drive.img");
        assert_eq!(config.disk_size, 16 * 1024 * 1024);
        assert_eq!(config.interface_name, "xila0");
    }

    #[test]
    fn parses_runtime_options() {
        let config = parse([
            "--terminal",
            "--no-network",
            "--resolution",
            "1024x768",
            "--disk-path",
            "/tmp/xila.img",
            "--disk-size",
            "33554432",
            "--interface-name",
            "xila1",
        ])
        .unwrap();

        assert_eq!(config.interface, InterfaceMode::Terminal);
        assert!(!config.networking);
        assert_eq!(config.resolution, (1024, 768));
        assert_eq!(config.disk_path, "/tmp/xila.img");
        assert_eq!(config.disk_size, 33_554_432);
        assert_eq!(config.interface_name, "xila1");
    }

    #[test]
    fn preserves_hyphen_prefixed_string_option_values() {
        let config = parse(["--disk-path", "-drive.img", "--interface-name", "-xila0"]).unwrap();
        assert_eq!(config.disk_path, "-drive.img");
        assert_eq!(config.interface_name, "-xila0");
    }

    #[test]
    fn rejects_conflicting_modes() {
        assert!(parse(["--terminal", "--gui"]).is_err());
        assert!(parse(["--network", "--no-network"]).is_err());
        assert!(parse(["--gui", "--terminal"]).is_err());
        assert!(parse(["--no-network", "--network"]).is_err());
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(parse(["--resolution", "0x600"]).is_err());
        assert!(parse(["--resolution", "800"]).is_err());
        assert!(parse(["--resolution", "32768x600"]).is_err());
        let below_minimum = (MINIMUM_DISK_SIZE - 1).to_string();
        assert!(parse(["--disk-size", &below_minimum]).is_err());
        let minimum = MINIMUM_DISK_SIZE.to_string();
        assert!(parse(["--disk-size", &minimum]).is_ok());
        assert!(parse(["--interface-name", ""]).is_err());
        assert!(parse(["--unknown"]).is_err());
    }

    #[test]
    fn generated_help_and_version_are_information_requests() {
        for flag in ["--help", "-h", "--version", "-V"] {
            let error = parse([flag]).err().unwrap();
            assert!(error.is_help() || error.is_version());
            assert!(error.to_string().contains("native_example"));
        }
    }

    #[test]
    fn malformed_values_are_borrowed_parser_errors() {
        let value = String::from("not-a-resolution");
        let error = parse(["--resolution", value.as_str()]).err().unwrap();
        assert!(
            matches!(error, getargs_derive::Error::InvalidValue { value: actual, .. }
            if actual.as_ptr() == value.as_ptr())
        );
    }

    #[test]
    fn parser_relations_and_ranges_report_the_offending_options() {
        let error = parse(["--gui", "--terminal"]).err().unwrap();
        assert!(matches!(
            error,
            getargs_derive::Error::ArgumentConflict { .. }
        ));
        let error = parse(["--disk-size", "1"]).err().unwrap();
        assert!(matches!(
            error,
            getargs_derive::Error::ValueOutOfRange { value: "1", .. }
        ));
    }

    #[test]
    fn disk_size_error_reports_the_numeric_minimum() {
        let value = String::from("1");
        let error = parse(["--disk-size", value.as_str()]).err().unwrap();
        assert_eq!(
            error.to_string(),
            "invalid value '1' for '--disk-size <BYTES>': 1 is not in 1049600.."
        );
        assert!(
            matches!(error, getargs_derive::Error::ValueOutOfRange { value: actual, .. }
            if actual.as_ptr() == value.as_ptr())
        );
    }

    #[test]
    fn suggests_misspelled_resolution_option() {
        let error = parse(["--resoluton", "800x600"]).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("similar argument exists: '--resolution'")
        );
    }
}
