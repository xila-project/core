use crate::{
    Error, Result, Shell, ShellCommandContext,
    commands::{dispatch_user_command, resolve_user_command},
    parser,
    tokenizer::{Kind, Segment, Segments, SyntaxError, Token},
};
use alloc::{
    borrow::ToOwned,
    string::{String, ToString},
    vec::Vec,
};
use xila::{
    executable::Standard,
    file_system::{AccessFlags, CreateFlags, Flags, Path, StateFlags},
    virtual_file_system::File,
};

pub const MAX_EXPANDED_BYTES: usize = 8192;
const PIPE_BYTES: usize = 512;
const MAX_BACKGROUND_JOBS: usize = 8;

pub(crate) async fn write_all(file: &mut File<'_>, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        let count = file.write(bytes).await.map_err(Error::FailedToWriteFile)?;
        if count == 0 {
            return Err(Error::FailedToWriteFile(
                xila::virtual_file_system::Error::FailedToWrite,
            ));
        }
        bytes = &bytes[count..];
    }
    Ok(())
}
struct PreparedCommand {
    arguments: Vec<String>,
    standard: Standard<'static>,
}

impl Shell {
    /// Execute shell source. Pre-separated executable arguments use a different, literal path.
    pub async fn execute_line(&mut self, source: &str) -> Result<usize> {
        let tokens = parser::parse(source).map_err(Error::Syntax)?;
        let mut offset = 0;
        for job in
            tokens.split_inclusive(|token| matches!(token.kind, Kind::Separator | Kind::Background))
        {
            if !self.running {
                break;
            }
            let delimiter = job
                .last()
                .filter(|token| matches!(token.kind, Kind::Separator | Kind::Background));
            let end = delimiter.map_or(source.len(), |token| token.position);
            let body = if delimiter.is_some() {
                &job[..job.len() - 1]
            } else {
                job
            };
            if delimiter.is_some_and(|token| token.kind == Kind::Background) {
                self.background_jobs.retain(|job| !job.is_finished());
                if self.background_jobs.len() == MAX_BACKGROUND_JOBS {
                    self.last_status = 1;
                    self.diagnostic(xila::internationalization::translate!(
                        "errors.too_many_background_jobs"
                    ))
                    .await;
                } else {
                    let source = source[offset..end].to_owned();
                    match self.launch_background(source).await {
                        Ok(job) => {
                            self.background_jobs.push(job);
                            self.last_status = 0;
                        }
                        Err(error) => {
                            self.last_status = 1;
                            self.diagnostic(error).await;
                        }
                    }
                }
            } else {
                self.run_and_or(body).await;
            }
            offset = delimiter.map_or(source.len(), |token| token.position + 1);
        }
        Ok(self.last_status)
    }

    async fn launch_background(&self, source: String) -> Result<xila::task::JoinHandle<usize>> {
        let mut standard = self
            .standard
            .duplicate()
            .await
            .map_err(Error::FailedToOpenFile)?;
        standard.standard_in = File::open(
            self.context.virtual_file_system,
            self.task,
            "/devices/standard_io/null",
            AccessFlags::Read.into(),
        )
        .await
        .map_err(Error::FailedToOpenFile)?;
        let directory = self.current_directory.clone();
        let previous = self.last_status;
        self.context
            .task_manager
            .spawn(
                self.task,
                "Shell background command",
                None,
                move |_| async move {
                    let mut child = Shell::new(standard).await;
                    child.current_directory = directory;
                    child.last_status = previous;
                    match parser::parse(&source) {
                        Ok(tokens) => child.run_and_or(&tokens).await,
                        Err(error) => child.diagnostic(error.message).await,
                    }
                    child.last_status
                },
            )
            .await
            .map(|(handle, _)| handle)
            .map_err(|_| Error::FailedToExecuteCommand)
    }

    async fn run_and_or(&mut self, tokens: &[Token<'_>]) {
        let mut condition = None;
        let mut start = 0;
        for end in 0..=tokens.len() {
            if end != tokens.len() && !matches!(tokens[end].kind, Kind::And | Kind::Or) {
                continue;
            }
            let should_run = match condition {
                Some(Kind::And) => self.last_status == 0,
                Some(Kind::Or) => self.last_status != 0,
                _ => true,
            };
            if should_run && self.running {
                match self.run_pipeline(&tokens[start..end]).await {
                    Ok(status) => self.last_status = status,
                    Err(error) => {
                        self.last_status = 1;
                        self.diagnostic(error).await;
                    }
                }
            }
            condition = tokens.get(end).map(|token| token.kind);
            start = end + 1;
        }
    }

    async fn run_pipeline(&mut self, tokens: &[Token<'_>]) -> Result<usize> {
        if tokens.is_empty() {
            return Ok(self.last_status);
        }
        if !tokens.iter().any(|token| token.kind == Kind::Pipe) {
            let standard = self
                .standard
                .duplicate()
                .await
                .map_err(Error::FailedToOpenFile)?;
            let prepared = self.prepare(tokens, standard).await?;
            return Ok(self.run_prepared(prepared).await);
        }
        let stage_count = tokens
            .iter()
            .filter(|token| token.kind == Kind::Pipe)
            .count()
            + 1;
        let mut prepared = Vec::with_capacity(stage_count);
        let mut input = None;
        for (index, stage) in tokens.split(|token| token.kind == Kind::Pipe).enumerate() {
            let mut standard = self
                .standard
                .duplicate()
                .await
                .map_err(Error::FailedToOpenFile)?;
            if let Some(reader) = input.take() {
                standard.standard_in = reader;
            }
            if index + 1 < stage_count {
                let (reader, writer) = File::create_unnamed_pipe(
                    self.context.virtual_file_system,
                    PIPE_BYTES,
                    StateFlags::from_bits_truncate(0),
                )
                .await
                .map_err(Error::FailedToOpenFile)?;
                standard.standard_out = writer;
                input = Some(reader);
            }
            prepared.push(self.prepare(stage, standard).await?);
        }
        let mut handles = Vec::with_capacity(stage_count);
        let mut failure = None;
        for command in prepared {
            let directory = self.current_directory.clone();
            match self
                .context
                .task_manager
                .spawn(
                    self.task,
                    "Shell pipeline stage",
                    None,
                    move |_| async move {
                        let mut child = Shell::new(command.standard).await;
                        child.current_directory = directory;
                        let mut status = child.run_arguments(&command.arguments).await;
                        if child.standard.out().flush().await.is_err()
                            || child.standard.error().flush().await.is_err()
                        {
                            status = 1;
                        }
                        status
                    },
                )
                .await
            {
                Ok((handle, _)) => handles.push(handle),
                Err(_) => {
                    failure = Some(Error::FailedToExecuteCommand);
                    break;
                }
            }
        }
        let mut status = 0;
        for handle in handles {
            status = handle.join().await;
        }
        if let Some(error) = failure {
            Err(error)
        } else {
            Ok(status)
        }
    }

    async fn expand(&self, raw: &str, used: &mut usize) -> Result<String> {
        let mut value = String::new();
        for part in Segments::new(raw) {
            let variable;
            let status;
            let text = match part.map_err(Error::Syntax)? {
                Segment::Literal(value) => value,
                Segment::Variable(name) => {
                    variable = self
                        .context
                        .task_manager
                        .get_environment_variable(self.task, name)
                        .await
                        .ok();
                    variable
                        .as_ref()
                        .map_or("", |variable| variable.get_value())
                }
                Segment::Status => {
                    status = self.last_status.to_string();
                    &status
                }
            };
            if *used + text.len() > MAX_EXPANDED_BYTES {
                return Err(Error::Syntax(SyntaxError::new(
                    0,
                    "expanded command is too long",
                )));
            }
            *used += text.len();
            value.push_str(text);
        }
        Ok(value)
    }

    async fn prepare(
        &self,
        tokens: &[Token<'_>],
        mut standard: Standard<'static>,
    ) -> Result<PreparedCommand> {
        let mut arguments = Vec::new();
        let mut used = 0;
        let mut iterator = tokens.iter();
        while let Some(token) = iterator.next() {
            if let Kind::Word(raw) = token.kind {
                arguments.push(self.expand(raw, &mut used).await?);
                continue;
            }
            let Some(Token {
                kind: Kind::Word(raw),
                ..
            }) = iterator.next()
            else {
                return Err(Error::MissingFileNameAfterRedirectOut);
            };
            let path = self.expand(raw, &mut used).await?;
            if path.is_empty() {
                return Err(Error::InvalidPath);
            }
            let path = if Path::from_str(&path).is_absolute() {
                Path::from_str(&path).to_owned()
            } else {
                self.current_directory
                    .clone()
                    .join(Path::from_str(&path))
                    .ok_or(Error::InvalidPath)?
            };
            let flags = match token.kind {
                Kind::Input => AccessFlags::Read.into(),
                Kind::Append | Kind::ErrorAppend => Flags::new(
                    AccessFlags::Write,
                    Some(CreateFlags::Create),
                    Some(StateFlags::Append),
                ),
                _ => Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
            };
            let file = File::open(self.context.virtual_file_system, self.task, &path, flags)
                .await
                .map_err(Error::FailedToOpenFile)?;
            match token.kind {
                Kind::Input => {
                    standard.standard_in = file;
                }
                Kind::ErrorOutput | Kind::ErrorAppend => {
                    standard.standard_error = file;
                }
                _ => {
                    standard.standard_out = file;
                }
            }
        }
        Ok(PreparedCommand {
            arguments,
            standard,
        })
    }

    async fn run_prepared(&mut self, mut command: PreparedCommand) -> usize {
        core::mem::swap(&mut self.standard, &mut command.standard);
        let mut status = self.run_arguments(&command.arguments).await;
        if self.standard.out().flush().await.is_err()
            || self.standard.error().flush().await.is_err()
        {
            status = 1;
        }
        core::mem::swap(&mut self.standard, &mut command.standard);
        status
    }

    pub(crate) async fn run_arguments(&mut self, arguments: &[String]) -> usize {
        let Some(name) = arguments.first() else {
            return 0;
        };
        let paths = self
            .context
            .task_manager
            .get_environment_variable(self.task, "Paths")
            .await;
        let paths = match paths {
            Ok(paths) => paths,
            Err(_) => {
                self.diagnostic(Error::FailedToGetPath).await;
                return 1;
            }
        };
        let paths: Vec<&Path> = paths.get_value().split(':').map(Path::from_str).collect();
        let result = if let Some(command) = resolve_user_command(name) {
            let mut options = getargs::Options::new(arguments[1..].iter().map(String::as_str));
            let mut context = ShellCommandContext::new(self);
            dispatch_user_command(command, &mut context, &mut options, &paths)
                .await
                .map(|()| 0)
        } else {
            self.execute(arguments.iter().map(String::as_str), &paths)
                .await
        };
        match result {
            Ok(status) => status,
            Err(Error::ExitStatus(status)) => status.get(),
            Err(error) => {
                let status = if matches!(error, Error::CommandNotFound) {
                    127
                } else {
                    1
                };
                self.diagnostic(error).await;
                status
            }
        }
    }
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
