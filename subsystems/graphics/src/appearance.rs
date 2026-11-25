//! Per-user appearance preferences: theme and screen brightness. Record
//! layouts and preset IDs are stable on disk; version 1 records, which have no
//! brightness, still load at full brightness, and a saved brightness outside
//! the allowed range is clamped on load instead of rejecting the record.
use crate as graphics;
use crate::{BRIGHTNESS_FULL, Color, Error, lvgl};
use file_system::{AccessFlags, CreateFlags, Flags, Permissions};
use task::TaskIdentifier;
use virtual_file_system::ResultExt as _;
use virtual_file_system::{self, File, VirtualFileSystem};

const RECORD_1_SIZE: usize = 11;
const RECORD_SIZE: usize = 12;
pub const PRESET_COUNT: u8 = 6;

/// Lowest brightness a saved appearance can hold, about 10 %, so that a
/// preference can never leave the screen dark at start-up or login.
pub const MINIMUM_BRIGHTNESS: u8 = 26;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub is_dark: bool,
    /// Zero selects the zinc palette's mode-dependent color.
    pub primary: u8,
    pub secondary: u8,
    /// Screen backlight level, from [`MINIMUM_BRIGHTNESS`] to
    /// [`BRIGHTNESS_FULL`]. Ignored on screens without brightness control.
    pub brightness: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            is_dark: true,
            primary: 0,
            secondary: 0,
            brightness: BRIGHTNESS_FULL,
        }
    }
}

impl Appearance {
    pub fn colors(self) -> (Color, Color) {
        // These palette accessors return immutable static data; no LVGL objects
        // are accessed, and they can be read before graphics initialization.
        let palette = unsafe {
            if self.is_dark {
                *lvgl::lv_theme_shadcn_palette_zinc_dark()
            } else {
                *lvgl::lv_theme_shadcn_palette_zinc_light()
            }
        };
        let preset = |index, automatic| match index {
            1 => Color::new(0x25, 0x63, 0xeb),
            2 => Color::new(0x16, 0xa3, 0x4a),
            3 => Color::new(0xea, 0x58, 0x0c),
            4 => Color::new(0xe1, 0x1d, 0x48),
            5 => Color::new(0x7c, 0x3a, 0xed),
            _ => Color::from_lvgl_color(automatic),
        };
        (
            preset(self.primary, palette.primary),
            preset(self.secondary, palette.secondary),
        )
    }

    /// Apply the theme and the screen brightness. Screens without brightness
    /// control keep their level. The caller holds the graphics manager's LVGL
    /// lock.
    pub async fn apply(self, manager: &graphics::Manager) -> graphics::Result<()> {
        let (primary, secondary) = self.colors();
        manager
            .update_theme_locked(primary, secondary, self.is_dark)
            .await?;
        self.apply_brightness(manager).await
    }

    /// Apply only the screen brightness, raised to [`MINIMUM_BRIGHTNESS`].
    /// Screens without brightness control keep their level. The LVGL lock may
    /// be held or not.
    pub async fn apply_brightness(self, manager: &graphics::Manager) -> graphics::Result<()> {
        match manager
            .set_brightness(self.brightness.max(MINIMUM_BRIGHTNESS))
            .await
        {
            Err(Error::Unsupported | Error::NotAvailable) => Ok(()),
            result => result,
        }
    }

    fn encode(self) -> [u8; RECORD_SIZE] {
        let mut bytes = [0; RECORD_SIZE];
        bytes[..4].copy_from_slice(b"XAP2");
        bytes[4] = u8::from(self.is_dark);
        bytes[5] = self.primary;
        bytes[6] = self.secondary;
        bytes[7] = self.brightness;
        let checksum = checksum(&bytes[..8]);
        bytes[8..].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        // Version 1 records end before the brightness byte.
        let (brightness, body) = match (bytes.len(), bytes.get(..4)?) {
            (RECORD_1_SIZE, b"XAP1") => (BRIGHTNESS_FULL, 7),
            (RECORD_SIZE, b"XAP2") => (bytes[7], 8),
            _ => return None,
        };
        if bytes[4] > 1
            || bytes[5] >= PRESET_COUNT
            || bytes[6] >= PRESET_COUNT
            || checksum(&bytes[..body]) != u32::from_le_bytes(bytes[body..].try_into().ok()?)
        {
            return None;
        }
        Some(Self {
            is_dark: bytes[4] != 0,
            primary: bytes[5],
            secondary: bytes[6],
            // Clamp instead of rejecting: the checksum already guards against
            // corruption, and a level saved below the current minimum (raised
            // after the record was written) must not discard the theme.
            brightness: brightness.clamp(MINIMUM_BRIGHTNESS, BRIGHTNESS_FULL),
        })
    }

    pub async fn load(
        vfs: &VirtualFileSystem,
        task: TaskIdentifier,
    ) -> virtual_file_system::Result<Self> {
        let user = vfs.task_manager().get_user(task).await.map_io_error()?;
        let path = vfs
            .get_user_home(user)
            .await?
            .append(".appearance")
            .ok_or(virtual_file_system::IoErrorKind::InvalidPath)?;
        let mut file = match File::open(vfs, task, &path, AccessFlags::Read.into()).await {
            Ok(file) => file,
            Err(file_system::IoErrorKind::NotFound) => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error),
        };
        // One extra byte rejects oversized files without allocating for their contents.
        let mut bytes = [0; RECORD_SIZE + 1];
        let mut length = 0;
        while length < bytes.len() {
            let read = file.read(&mut bytes[length..]).await?;
            if read == 0 {
                break;
            }
            length += read;
        }
        file.close(vfs).await?;
        Self::decode(&bytes[..length]).ok_or(virtual_file_system::IoErrorKind::InvalidArgument)
    }

    /// Persist this user's choices. Callers must serialize saves for that user;
    /// Settings does so with the LVGL lock held during event dispatch.
    pub async fn save(
        self,
        vfs: &VirtualFileSystem,
        task: TaskIdentifier,
    ) -> virtual_file_system::Result<()> {
        let user = vfs.task_manager().get_user(task).await.map_io_error()?;
        let home = vfs.ensure_user_home(task, user).await?;
        let path = home
            .clone()
            .append(".appearance")
            .ok_or(virtual_file_system::IoErrorKind::InvalidPath)?;
        let temporary = home
            .append(".appearance.tmp")
            .ok_or(virtual_file_system::IoErrorKind::InvalidPath)?;
        // The graphics lock serializes Settings writers. Remove any temporary
        // file left by an interrupted save.
        match vfs.remove(task, &temporary).await {
            Ok(()) | Err(file_system::IoErrorKind::NotFound) => {}
            Err(error) => return Err(error),
        }
        let mut file = File::open(
            vfs,
            task,
            &temporary,
            Flags::new(AccessFlags::Write, Some(CreateFlags::CREATE_TRUNCATE), None),
        )
        .await?;
        let bytes = self.encode();
        if Self::decode(&bytes).is_none() {
            return Err(virtual_file_system::IoErrorKind::InvalidArgument);
        }
        file.set_permissions(Permissions::USER_READ_WRITE).await?;
        let mut written = 0;
        while written < bytes.len() {
            let count = file.write(&bytes[written..]).await?;
            if count == 0 {
                return Err(virtual_file_system::IoErrorKind::WriteZero);
            }
            written += count;
        }
        file.flush().await?;
        file.close(vfs).await?;
        vfs.rename(task, &temporary, &path).await
    }
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

/// Restore the current user's appearance at login, before opening application
/// windows. This acquires the LVGL lock; callers must not already hold it.
/// Invalid preferences leave the existing theme intact.
pub async fn restore_appearance(
    manager: &graphics::Manager,
    vfs: &VirtualFileSystem,
    task: TaskIdentifier,
) {
    if let Ok(user) = vfs.task_manager().get_user(task).await
        && let Err(error) = vfs.ensure_user_home(task, user).await
    {
        log::warning!("Could not prepare user home: {}", error);
    }
    match Appearance::load(vfs, task).await {
        Ok(appearance) => {
            let _lock = manager.lock().await;
            if let Err(error) = appearance.apply(manager).await {
                log::warning!("Could not restore appearance: {}", error);
            }
        }
        Err(error) => log::warning!("Could not read appearance preferences: {}", error),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn records_round_trip_and_reject_damaged_or_unknown_choices() {
        for is_dark in [false, true] {
            for primary in 0..PRESET_COUNT {
                for secondary in 0..PRESET_COUNT {
                    for brightness in [MINIMUM_BRIGHTNESS, 128, BRIGHTNESS_FULL] {
                        let appearance = Appearance {
                            is_dark,
                            primary,
                            secondary,
                            brightness,
                        };
                        assert_eq!(Appearance::decode(&appearance.encode()), Some(appearance));
                    }
                }
            }
        }
        let bytes = Appearance::default().encode();
        for index in 0..bytes.len() {
            let mut damaged = bytes;
            damaged[index] ^= 1;
            assert_eq!(Appearance::decode(&damaged), None);
        }
        for length in 0..bytes.len() {
            assert_eq!(Appearance::decode(&bytes[..length]), None);
        }
        assert_eq!(Appearance::decode(&[0; RECORD_SIZE + 1]), None);
        for invalid in [
            Appearance {
                primary: PRESET_COUNT,
                ..Appearance::default()
            },
            Appearance {
                secondary: PRESET_COUNT,
                ..Appearance::default()
            },
        ] {
            assert_eq!(Appearance::decode(&invalid.encode()), None);
        }
    }

    #[test]
    fn brightness_below_the_minimum_is_clamped_and_the_theme_kept() {
        // A record saved before the minimum was raised keeps its theme and
        // loads with the brightness clamped to the allowed range.
        for brightness in [0, 1, MINIMUM_BRIGHTNESS - 1] {
            let saved = Appearance {
                is_dark: false,
                primary: 3,
                secondary: 5,
                brightness,
            };
            assert_eq!(
                Appearance::decode(&saved.encode()),
                Some(Appearance {
                    brightness: MINIMUM_BRIGHTNESS,
                    ..saved
                })
            );
        }
    }

    #[test]
    fn version_1_records_load_at_full_brightness() {
        let mut bytes = [0; RECORD_1_SIZE];
        bytes[..4].copy_from_slice(b"XAP1");
        bytes[4..7].copy_from_slice(&[0, 3, 5]);
        let checksum = checksum(&bytes[..7]);
        bytes[7..].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(
            Appearance::decode(&bytes),
            Some(Appearance {
                is_dark: false,
                primary: 3,
                secondary: 5,
                brightness: BRIGHTNESS_FULL,
            })
        );
        // A version 1 tag on a version 2 length, and the reverse, are damaged.
        let mut current = Appearance::default().encode();
        current[..4].copy_from_slice(b"XAP1");
        assert_eq!(Appearance::decode(&current), None);
        bytes[..4].copy_from_slice(b"XAP2");
        assert_eq!(Appearance::decode(&bytes), None);
    }

    #[test]
    fn automatic_colors_follow_zinc_mode_and_presets_survive_mode_changes() {
        let mut appearance = Appearance::default();
        assert!(appearance.is_dark);
        assert_eq!(
            appearance.colors(),
            (Color::new(228, 228, 231), Color::new(39, 39, 42))
        );
        appearance.is_dark = false;
        assert_eq!(
            appearance.colors(),
            (Color::new(24, 24, 27), Color::new(244, 244, 245))
        );
        appearance.primary = 1;
        appearance.secondary = 2;
        let colors = appearance.colors();
        assert_ne!(colors.0, colors.1);
        appearance.is_dark = true;
        assert_eq!(appearance.colors(), colors);
    }
}
