use crate::{Error, Result};
use alloc::{borrow::ToOwned, vec::Vec};
use getargs_derive::GetArgs;
use xila::{
    file_system::{AccessFlags, Path},
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand};

pub struct HeadCommand;

impl UserCommand for HeadCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        _paths: &[&Path],
    ) -> Result<(), crate::Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        execute_head(context, options).await
    }
}

#[derive(GetArgs)]
#[command(name = "head", about = "Print the first lines of a file.")]
pub(super) struct HeadArguments<'a> {
    path: &'a str,
    #[arg(short = 'n', long = "lines", default_value_t = 10)]
    lines: usize,
}

fn resolve_path<C: CommandContext>(
    context: &C,
    path: &str,
) -> Result<xila::file_system::PathOwned> {
    let path = Path::from_str(path);

    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        context
            .current_directory_owned()
            .join(path)
            .ok_or(Error::FailedToJoinPath)
    }
}

async fn read_head_bytes(mut file: File<'_>, lines_to_keep: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    if lines_to_keep == 0 {
        return Ok(output);
    }

    let mut remaining_lines = lines_to_keep;
    let mut buffer = [0u8; 256];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(Error::FailedToReadFile)?;

        if bytes_read == 0 {
            break;
        }

        for &byte in &buffer[..bytes_read] {
            output.push(byte);

            if byte == b'\n' {
                remaining_lines -= 1;
                if remaining_lines == 0 {
                    return Ok(output);
                }
            }
        }
    }

    Ok(output)
}

async fn execute_head<'a, I, C>(
    context: &mut C,
    options: &mut getargs::Options<&'a str, I>,
) -> Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let HeadArguments { path, lines } = HeadArguments::parse(options)?;
    let path = resolve_path(context, path)?;

    let file = File::open(
        context.executable_context().virtual_file_system,
        context.task_id(),
        &path,
        AccessFlags::Read.into(),
    )
    .await
    .map_err(Error::FailedToOpenFile)?;

    let output = read_head_bytes(file, lines).await?;
    context.write_out(&output).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use getargs::Options;

    use super::HeadArguments;

    fn select_head_bytes(input: &[u8], lines_to_keep: usize) -> Vec<u8> {
        let mut output = Vec::new();
        if lines_to_keep == 0 {
            return output;
        }

        let mut remaining_lines = lines_to_keep;
        for &byte in input {
            output.push(byte);
            if byte == b'\n' {
                remaining_lines -= 1;
                if remaining_lines == 0 {
                    break;
                }
            }
        }

        output
    }

    #[test]
    fn keeps_first_two_lines_with_trailing_newline() {
        let input = b"line1\nline2\nline3\n";
        assert_eq!(select_head_bytes(input, 2), b"line1\nline2\n");
    }

    #[test]
    fn keeps_all_when_fewer_than_requested() {
        let input = b"line1\nline2\n";
        assert_eq!(select_head_bytes(input, 10), input);
    }

    #[test]
    fn handles_file_without_trailing_newline() {
        let input = b"line1\nline2";
        assert_eq!(select_head_bytes(input, 1), b"line1\n");
        assert_eq!(select_head_bytes(input, 2), input);
    }

    #[test]
    fn keeps_nothing_when_zero_lines_requested() {
        let input = b"line1\nline2\n";
        assert_eq!(select_head_bytes(input, 0), Vec::<u8>::new());
    }

    #[test]
    fn parses_option_value_before_positional_path() {
        let args = ["-n", "2", "notes.txt"];
        let mut options = Options::new(args.into_iter());

        let parsed = HeadArguments::parse(&mut options);

        assert!(parsed.is_ok());
        let parsed = parsed.unwrap();
        assert_eq!(parsed.lines, 2);
        assert_eq!(parsed.path, "notes.txt");
    }

    #[test]
    fn preserves_default_and_zero_line_counts() {
        let parsed = HeadArguments::from_args(["notes.txt"].into_iter()).unwrap();
        assert_eq!(parsed.lines, 10);
        let parsed = HeadArguments::from_args(["notes.txt", "--lines", "0"].into_iter()).unwrap();
        assert_eq!(parsed.lines, 0);
    }

    #[test]
    fn accepts_hyphenated_path_after_end_of_options() {
        let parsed = HeadArguments::from_args(["--", "-notes.txt"].into_iter()).unwrap();
        assert_eq!(parsed.path, "-notes.txt");
    }

    #[test]
    fn help_names_the_builtin_command() {
        let error = HeadArguments::from_args(["--help"].into_iter())
            .err()
            .unwrap();
        assert!(error.is_help());
        assert!(alloc::string::ToString::to_string(&error).contains("Usage: head"));
    }
}
