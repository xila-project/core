use alloc::string::String;
use xila::file_system::Kind;
use xila::graphics::OwnedWindow;
use xila::graphics::{
    self, EventKind, lvgl,
    palette::{self, Hue},
};
use xila::task;

use crate::error::Result;
use crate::tabs::{AboutTab, AppearanceTab, GeneralTab, NetworkTab, PasswordTab, Tab};

pub struct Settings {
    graphics_manager: &'static graphics::Manager,
    window: OwnedWindow,
    tabs: [Tab; 5],
}

#[derive(Clone)]
pub struct FileItem {
    pub name: String,
    pub r#type: Kind,
    pub size: u64,
}

impl Settings {
    pub async fn new(context: &'static xila::executable::ExecutableContext) -> Result<Self> {
        let graphics_manager = graphics::ffi_manager();
        // On construction failure, release the lock before OwnedWindow::drop
        // acquires it to delete the partially built widget tree.
        let mut window;
        let _lock = graphics_manager.lock().await;
        window = graphics_manager.create_window().await?;

        window.set_icon("Se", palette::get(Hue::Grey, palette::Tone::MAIN));

        // Flatten window chrome so the tabview fills the window edge-to-edge
        unsafe {
            let window = window.as_object_mutable();
            lvgl::lv_obj_set_style_pad_all(window, 0, lvgl::LV_STATE_DEFAULT);
            lvgl::lv_obj_set_layout(window, lvgl::lv_layout_t_LV_LAYOUT_FLEX);
            lvgl::lv_obj_set_flex_flow(window, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
        }

        // Create tabview
        let tabview = unsafe {
            let tabview = lvgl::lv_tabview_create(window.as_object_mutable());

            if tabview.is_null() {
                return Err(crate::error::Error::FailedToCreateUiElement);
            }

            lvgl::lv_obj_set_size(tabview, lvgl::lv_pct(100), lvgl::lv_pct(100));
            lvgl::lv_obj_set_flex_grow(tabview, 1);

            tabview
        };

        // Create tabs
        let mut tabs = [
            Tab::General(GeneralTab::new()),
            Tab::Appearance(AppearanceTab::new(context).await),
            Tab::Password(PasswordTab::new(context)),
            Tab::Network(NetworkTab::new(context)),
            Tab::About(AboutTab::new(context)),
        ];

        for tab in &mut tabs {
            tab.create_ui(tabview).await?;
        }

        let manager = Self {
            graphics_manager,
            window,
            tabs,
        };

        Ok(manager)
    }

    pub async fn handle_events(&mut self) -> bool {
        let mut needs_refresh = false;
        graphics::lock!(self.graphics_manager, {
            while let Some(event) = self.window.pop_event() {
                // Logique de filtrage spécifique à Settings
                if (event.code == EventKind::Delete || event.code == EventKind::CloseRequested)
                    && event.target == self.window.as_object_mutable()
                {
                    return false;
                }

                for tab in &mut self.tabs {
                    if tab.handle_event(&event).await {
                        break;
                    }
                }
            }
            for tab in &mut self.tabs {
                if let Tab::Network(network) = tab {
                    network.refresh().await;
                    needs_refresh = network.needs_refresh();
                }
            }
        });

        let events = task::suspend(|ctx| {
            self.window.register_waker(ctx.waker());
        });
        if needs_refresh {
            embassy_futures::select::select(
                events,
                task::sleep(core::time::Duration::from_millis(250)),
            )
            .await;
        } else {
            events.await;
        }

        true
    }
}

// Interface panels own widgets and ephemeral password inputs. Release them
// under the graphics lock before OwnedWindow deletes their parent tree.
impl Drop for Settings {
    fn drop(&mut self) {
        let _lock = task::block_on(self.graphics_manager.lock());
        for tab in &mut self.tabs {
            if let Tab::Network(network) = tab {
                network.close_panel();
            }
        }
    }
}
