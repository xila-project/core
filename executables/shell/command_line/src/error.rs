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
                translate!("errors.syntax_error_at_byte"),
                error.position, error.message
            ),
            Error::FailedToWriteFile(error) => {
                write!(formatter, translate!("errors.failed_to_write_file"), error)
            }
            Error::LineTooLong => write!(formatter, translate!("errors.line_too_long")),
            Error::FailedToRename(error) => {
                write!(formatter, translate!("errors.failed_to_rename_path"), error)
            }
            Error::FailedToCreateSymbolicLink(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_create_symbolic_link"),
                    error
                )
            }
            Error::FailedToReadSymbolicLink(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_read_symbolic_link"),
                    error
                )
            }
            Error::AuthenticationFailed(error) => {
                write!(formatter, translate!("errors.authentication_failed"), error)
            }
            Error::FailedToSetTaskUser(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_task_user"),
                    error
                )
            }
            Error::FailedToSetEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_environment_variable"),
                    error
                )
            }
            Error::FailedToTokenizeCommandLine => {
                write!(
                    formatter,
                    translate!("errors.failed_to_tokenize_command_line")
                )
            }
            Error::MissingFileNameAfterRedirectOut => {
                write!(
                    formatter,
                    translate!("errors.missing_file_name_after_redirect_out")
                )
            }
            Error::MissingFileNameAfterRedirectIn => {
                write!(
                    formatter,
                    translate!("errors.missing_file_name_after_redirect_in")
                )
            }
            Error::MissingCommand => {
                write!(formatter, translate!("execution.errors.missing_command"))
            }
            Error::CommandNotFound => {
                write!(formatter, translate!("execution.errors.command_not_found"))
            }
            Error::FailedToGetTaskUser(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_task_user"),
                    error
                )
            }
            Error::FailedToGetTaskIdentifier => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_task_identifier")
                )
            }
            Error::InvalidPath => write!(formatter, translate!("errors.invalid_path")),
            Error::FailedToGetPath => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_environment_variable")
                )
            }
            Error::FailedToExecuteCommand => {
                write!(formatter, translate!("errors.failed_to_execute_command"))
            }
            Error::FailedToJoinTask => {
                write!(formatter, translate!("errors.failed_to_join_task"))
            }
            Error::InvalidNumberOfArguments => {
                write!(formatter, translate!("errors.invalid_number_of_arguments"))
            }
            Error::FailedToRemoveEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_remove_environment_variable"),
                    error
                )
            }
            Error::FailedToJoinPath => {
                write!(formatter, translate!("errors.failed_to_join_path"))
            }
            Error::FailedToCreateDirectory(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_create_directory"),
                    error
                )
            }
            Error::FailedToRemoveDirectory(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_remove_directory"),
                    error
                )
            }
            Error::FailedToOpenDirectory(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_open_directory"),
                    error
                )
            }
            Error::FailedToOpenFile(error) => {
                write!(formatter, translate!("errors.failed_to_open_file"), error)
            }
            Error::FailedToReadFile(error) => {
                write!(formatter, translate!("errors.failed_to_read_file"), error)
            }
            Error::RequiresValue => {
                write!(formatter, translate!("errors.option_requires_a_value"))
            }
            Error::DoesNotRequireValue => {
                write!(
                    formatter,
                    translate!("errors.option_does_not_require_a_value")
                )
            }
            Error::InvalidArgument => {
                write!(formatter, translate!("errors.invalid_argument"))
            }
            Error::MissingPositionalArgument(name) => {
                write!(
                    formatter,
                    translate!("errors.missing_positional_argument"),
                    name
                )
            }
            Error::InvalidOption => {
                write!(formatter, translate!("errors.invalid_option"))
            }
            Error::FailedToGetMetadata(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_get_metadata"),
                    error
                )
            }
            Error::FailedToSetCurrentDirectory(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_current_directory"),
                    error
                )
            }
            Error::FailedToReadDirectoryEntry(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_read_directory_entry"),
                    error
                )
            }
            Error::FailedToResolve(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_resolve_domain"),
                    error
                )
            }
            Error::FailedToCreateSocket(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_create_socket"),
                    error
                )
            }
            Error::ArgumentParsing(error) => {
                write!(
                    formatter,
                    translate!("errors.argument_parsing_error"),
                    error
                )
            }
            Error::InvalidSleepDuration => {
                write!(formatter, translate!("sleep.errors.invalid_duration"))
            }
            Error::FailedToSetPermissions(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_set_permissions"),
                    error
                )
            }
            Error::Format => {
                write!(formatter, translate!("errors.format_failed"))
            }
            Error::FailedToReadEnvironmentVariable(error) => {
                write!(
                    formatter,
                    translate!("errors.failed_to_read_environment_variable"),
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
