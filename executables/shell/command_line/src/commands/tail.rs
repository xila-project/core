use crate::{Error, Result};
use alloc::{borrow::ToOwned, vec::Vec};
use getargs_derive::GetArgs;
use xila::{
    file_system::{AccessFlags, Path},
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand};

pub struct TailCommand;

impl UserCommand for TailCommand {
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
        execute_tail(context, options).await
    }
}

#[derive(GetArgs)]
#[command(name = "tail", about = "Print the last lines of a file.")]
pub(super) struct TailArguments<'a> {
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

fn trim_to_last_lines(buffer: &mut Vec<u8>, lines_to_keep: usize) {
    if lines_to_keep == 0 {
        buffer.clear();
        return;
    }

    let trailing_newline = buffer.last().copied() == Some(b'\n');
    let newline_count = buffer.iter().filter(|&&byte| byte == b'\n').count();
    let line_count = newline_count + usize::from(!buffer.is_empty() && !trailing_newline);

    if line_count <= lines_to_keep {
        return;
    }

    let separators_to_skip = line_count - lines_to_keep;

    let mut skipped = 0;
    let mut start_index = 0;

    for (index, &byte) in buffer.iter().enumerate() {
        if byte == b'\n' {
            skipped += 1;
            if skipped == separators_to_skip {
                start_index = index + 1;
                break;
            }
        }
    }

    buffer.drain(..start_index);
}

async fn read_tail_bytes(mut file: File<'_>, lines_to_keep: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    if lines_to_keep == 0 {
        return Ok(output);
    }

    let mut buffer = [0u8; 256];
    loop {
        let bytes_read = file
            .read(&mut buffer)
            .await
            .map_err(Error::FailedToReadFile)?;

        if bytes_read == 0 {
            break;
        }

        output.extend_from_slice(&buffer[..bytes_read]);
        trim_to_last_lines(&mut output, lines_to_keep);
    }

    Ok(output)
}

async fn execute_tail<'a, I, C>(
    context: &mut C,
    options: &mut getargs::Options<&'a str, I>,
) -> Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let TailArguments { path, lines } = TailArguments::parse(options)?;
    let path = resolve_path(context, path)?;

    let file = File::open(
        context.executable_context().virtual_file_system,
        context.task_id(),
        &path,
        AccessFlags::Read.into(),
    )
    .await
    .map_err(Error::FailedToOpenFile)?;

    let output = read_tail_bytes(file, lines).await?;
    context.write_out(&output).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::trim_to_last_lines;
    use alloc::vec::Vec;

    fn select_tail_bytes(input: &[u8], lines_to_keep: usize) -> Vec<u8> {
        let mut output = input.to_vec();
        trim_to_last_lines(&mut output, lines_to_keep);
        output
    }

    #[test]
    fn keeps_last_two_lines_with_trailing_newline() {
        let input = b"line1\nline2\nline3\n";
        assert_eq!(select_tail_bytes(input, 2), b"line2\nline3\n");
    }

    #[test]
    fn keeps_all_when_fewer_than_requested() {
        let input = b"line1\nline2\n";
        assert_eq!(select_tail_bytes(input, 10), input);
    }

    #[test]
    fn handles_file_without_trailing_newline() {
        let input = b"line1\nline2\nline3";
        assert_eq!(select_tail_bytes(input, 2), b"line2\nline3");
    }

    #[test]
    fn keeps_nothing_when_zero_lines_requested() {
        let input = b"line1\nline2\n";
        assert_eq!(select_tail_bytes(input, 0), Vec::<u8>::new());
    }
}
