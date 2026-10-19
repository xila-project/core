use core::fmt::Debug;

use crate::ColorRGB888;

use super::{ColorRGB565, ColorRGBA8888};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorARGB8888(u32);

impl Debug for ColorARGB8888 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Color_ARGB8888_type")
            .field("Alpha", &self.get_alpha())
            .field("Red", &self.get_red())
            .field("Green", &self.get_green())
            .field("Blue", &self.get_blue())
            .finish()
    }
}

impl ColorARGB8888 {
    pub const fn new(alpha: u8, red: u8, green: u8, blue: u8) -> Self {
        Self(0)
            .set_alpha(alpha)
            .set_red(red)
            .set_green(green)
            .set_blue(blue)
    }

    pub const fn from_rgb565(value: ColorRGB565) -> Self {
        Self::new(
            0xFF,
            value.get_red() << 3 | value.get_red() >> 2,
            value.get_green() << 2 | value.get_green() >> 4,
            value.get_blue() << 3 | value.get_blue() >> 2,
        )
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn get_alpha(&self) -> u8 {
        ((self.0 >> 24) & 0b1111_1111) as u8
    }

    pub const fn get_red(&self) -> u8 {
        ((self.0 >> 16) & 0b1111_1111) as u8
    }

    pub const fn get_green(&self) -> u8 {
        ((self.0 >> 8) & 0b1111_1111) as u8
    }

    pub const fn get_blue(&self) -> u8 {
        ((self.0) & 0b1111_1111) as u8
    }

    pub const fn set_alpha(mut self, value: u8) -> Self {
        self.0 = (self.0 & !(0b1111_1111 << 24)) | ((value as u32) << 24);
        self
    }

    pub const fn set_red(mut self, value: u8) -> Self {
        self.0 = (self.0 & 0xFF00_FFFF) | ((value as u32) << 16);
        self
    }

    pub const fn set_green(mut self, value: u8) -> Self {
        self.0 = (self.0 & 0xFFFF_00FF) | ((value as u32) << 8);
        self
    }

    pub const fn set_blue(mut self, value: u8) -> Self {
        self.0 = (self.0 & 0xFFFF_FF00) | (value as u32);
        self
    }
}

impl From<ColorRGB888> for ColorARGB8888 {
    fn from(value: ColorRGB888) -> Self {
        Self::new(0xFF, value.get_red(), value.get_green(), value.get_blue())
    }
}

impl From<ColorARGB8888> for u32 {
    fn from(value: ColorARGB8888) -> u32 {
        value.0
    }
}

impl From<u32> for ColorARGB8888 {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ColorRGB565> for ColorARGB8888 {
    fn from(value: ColorRGB565) -> Self {
        Self::from_rgb565(value)
    }
}

impl From<ColorRGBA8888> for ColorARGB8888 {
    fn from(value: ColorRGBA8888) -> Self {
        Self::new(
            value.get_alpha(),
            value.get_red(),
            value.get_green(),
            value.get_blue(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_argb8888() {
        let color = ColorARGB8888::new(255, 255, 255, 255);
        assert_eq!(color.get_alpha(), 0xFF);
        assert_eq!(color.get_red(), 0xFF);
        assert_eq!(color.get_green(), 0xFF);
        assert_eq!(color.get_blue(), 0xFF);
        assert_eq!(color.0, 0xFFFF_FFFF);

        let color = ColorARGB8888::new(255, 0, 0, 0);
        assert_eq!(color.get_alpha(), 0xFF);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0xFF00_0000);

        let color = ColorARGB8888::new(0, 255, 0, 0);
        assert_eq!(color.get_alpha(), 0);
        assert_eq!(color.get_red(), 0xFF);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0x00FF_0000);

        let color = ColorARGB8888::new(0, 0, 255, 0);
        assert_eq!(color.get_alpha(), 0);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0xFF);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0x0000_FF00);

        let color = ColorARGB8888::new(0, 0, 0, 255);
        assert_eq!(color.get_alpha(), 0);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0xFF);
        assert_eq!(color.0, 0x0000_00FF);

        let color = ColorARGB8888::new(0, 0, 0, 0);
        assert_eq!(color.get_alpha(), 0);
        assert_eq!(color.get_red(), 0);
        assert_eq!(color.get_green(), 0);
        assert_eq!(color.get_blue(), 0);
        assert_eq!(color.0, 0);
    }
}
