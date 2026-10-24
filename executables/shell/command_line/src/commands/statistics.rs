use crate::{Error, Result};
use alloc::{borrow::ToOwned, format, string::String};
use getargs::Options;
use getargs_derive::GetArgs;
use xila::{
    file_system::Path,
    internationalization::translate,
    shared::{BYTES_SUFFIX, Unit},
};

use super::{CommandContext, UserCommand};

pub struct StatisticsCommand;

impl UserCommand for StatisticsCommand {
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
        execute_statistics(context, options).await
    }
}

#[derive(GetArgs)]
#[command(name = "stat", about = "Show file metadata.")]
pub(super) struct StatisticsArguments<'a> {
    path: &'a str,
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

async fn resolve_user_name<C: CommandContext>(
    context: &C,
    user: xila::users::UserIdentifier,
) -> String {
    match context
        .executable_context()
        .users_manager
        .get_user_name(user)
        .await
    {
        Ok(name) => name,
        Err(_) => format!("{}", user.as_u16()),
    }
}

async fn resolve_group_name<C: CommandContext>(
    context: &C,
    group: xila::users::GroupIdentifier,
) -> String {
    match context
        .executable_context()
        .users_manager
        .get_group_name(group)
        .await
    {
        Ok(name) => name,
        Err(_) => format!("{}", group.as_u16()),
    }
}

fn format_statistics(
    statistics: &xila::file_system::Statistics,
    user: &str,
    group: &str,
) -> String {
    let size = Unit::new(statistics.size as f32, BYTES_SUFFIX.name);

    format!(
        r#"{}: {} - {} : {}
{}: {} - {}: {}
{}: {} - {}: {} - {}: {}
{}: {}
{}: {}
{}: {}
{}: {}

"#,
        translate!("listing.columns.kind"),
        statistics.kind,
        translate!("listing.columns.inode"),
        statistics.inode,
        translate!("listing.columns.links"),
        statistics.links,
        translate!("listing.columns.size"),
        size,
        translate!("listing.columns.user"),
        user,
        translate!("listing.columns.group"),
        group,
        translate!("listing.columns.permissions"),
        statistics.permissions,
        translate!("listing.columns.accessed"),
        statistics.access,
        translate!("listing.columns.modified"),
        statistics.modification,
        translate!("listing.columns.created"),
        statistics.creation,
        translate!("listing.columns.status"),
        statistics.status,
    )
}

async fn execute_statistics<'a, I, C>(
    context: &mut C,
    options: &mut Options<&'a str, I>,
) -> Result<(), crate::Error<'a>>
where
    I: Iterator<Item = &'a str>,
    C: CommandContext,
{
    let StatisticsArguments { path } = StatisticsArguments::parse(options)?;
    let path = resolve_path(context, path)?;

    let statistics = context
        .executable_context()
        .virtual_file_system
        .get_statistics(&path)
        .await
        .map_err(Error::FailedToGetMetadata)?;

    let user = resolve_user_name(context, statistics.user).await;
    let group = resolve_group_name(context, statistics.group).await;
    let output = format_statistics(&statistics, &user, &group);

    context.write_out_fmt(format_args!("{}", output)).await?;

    Ok(())
}
