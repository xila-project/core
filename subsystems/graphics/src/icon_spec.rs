use crate::{Color, icon_text::IconText};
use alloc::{format, string::String};

/// Environment variable containing the validated, launch-specific descriptor.
pub const ICON_ENVIRONMENT: &str = "XILA_WINDOW_ICON";

/// Read a task's inherited icon into fixed-size host-owned data.
pub async fn task_launch_icon(
    tasks: &task::Manager,
    task: task::TaskIdentifier,
) -> Option<(IconSpec, bool)> {
    let value = tasks
        .get_environment_variable(task, ICON_ENVIRONMENT)
        .await
        .ok()?;
    IconSpec::decode_launch(value.get_value())
}

/// A fixed set of inexpensive decorative templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum IconMotif {
    Rings,
    Diagonal,
    Grid,
    Steps,
    Orbit,
    Rays,
}

/// Host-owned icon data. It contains no pointers or per-icon pixel buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconSpec {
    pub motif: IconMotif,
    text: IconText,
    pub background: Color,
    pub foreground: Color,
    pub accent: Color,
    pub seed: u32,
}

impl IconSpec {
    /// Build a reproducible fallback from an application identity or legacy text.
    pub fn automatic(identity: &str, text: &str, background: Color) -> Self {
        let mut seed: u32 = 2_166_136_261;
        for byte in identity.bytes() {
            seed = (seed ^ u32::from(byte)).wrapping_mul(16_777_619);
        }
        let motif = match seed % 6 {
            0 => IconMotif::Rings,
            1 => IconMotif::Diagonal,
            2 => IconMotif::Grid,
            3 => IconMotif::Steps,
            4 => IconMotif::Orbit,
            _ => IconMotif::Rays,
        };
        let brightness = u32::from(background.get_red()) * 299
            + u32::from(background.get_green()) * 587
            + u32::from(background.get_blue()) * 114;
        let foreground = if brightness >= 150_000 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        Self::procedural(motif, text, background, foreground, foreground, seed)
    }

    /// Preserve the supplied template, colors and variation exactly.
    pub fn procedural(
        motif: IconMotif,
        text: &str,
        background: Color,
        foreground: Color,
        accent: Color,
        seed: u32,
    ) -> Self {
        Self {
            motif,
            text: IconText::new(text),
            background,
            foreground,
            accent,
            seed,
        }
    }

    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// Parse the validated launch wrapper without retaining the input buffer.
    pub fn decode_launch(value: &str) -> Option<(Self, bool)> {
        let (mode, payload) = value.split_once(';')?;
        let explicit = match mode {
            "auto" => false,
            "explicit" => true,
            _ => return None,
        };
        Some((Self::decode(payload)?, explicit))
    }

    /// Encode a resolved descriptor for bounded, inherited launch metadata.
    pub fn encode(&self) -> String {
        fn rgb(value: Color) -> u32 {
            u32::from(value.get_red()) << 16
                | u32::from(value.get_green()) << 8
                | u32::from(value.get_blue())
        }
        format!(
            "1,{},{},{:06x},{:06x},{:06x},{}",
            self.motif as u8,
            self.seed,
            rgb(self.background),
            rgb(self.foreground),
            rgb(self.accent),
            self.text()
        )
    }

    /// Decode the host-owned versioned form; reject malformed or oversized values.
    pub fn decode(value: &str) -> Option<Self> {
        if value.len() > 64 || value.as_bytes().contains(&0) {
            return None;
        }
        let mut fields = value.splitn(7, ',');
        if fields.next()? != "1" {
            return None;
        }
        let motif = match fields.next()? {
            "0" => IconMotif::Rings,
            "1" => IconMotif::Diagonal,
            "2" => IconMotif::Grid,
            "3" => IconMotif::Steps,
            "4" => IconMotif::Orbit,
            "5" => IconMotif::Rays,
            _ => return None,
        };
        let seed = fields.next()?.parse().ok()?;
        fn rgb(value: &str) -> Option<Color> {
            if value.len() != 6 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let value = u32::from_str_radix(value, 16).ok()?;
            Some(Color::new(
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ))
        }
        let background = rgb(fields.next()?)?;
        let foreground = rgb(fields.next()?)?;
        let accent = rgb(fields.next()?)?;
        let text = fields.next()?;
        if text.chars().count() > 2 {
            return None;
        }
        Some(Self::procedural(
            motif, text, background, foreground, accent, seed,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    #[test]
    fn automatic_icons_are_deterministic_and_stable() {
        let a = IconSpec::automatic("com.example.calculator", "Ca", Color::new(30, 60, 90));
        let b = IconSpec::automatic("com.example.calculator", "Ca", Color::new(30, 60, 90));
        assert_eq!(a, b);
        assert_ne!(
            a.seed,
            IconSpec::automatic("com.example.clock", "Ca", Color::new(30, 60, 90)).seed
        );
    }

    #[test]
    fn wire_roundtrip_and_rejects_malformed_input() {
        let spec = IconSpec::procedural(
            IconMotif::Orbit,
            "界🦀",
            Color::new(1, 2, 3),
            Color::new(4, 5, 6),
            Color::new(7, 8, 9),
            42,
        );
        let encoded = spec.encode();
        assert_eq!(IconSpec::decode(&encoded), Some(spec));
        assert_eq!(IconSpec::decode("2,0,0,000000,ffffff,000000,A"), None);
        assert_eq!(IconSpec::decode("1,6,0,000000,ffffff,000000,A"), None);
        assert_eq!(IconSpec::decode("1,0,0,xyz,ffffff,000000,A"), None);
        assert_eq!(IconSpec::decode("1,0,0,000000,ffffff,000000,ABC"), None);
    }

    #[test]
    fn explicitly_chosen_values_are_preserved() {
        let spec = IconSpec::procedural(
            IconMotif::Rings,
            "界",
            Color::new(1, 2, 3),
            Color::new(4, 5, 6),
            Color::new(7, 8, 9),
            42,
        );
        assert_eq!(spec.motif, IconMotif::Rings);
        assert_eq!(spec.text(), "界");
        assert_eq!(spec.background, Color::new(1, 2, 3));
        assert_eq!(spec.foreground, Color::new(4, 5, 6));
        assert_eq!(spec.accent, Color::new(7, 8, 9));
        assert_eq!(spec.seed, 42);
    }
}
