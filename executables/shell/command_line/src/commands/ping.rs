use getargs_derive::GetArgs;
use xila::{
    file_system::Path,
    internationalization::translate,
    network::{self, DnsQueryKind, Duration, IcmpEndpoint},
};

use crate::Error;

use super::{CommandContext, UserCommand};

pub struct PingCommand;

impl UserCommand for PingCommand {
    async fn execute<'a, I, C>(
        &self,
        context: &mut C,
        options: &mut getargs::Options<&'a str, I>,
        _paths: &[&Path],
    ) -> crate::Result<(), crate::Error<'a>>
    where
        I: Iterator<Item = &'a str>,
        C: CommandContext,
    {
        execute_ping(context, options).await
    }
}

const ICMP_IDENTIFIER: u16 = 0x22b;

#[derive(GetArgs)]
#[command(name = "ping", about = "Send ICMP echo requests.")]
pub(super) struct PingArguments<'a> {
    target: &'a str,
    #[arg(short = 'c', long = "count", default_value_t = 4)]
    count: u16,
    #[arg(short = 't', long = "timeout", default_value_t = 5)]
    timeout: u64,
    #[arg(short = 's', long = "size", default_value_t = 56)]
    size: usize,
}

async fn write_ping_line<C: CommandContext>(
    context: &mut C,
    target: &str,
    resolved_target: &xila::network::IpAddress,
    payload_size: usize,
) -> crate::Result<()> {
    let unit = xila::internationalization::translate_plural!(
        "ping.request.payload_byte_unit",
        payload_size
    );
    context
        .write_out_fmt(format_args!(
            "PING {} ({}): {} {}\n",
            target, resolved_target, payload_size, unit
        ))
        .await
}

async fn ping_loop<C: CommandContext>(
    context: &mut C,
    network_manager: &network::Manager,
    resolved_target: &xila::network::IpAddress,
    count: u16,
    timeout_seconds: u64,
    payload_size: usize,
) -> crate::Result<()> {
    let socket = network_manager
        .new_icmp_socket(256, 256, 1, 1, None)
        .await
        .map_err(Error::FailedToCreateSocket)?;

    socket
        .bind(IcmpEndpoint::Identifier(ICMP_IDENTIFIER))
        .await
        .map_err(Error::FailedToCreateSocket)?;

    for i in 0..count {
        match socket
            .ping(
                resolved_target,
                i,
                ICMP_IDENTIFIER,
                Duration::from_seconds(timeout_seconds),
                payload_size,
            )
            .await
        {
            Ok(duration) => {
                let unit = xila::internationalization::translate_plural!(
                    "ping.reply.byte_unit",
                    payload_size
                );
                context
                    .write_out_fmt(format_args!(
                        "{} {} {}: icmp_seq={} {}={:.2} ms\n",
                        payload_size,
                        unit,
                        resolved_target,
                        i,
                        translate!("ping.reply.time_label"),
                        duration.as_milliseconds()
                    ))
                    .await?;
            }
            Err(network::Error::TimedOut) => {
                context
                    .write_out_fmt(format_args!(
                        "{}\n",
                        format_args!(translate!("ping.errors.request_timeout"), i)
                    ))
                    .await?;
            }
            Err(e) => {
                context
                    .write_out_fmt(format_args!(
                        "{}\n",
                        format_args!(translate!("errors.with_details"), e)
                    ))
                    .await?;
            }
        }
    }

    Ok(())
}

async fn execute_ping<'a, I, C>(
    context: &mut C,
    options: &mut getargs::Options<&'a str, I>,
) -> crate::Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let PingArguments {
        target,
        count,
        timeout: timeout_seconds,
        size: payload_size,
    } = PingArguments::parse(options)?;

    let manager = context
        .executable_context()
        .network_manager
        .as_ref()
        .ok_or(crate::Error::InvalidOption)?;

    let resolved_target = manager
        .resolve(target, DnsQueryKind::A | DnsQueryKind::Aaaa, true, None)
        .await
        .ok()
        .and_then(|ips| ips.first().cloned());

    let resolved_target = match resolved_target {
        Some(ip) => ip,
        None => {
            context
                .write_out_fmt(format_args!(
                    "{}\n",
                    format_args!(translate!("dns.errors.unknown_host"), target)
                ))
                .await?;
            return Ok(());
        }
    };

    write_ping_line(context, target, &resolved_target, payload_size).await?;
    ping_loop(
        context,
        manager,
        &resolved_target,
        count,
        timeout_seconds,
        payload_size,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::PingArguments;
    use alloc::string::ToString;

    #[test]
    fn preserves_defaults_and_option_names() {
        let parsed = PingArguments::from_args(["localhost"].into_iter()).unwrap();
        assert_eq!((parsed.count, parsed.timeout, parsed.size), (4, 5, 56));
        let parsed = PingArguments::from_args(
            ["-c", "2", "--timeout", "3", "--size=32", "localhost"].into_iter(),
        )
        .unwrap();
        assert_eq!((parsed.count, parsed.timeout, parsed.size), (2, 3, 32));
    }

    #[test]
    fn ping_byte_counts_select_singular_and_plural() {
        let (data_one, data_many, reply_one, reply_many) =
            match xila::internationalization::get_locale() {
                "fr" => (
                    "octet de données",
                    "octets de données",
                    "octet de",
                    "octets de",
                ),
                "zh-CN" => ("字节数据", "字节数据", "字节，来自", "字节，来自"),
                "ja" => (
                    "データバイト",
                    "データバイト",
                    "バイトの応答元",
                    "バイトの応答元",
                ),
                _ => ("data byte", "data bytes", "byte from", "bytes from"),
            };
        assert_eq!(
            xila::internationalization::translate_plural!("ping.request.payload_byte_unit", 1usize),
            data_one
        );
        assert_eq!(
            xila::internationalization::translate_plural!("ping.request.payload_byte_unit", 2usize),
            data_many
        );
        assert_eq!(
            xila::internationalization::translate_plural!("ping.reply.byte_unit", 1usize),
            reply_one
        );
        assert_eq!(
            xila::internationalization::translate_plural!("ping.reply.byte_unit", 2usize),
            reply_many
        );
    }

    #[test]
    fn suggests_misspelled_timeout_option() {
        let error = PingArguments::from_args(["localhost", "--timout", "3"].into_iter())
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains("similar argument exists: '--timeout'")
        );
    }
}
