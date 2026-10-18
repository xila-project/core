use crate::error::Result;
use xila::graphics::{Event, lvgl};

/// Enum to hold all tab types (avoids dyn compatibility issues with async traits)
pub enum Tab {
    General(GeneralTab),
    Appearance(AppearanceTab),
    Password(PasswordTab),
    About(AboutTab),
    Network(NetworkTab),
}

impl Tab {
    pub async fn create_ui(&mut self, parent: *mut lvgl::lv_obj_t) -> Result<*mut lvgl::lv_obj_t> {
        match self {
            Tab::General(tab) => tab.create_ui(parent).await,
            Tab::Appearance(tab) => tab.create_ui(parent).await,
            Tab::Password(tab) => tab.create_ui(parent).await,
            Tab::About(tab) => tab.create_ui(parent).await,
            Tab::Network(tab) => tab.create_ui(parent).await,
        }
    }

    pub async fn handle_event(&mut self, event: &Event) -> bool {
        match self {
            Tab::General(tab) => tab.handle_event(event).await,
            Tab::Appearance(tab) => tab.handle_event(event).await,
            Tab::Password(tab) => tab.handle_event(event).await,
            Tab::About(tab) => tab.handle_event(event).await,
            Tab::Network(tab) => tab.handle_event(event).await,
        }
    }
}

// Re-export tab modules
pub mod about;
pub mod appearance;
pub mod general;
pub mod network;
pub mod password;

pub use about::AboutTab;
pub use appearance::AppearanceTab;
pub use general::GeneralTab;
pub use network::NetworkTab;
pub use password::PasswordTab;
