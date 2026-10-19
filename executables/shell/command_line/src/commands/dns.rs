use crate::Result;
use getargs::Options;
use getargs_derive::GetArgs;
use xila::{
    file_system::Path,
    internationalization::translate,
    network::{self, DnsQueryKind},
};

use super::{CommandContext, UserCommand};

pub struct DnsResolveCommand;

impl UserCommand for DnsResolveCommand {
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
        execute_dns_resolve(context, options).await
    }
}

#[derive(GetArgs)]
#[command(name = "dns_resolve", about = "Resolve a DNS name.")]
pub(super) struct DnsResolveArguments<'a> {
    domain: &'a str,
    #[arg(short = 'a', long = "a")]
    a_enabled: bool,
    #[arg(short = 'A', long = "aaaa")]
    aaaa_enabled: bool,
    #[arg(short = 'c', long = "cname")]
    cname_enabled: bool,
    #[arg(short = 'n', long = "ns")]
    ns_enabled: bool,
    #[arg(short = 's', long = "soa")]
    soa_enabled: bool,
}

fn format_kind(kind: DnsQueryKind) -> &'static str {
    match kind {
        DnsQueryKind::A => "A",
        DnsQueryKind::Aaaa => "AAAA",
        DnsQueryKind::Cname => "CNAME",
        DnsQueryKind::Ns => "NS",
        DnsQueryKind::Soa => "SOA",
        _ => "UNKNOWN",
    }
}

async fn resolve_record<C: CommandContext>(
    context: &mut C,
    network_manager: &network::Manager,
    domain: &str,
    kind: DnsQueryKind,
) -> Result<()> {
    match network_manager.resolve(domain, kind, true, None).await {
        Ok(ip) => {
            let count = ip.len();
            let record_label =
                xila::internationalization::translate_plural!("DNS records for domain", count);
            // DNS record types (A, AAAA, etc.) remain invariant CLI identifiers.
            context
                .write_out_fmt(format_args!(
                    "{}: {} {} '{}':\n",
                    format_kind(kind),
                    count,
                    record_label,
                    domain
                ))
                .await?;
            for address in &ip {
                context
                    .write_out_fmt(format_args!(" - {}\n", address))
                    .await?;
            }
        }
        Err(network::Error::Failed) => {
            context
                .write_out_fmt(format_args!(
                    "{}\n",
                    format_args!(
                        translate!("No {} records found for domain '{}'"),
                        format_kind(kind),
                        domain
                    )
                ))
                .await?;
        }
        Err(e) => {
            context
                .write_out_fmt(format_args!(
                    "{}",
                    format_args!(
                        translate!("Failed to resolve domain '{}' for {} records: {}"),
                        domain,
                        format_kind(kind),
                        e
                    )
                ))
                .await?;
        }
    }

    Ok(())
}

async fn execute_dns_resolve<'a, I, C>(
    context: &mut C,
    options: &mut Options<&'a str, I>,
) -> Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let DnsResolveArguments {
        domain,
        a_enabled,
        aaaa_enabled,
        cname_enabled,
        ns_enabled,
        soa_enabled,
    } = DnsResolveArguments::parse(options)?;

    let default = !a_enabled && !aaaa_enabled && !cname_enabled && !ns_enabled && !soa_enabled;

    let network_manager = context
        .executable_context()
        .network_manager
        .as_ref()
        .ok_or(crate::Error::InvalidOption)?;

    if a_enabled || default {
        resolve_record(context, network_manager, domain, DnsQueryKind::A).await?;
    }
    if aaaa_enabled || default {
        resolve_record(context, network_manager, domain, DnsQueryKind::Aaaa).await?;
    }
    if cname_enabled {
        resolve_record(context, network_manager, domain, DnsQueryKind::Cname).await?;
    }
    if ns_enabled {
        resolve_record(context, network_manager, domain, DnsQueryKind::Ns).await?;
    }
    if soa_enabled {
        resolve_record(context, network_manager, domain, DnsQueryKind::Soa).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_kind;
    use xila::network::DnsQueryKind;

    #[test]
    fn dns_record_count_selects_singular_and_plural() {
        let (one, other) = match xila::internationalization::get_locale() {
            "fr" => (
                "enregistrement pour le domaine",
                "enregistrements pour le domaine",
            ),
            _ => ("record for domain", "records for domain"),
        };
        assert_eq!(
            xila::internationalization::translate_plural!("DNS records for domain", 1usize),
            one
        );
        assert_eq!(
            xila::internationalization::translate_plural!("DNS records for domain", 2usize),
            other
        );
    }

    #[test]
    fn format_kind_maps_a() {
        assert_eq!(format_kind(DnsQueryKind::A), "A");
    }

    #[test]
    fn format_kind_maps_aaaa() {
        assert_eq!(format_kind(DnsQueryKind::Aaaa), "AAAA");
    }

    #[test]
    fn format_kind_maps_cname() {
        assert_eq!(format_kind(DnsQueryKind::Cname), "CNAME");
    }

    #[test]
    fn format_kind_maps_ns() {
        assert_eq!(format_kind(DnsQueryKind::Ns), "NS");
    }

    #[test]
    fn format_kind_maps_soa() {
        assert_eq!(format_kind(DnsQueryKind::Soa), "SOA");
    }
}
