#![no_std]

extern crate alloc;

xila::internationalization::track_locales!();

mod device;
mod error;
mod executable;
mod terminal;

use alloc::boxed::Box;
pub use executable::*;

use alloc::{string::String, vec, vec::Vec};
use core::fmt::Write;
use core::{num::NonZeroUsize, time::Duration};
use error::*;
use xila::executable::Standard;
use xila::graphics;
use xila::log;
use xila::task::{self, TaskIdentifier};
use xila::virtual_file_system::ItemStatic;

use crate::{error::Result, terminal::Terminal};

pub const SHORTCUT: &str = r#"
{
    "name": "Terminal",
    "command": "/applications/executables/terminal",
    "arguments": [],
    "terminal": false,
    "icon_string": ">_",
    "icon_color": [0, 0, 0]
}"#;

async fn mount_and_open(
    task: TaskIdentifier,
    terminal: &'static Terminal,
    standard: &Standard<'static>,
) -> Result<Standard<'static>> {
    let virtual_file_system = &standard.context.virtual_file_system;

    let _ = virtual_file_system
        .remove(task, &"/devices/standard_io/terminal")
        .await;

    virtual_file_system
        .mount_static(
            task,
            &"/devices/standard_io/terminal",
            ItemStatic::CharacterDevice(terminal),
        )
        .await?;

    let standard = Standard::open(
        &"/devices/standard_io/terminal",
        &"/devices/standard_io/terminal",
        &"/devices/standard_io/terminal",
        task,
        virtual_file_system,
        standard.context,
    )
    .await?;

    Ok(standard)
}

async fn inner_main(task: TaskIdentifier, standard: &Standard<'static>) -> Result<()> {
    let terminal = Terminal::new(graphics::ffi_manager()).await?;

    let terminal = Box::leak(Box::new(terminal));

    let standard = mount_and_open(task, terminal, standard).await?;

    xila::executable::execute(
        standard.context,
        "/applications/executables/command_line_shell",
        vec![],
        standard,
        None,
    )
    .await?;

    while terminal.handle_events().await? {
        task::Manager::sleep(Duration::from_millis(20)).await;
    }

    Ok(())
}

pub async fn main(
    mut standard: Standard<'static>,
    _: Vec<String>,
) -> core::result::Result<(), NonZeroUsize> {
    let task_manager = &standard.context.task_manager;
    let task = task_manager.get_current_task_identifier().await;

    if let Err(error) = inner_main(task, &standard).await {
        let _ = writeln!(standard.error(), "{}", error);
        log::error!("Terminal executable error: {}", error);
        return Err(error.into());
    }

    Ok(())
}
