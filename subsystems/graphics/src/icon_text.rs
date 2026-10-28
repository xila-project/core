/// At most two Unicode scalar values, stored without allocating.
#[derive(Clone, Copy)]
pub(crate) struct IconText([u8; 8]);

impl IconText {
    pub(crate) fn new(text: &str) -> Self {
        let mut bytes = [0; 8];
        let mut length = 0;
        for character in text.chars().take(2) {
            if character == '\0' {
                break;
            }
            let mut encoded = [0; 4];
            let encoded = character.encode_utf8(&mut encoded).as_bytes();
            bytes[length..length + encoded.len()].copy_from_slice(encoded);
            length += encoded.len();
        }
        Self(bytes)
    }

    pub(crate) fn as_str(&self) -> &str {
        let end = self
            .0
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(self.0.len());
        core::str::from_utf8(&self.0[..end]).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_at_most_two_complete_unicode_scalars() {
        assert_eq!(IconText::new("界éX").as_str(), "界é");
        assert_eq!(IconText::new("😀😀x").as_str(), "😀😀");
    }

    #[test]
    fn empty_and_single_character_icons_do_not_have_stale_bytes() {
        let mut text = IconText::new("AB");
        assert_eq!(text.as_str(), "AB");
        text = IconText::new("Z");
        assert_eq!(text.as_str(), "Z");
        text = IconText::new("");
        assert_eq!(text.as_str(), "");
        assert_eq!(IconText::new("a\0b").as_str(), "a");
    }
}
