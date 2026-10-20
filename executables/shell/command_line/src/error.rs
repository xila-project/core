use core::fmt::Display;
use core::num::{NonZeroU16, NonZeroUsize};

use alloc::fmt;
use xila::{authentication, internationalization::translate, task};
use xila::{network, virtual_file_system};

pub type Result<T, E = Error<'static>> = core::result::Result<T, E>;

#[derive(Debug, Clone)]
#[repr(u16)]
pub enum Error<'a> {
    AuthenticationFailed(authentication::Error) = 1,
    FailedToSetTaskUser(task::Error),
    FailedToSetEnvironmentVariable(task::Error),
    FailedToSetCurrentDirectory(task::Error),
    FailedToRemoveEnvironmentVariable(task::Error),
    FailedToReadEnvironmentVariable(task::Error),
    FailedToTokenizeCommandLine,
    MissingFileNameAfterRedirectOut,
    MissingFileNameAfterRedirectIn,
    MissingCommand,
    CommandNotFound,
    FailedToGetTaskIdentifier,
    InvalidPath,
    FailedToGetPath,
    FailedToExecuteCommand,
    FailedToJoinTask,
    InvalidNumberOfArguments,
    FailedToJoinPath,
    FailedToCreateDirectory(virtual_file_system::Error),
    FailedToRemoveDirectory(virtual_file_system::Error),
    FailedToOpenDirectory(virtual_file_system::Error),
    FailedToOpenFile(virtual_file_system::Error),
    FailedToReadFile(virtual_file_system::Error),
    RequiresValue,
    DoesNotRequireValue,
    ArgumentParsing(getargs_derive::Error<'a>),
    InvalidArgument,
    MissingPositionalArgument(&'static str),
    InvalidOption,
    FailedToGetMetadata(virtual_file_system::Error),
    FailedToReadDirectoryEntry(virtual_file_system::Error),
    FailedToResolve(network::Error),
    FailedToCreateSocket(network::Error),
    Format,
    InvalidSleepDuration,
    FailedToSetPermissions(virtual_file_system::Error),
    FailedToGetTaskUser(task::Error),
    ExitStatus(NonZeroUsize),
    Syntax(crate::tokenizer::SyntaxError),
    FailedToWriteFile(virtual_file_system::Error),
    LineTooLong,
    FailedToRename(virtual_file_system::Error),
    FailedToCreateSymbolicLink(virtual_file_system::Error),
    FailedToReadSymbolicLink(virtual_file_system::Error),
}

impl<A: getargs::Argument> From<getargs::Error<A>> for Error<'_> {
    fn from(value: getargs::Error<A>) -> Self {
        match value {
            getargs::Error::RequiresValue(_) => Error::RequiresValue,
            getargs::Error::DoesNotRequireValue(_) => Error::DoesNotRequireValue,
            _ => Error::InvalidOption,
        }
    }
}

impl<'a> From<getargs_derive::Error<'a>> for Error<'a> {
    fn from(value: getargs_derive::Error<'a>) -> Self {
        Error::ArgumentParsing(value)
    }
}

impl Error<'_> {
    pub fn get_discriminant(&self) -> NonZeroU16 {
        unsafe { *<*const _>::from(self).cast::<NonZeroU16>() }
    }
}

impl From<fmt::Error> for Error<'_> {
    fn from(_: fmt::Error) -> Self {
        Error::Format
    }
}

impl Display for Error<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            Error::ExitStatus(status) => write!(formatter, "{status}"),
            Error::Syntax(error) => write!(
                formatter,
                translate!("Syntax error at byte {}: {}"),
                error.position, error.message
            ),
            Error::FailedToWriteFile(error) => {
                write!(formatter, translate!("Failed to write file: {}"), error)
            }
            Error::LineTooLong => write!(formatter, translate!("Line too long")),
            Error::FailedToRename(error) => {
                write!(formatter, translate!("Failed to rename path: {}"), error)
            }
            Error::FailedToCreateSymbolicLink(error) => {
                write!(
                    formatter,
                    translate!("Failed to create symbolic link: {}"),
                    error
                )
            }
            Error::FailedToReadSymbolicLink(error) => {
                write!(
                    formatter,
                    translate!("Failed to read symbolic link: {}"),
                    error
                )
            }
            Error::AuthenticationFailed(error) => {
                write!(formatter, translate!("Authentication failed: {}"), error)
            }
            Error::FailedToSetTaskUser(error) => {
                write!(formatter, translate!("Failed to set task user: {}"), error)
            }
            Error::FailedToSetEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("Failed to set environment variable: {}"),
                    error
                )
            }
            Error::FailedToTokenizeCommandLine => {
                write!(formatter, translate!("Failed to tokenize command line"))
            }
            Error::MissingFileNameAfterRedirectOut => {
                write!(
                    formatter,
                    translate!("Missing file name after redirect out")
                )
            }
            Error::MissingFileNameAfterRedirectIn => {
                write!(formatter, translate!("Missing file name after redirect in"))
            }
            Error::MissingCommand => write!(formatter, translate!("Missing command")),
            Error::CommandNotFound => write!(formatter, translate!("Command not found")),
            Error::FailedToGetTaskUser(error) => {
                write!(formatter, translate!("Failed to get task user: {}"), error)
            }
            Error::FailedToGetTaskIdentifier => {
                write!(formatter, translate!("Failed to get task identifier"))
            }
            Error::InvalidPath => write!(formatter, translate!("Invalid path")),
            Error::FailedToGetPath => {
                write!(formatter, translate!("Failed to get environment variable"))
            }
            Error::FailedToExecuteCommand => {
                write!(formatter, translate!("Failed to execute command"))
            }
            Error::FailedToJoinTask => {
                write!(formatter, translate!("Failed to join task"))
            }
            Error::InvalidNumberOfArguments => {
                write!(formatter, translate!("Invalid number of arguments"))
            }
            Error::FailedToRemoveEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("Failed to remove environment variable: {}"),
                    error
                )
            }
            Error::FailedToJoinPath => {
                write!(formatter, translate!("Failed to join path"))
            }
            Error::FailedToCreateDirectory(error) => {
                write!(
                    formatter,
                    translate!("Failed to create directory: {}"),
                    error
                )
            }
            Error::FailedToRemoveDirectory(error) => {
                write!(
                    formatter,
                    translate!("Failed to remove directory: {}"),
                    error
                )
            }
            Error::FailedToOpenDirectory(error) => {
                write!(formatter, translate!("Failed to open directory: {}"), error)
            }
            Error::FailedToOpenFile(error) => {
                write!(formatter, translate!("Failed to open file: {}"), error)
            }
            Error::FailedToReadFile(error) => {
                write!(formatter, translate!("Failed to read file: {}"), error)
            }
            Error::RequiresValue => {
                write!(formatter, translate!("Option requires a value"))
            }
            Error::DoesNotRequireValue => {
                write!(formatter, translate!("Option does not require a value"))
            }
            Error::InvalidArgument => {
                write!(formatter, translate!("Invalid argument"))
            }
            Error::MissingPositionalArgument(name) => {
                write!(
                    formatter,
                    translate!("Missing positional argument: {}"),
                    name
                )
            }
            Error::InvalidOption => {
                write!(formatter, translate!("Invalid option"))
            }
            Error::FailedToGetMetadata(error) => {
                write!(formatter, translate!("Failed to get metadata: {}"), error)
            }
            Error::FailedToSetCurrentDirectory(error) => {
                write!(
                    formatter,
                    translate!("Failed to set current directory: {}"),
                    error
                )
            }
            Error::FailedToReadDirectoryEntry(error) => {
                write!(
                    formatter,
                    translate!("Failed to read directory entry: {}"),
                    error
                )
            }
            Error::FailedToResolve(error) => {
                write!(formatter, translate!("Failed to resolve domain: {}"), error)
            }
            Error::FailedToCreateSocket(error) => {
                write!(formatter, translate!("Failed to create socket: {}"), error)
            }
            Error::ArgumentParsing(error) => {
                write!(formatter, translate!("Argument parsing error: {}"), error)
            }
            Error::InvalidSleepDuration => {
                write!(
                    formatter,
                    translate!(
                        "Invalid sleep duration: use a nonnegative integer with ms, s, m, or h (maximum 18446744073709551615 nanoseconds)"
                    )
                )
            }
            Error::FailedToSetPermissions(error) => {
                write!(
                    formatter,
                    translate!("Failed to set permissions: {}"),
                    error
                )
            }
            Error::Format => {
                write!(formatter, translate!("Format error"))
            }
            Error::FailedToReadEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("Failed to read environment variable: {}"),
                    error
                )
            }
        }
    }
}

impl From<Error<'_>> for NonZeroUsize {
    fn from(error: Error<'_>) -> Self {
        match error {
            Error::ExitStatus(status) => status,
            error => error.get_discriminant().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;
    use alloc::string::String;

    #[test]
    fn parser_error_keeps_the_borrowed_argument() {
        let input = String::from("not-a-number");
        let error: Error<'_> = getargs_derive::Error::InvalidValue {
            name: "--lines <LINES>",
            value: input.as_str(),
        }
        .into();
        let Error::ArgumentParsing(getargs_derive::Error::InvalidValue { value, .. }) = error
        else {
            panic!("parser error was not preserved");
        };
        assert_eq!(value.as_ptr(), input.as_ptr());
    }
}
