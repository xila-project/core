use crate::error::Result;
use alloc::{vec, vec::Vec};
use getargs::{Arg, Options};
use xila::{
    file_system::{AccessFlags, Path},
    shared::{HttpRequestBuilder, Url},
    virtual_file_system::File,
};

use super::{CommandContext, UserCommand};

fn select_client_device_path(url: &Url<'_>) -> Option<&'static str> {
    match url.scheme {
        "http" => Some("/devices/network/http_client"),
        "https" => Some("/devices/network/https_client"),
        _ => None,
    }
}

pub struct WebRequestCommand;

impl UserCommand for WebRequestCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut Options<&'a str, I>,
        _paths: &[&Path],
    ) -> Result<(), crate::Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        execute_web_request(context, options).await
    }
}

struct WebRequestParameters<'a> {
    headers: Vec<&'a str>,
    method: &'a str,
    url: &'a str,
    body: Option<&'a str>,
    body_file: Option<&'a Path>,
}

fn parse_web_request_parameters<'a, I>(
    options: &mut Options<&'a str, I>,
) -> Result<WebRequestParameters<'a>>
where
    I: Iterator<Item = &'a str>,
{
    let mut headers: Vec<&'a str> = Vec::with_capacity(20);
    let mut method: &'a str = "GET";
    let mut url: Option<&'a str> = None;
    let mut body: Option<&'a str> = None;
    let mut body_file: Option<&'a Path> = None;

    while let Some(argument) = options.next_arg()? {
        match argument {
            Arg::Long("header") | Arg::Short('h') => {
                headers.push(options.value()?);
            }
            Arg::Long("method") | Arg::Short('m') => {
                method = options.value()?;
            }
            Arg::Long("body") | Arg::Short('b') => {
                let value = options.value()?;
                if body.replace(value).is_some() {
                    xila::log::error!("Body already set");
                    return Err(crate::error::Error::InvalidArgument);
                }
            }
            Arg::Long("body-file") | Arg::Short('f') => {
                let value = options.value()?;
                let path = Path::from_str(value);
                if body_file.replace(path).is_some() {
                    xila::log::error!("Body file already set");
                    return Err(crate::error::Error::InvalidArgument);
                }
            }
            Arg::Positional(positional) => {
                if url.replace(positional).is_some() {
                    xila::log::error!("URL already set");
                    return Err(crate::error::Error::InvalidArgument);
                }
            }
            e => {
                xila::log::error!("Unexpected argument: {:?}", e);
                return Err(crate::error::Error::InvalidArgument);
            }
        }
    }

    Ok(WebRequestParameters {
        headers,
        method,
        url: url.ok_or(crate::error::Error::MissingPositionalArgument("url"))?,
        body,
        body_file,
    })
}

async fn append_body<'a>(
    request_builder: &mut HttpRequestBuilder<'_>,
    parameters: &WebRequestParameters<'a>,
    virtual_file_system: &xila::virtual_file_system::VirtualFileSystem,
    task: xila::task::TaskIdentifier,
) -> Result<()> {
    if let Some(body) = parameters.body {
        request_builder
            .add_body(body.as_bytes())
            .ok_or(crate::error::Error::InvalidArgument)?;
    } else if let Some(body_file) = parameters.body_file {
        let mut buffer: Vec<u8> = Vec::with_capacity(4096);

        File::read_from_path(virtual_file_system, task, body_file, &mut buffer)
            .await
            .map_err(crate::error::Error::FailedToOpenFile)?;

        request_builder
            .add_body(&buffer)
            .ok_or(crate::error::Error::InvalidArgument)?;
    }

    Ok(())
}

async fn build_request_buffer<'a>(
    parameters: &WebRequestParameters<'a>,
    virtual_file_system: &xila::virtual_file_system::VirtualFileSystem,
    task: xila::task::TaskIdentifier,
) -> Result<Vec<u8>> {
    let mut buffer: Vec<u8> = vec![0; 4096];

    let url = Url::parse(parameters.url).ok_or(crate::error::Error::InvalidArgument)?;

    let mut request_builder = HttpRequestBuilder::from_buffer(&mut buffer);

    request_builder
        .add_request(
            parameters.method,
            url.path,
            HttpRequestBuilder::HTTP_VERSION_1_1,
        )
        .ok_or(crate::error::Error::InvalidArgument)?
        .add_header("Host", url.host.as_bytes())
        .ok_or(crate::error::Error::InvalidArgument)?
        .add_header("Connection", b"close")
        .ok_or(crate::error::Error::InvalidArgument)?;

    for header in &parameters.headers {
        let (name, value) = header
            .split_once(':')
            .ok_or(crate::error::Error::InvalidArgument)?;
        request_builder
            .add_header(name.trim(), value.trim().as_bytes())
            .ok_or(crate::error::Error::InvalidArgument)?;
    }

    append_body(&mut request_builder, parameters, virtual_file_system, task).await?;

    Ok(buffer)
}

async fn execute_web_request<'a, I, C>(
    context: &mut C,
    options: &mut Options<&'a str, I>,
) -> Result<()>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let parameters = parse_web_request_parameters(options)?;
    let virtual_file_system = &context.executable_context().virtual_file_system;
    let task = context.task_id();

    let mut buffer = build_request_buffer(&parameters, virtual_file_system, task).await?;

    let mut file = File::open(
        virtual_file_system,
        task,
        select_client_device_path(
            &Url::parse(parameters.url).ok_or(crate::error::Error::InvalidArgument)?,
        )
        .ok_or(crate::error::Error::InvalidArgument)?,
        AccessFlags::READ_WRITE.into(),
    )
    .await
    .map_err(crate::error::Error::FailedToOpenFile)?;

    file.write(&buffer)
        .await
        .map_err(crate::error::Error::FailedToOpenFile)?;

    buffer.fill(0);
    let header_bytes = file
        .read(&mut buffer)
        .await
        .map_err(crate::error::Error::FailedToOpenFile)?;
    context.write_out(&buffer[..header_bytes]).await?;

    buffer.fill(0);
    let body_bytes = file
        .read(&mut buffer)
        .await
        .map_err(crate::error::Error::FailedToOpenFile)?;
    context
        .write_out_line(&buffer[..body_bytes.min(128)])
        .await?;

    file.close(virtual_file_system)
        .await
        .map_err(crate::error::Error::FailedToOpenFile)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_web_request_parameters, select_client_device_path};
    use getargs::Options;
    use xila::shared::Url;

    #[test]
    fn parse_web_request_parameters_uses_get_and_url_defaults() {
        let input = ["https://example.com"];
        let mut options = Options::new(input.into_iter());

        let parameters = parse_web_request_parameters(&mut options).unwrap();

        assert_eq!(parameters.method, "GET");
        assert_eq!(parameters.url, "https://example.com");
        assert!(parameters.headers.is_empty());
        assert!(parameters.body.is_none());
        assert!(parameters.body_file.is_none());
    }

    #[test]
    fn parse_web_request_parameters_parses_method_headers_and_body() {
        let input = [
            "-m",
            "POST",
            "-h",
            "Content-Type:application/json",
            "-b",
            "{\"a\":1}",
            "https://example.com/api",
        ];
        let mut options = Options::new(input.into_iter());

        let parameters = parse_web_request_parameters(&mut options).unwrap();

        assert_eq!(parameters.method, "POST");
        assert_eq!(parameters.url, "https://example.com/api");
        assert_eq!(parameters.headers, ["Content-Type:application/json"]);
        assert_eq!(parameters.body, Some("{\"a\":1}"));
        assert!(parameters.body_file.is_none());
    }

    #[test]
    fn parse_web_request_parameters_rejects_duplicate_body() {
        let input = ["-b", "first", "-b", "second", "https://example.com"];
        let mut options = Options::new(input.into_iter());

        let result = parse_web_request_parameters(&mut options);

        assert!(matches!(result, Err(crate::error::Error::InvalidArgument)));
    }

    #[test]
    fn parse_web_request_parameters_rejects_duplicate_url() {
        let input = ["https://example.com/1", "https://example.com/2"];
        let mut options = Options::new(input.into_iter());

        let result = parse_web_request_parameters(&mut options);

        assert!(matches!(result, Err(crate::error::Error::InvalidArgument)));
    }

    #[test]
    fn parse_web_request_parameters_requires_url() {
        let input: [&str; 0] = [];
        let mut options = Options::new(input.into_iter());

        let result = parse_web_request_parameters(&mut options);

        assert!(matches!(
            result,
            Err(crate::error::Error::MissingPositionalArgument("url"))
        ));
    }

    #[test]
    fn web_request_uses_https_device_for_https_scheme() {
        let url = Url::parse("https://example.com").unwrap();
        assert_eq!(
            select_client_device_path(&url),
            Some("/devices/network/https_client")
        );
    }

    #[test]
    fn web_request_uses_http_device_for_http_scheme() {
        let url = Url::parse("http://example.com").unwrap();
        assert_eq!(
            select_client_device_path(&url),
            Some("/devices/network/http_client")
        );
    }
}
