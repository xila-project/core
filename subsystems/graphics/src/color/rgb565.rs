use core::fmt::Debug;

use crate::ColorRGB888;

use super::{ColorARGB8888, ColorRGBA8888};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorRGB565(u16);

impl Debug for ColorRGB565 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Color_RGB565_type")
            .field("Red", &self.get_red())
            .field("Green", &self.get_green())
            .field("Blue", &self.get_blue())
            .finish()
    }
}

impl ColorRGB565 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self(0).set_red(red).set_green(green).set_blue(blue)
    }

    pub const fn as_u16(self) -> u16 {
        self.0
    }

    pub const fn get_red(&self) -> u8 {
        ((self.0 >> 11) & 0b11111) as u8
    }

    pub const fn get_green(&self) -> u8 {
        ((self.0 >> 5) & 0b111111) as u8
    }

    pub const fn get_blue(&self) -> u8 {
        (self.0 & 0b11111) as u8
    }

    pub const fn set_red(mut self, value: u8) -> Self {
        let value = value & 0b11111; // 5 bits
        self.0 = (self.0 & !(0b11111 << 11)) | ((value as u16) << 11);
        self
    }

    pub const fn set_green(mut self, value: u8) -> Self {
        let value = value & 0b111111; // 6 bits
        self.0 = (self.0 & !(0b111111 << 5)) | ((value as u16) << 5);
        self
    }

    pub const fn set_blue(mut self, value: u8) -> Self {
        let value = value & 0b11111; // 5 bits
        self.0 = (self.0 & !0b11111) | (value as u16);
        self
    }
}

impl From<ColorRGB888> for ColorRGB565 {
    fn from(value: ColorRGB888) -> Self {
        Self::new(
            value.get_red() >> 3,
            value.get_green() >> 2,
            value.get_blue() >> 3,
        )
    }
}

impl From<ColorRGB565> for u16 {
    fn from(value: ColorRGB565) -> u16 {
        value.0
    }
}

impl From<u16> for ColorRGB565 {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl From<ColorARGB8888> for ColorRGB565 {
    fn from(value: ColorARGB8888) -> Self {
        Self::new(
            value.get_red() >> 3,
            value.get_green() >> 2,
            value.get_blue() >> 3,
        )
    }
}

impl From<ColorRGBA8888> for ColorRGB565 {
    fn from(value: ColorRGBA8888) -> Self {
        Self::new(
            value.get_red() >> 3,
            value.get_green() >> 2,
            value.get_blue() >> 3,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb565() {
        let color = ColorRGB565::new(255, 255, 255);
        assert_eq!(color.get_red(), 0b11111);
        assert_eq!(color.get_green(), 0b111111);
        assert_eq!(color.get_blue(), 0b11111);
        assert_eq!(color.0, 0xFFFF);

        let color = ColorRGB565::new(255, 0, 0);
        assert_eq!(color.get_red(), 0b11111);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0xF800);

        let color = ColorRGB565::new(0, 255, 0);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0b111111);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0x07E0);

        let color = ColorRGB565::new(0, 0, 255);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0b11111);
        assert_eq!(color.0, 0x00_1F);

        let color = ColorRGB565::new(0, 0, 0);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0);
    }
}
