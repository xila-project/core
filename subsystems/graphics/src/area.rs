use crate::{Point, lvgl};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Area(Point, Point);

impl Area {
    pub fn new(point_1: Point, point_2: Point) -> Self {
        Self(point_1, point_2)
    }

    pub fn get_point_1(&self) -> Point {
        self.0
    }

    pub fn get_point_2(&self) -> Point {
        self.1
    }

    pub fn get_width(&self) -> u16 {
        (self.1.get_x() - self.0.get_x()).unsigned_abs() + 1
    }

    pub fn get_height(&self) -> u16 {
        (self.1.get_y() - self.0.get_y()).unsigned_abs() + 1
    }

    pub fn set_point_1(mut self, value: Point) -> Self {
        self.0 = value;
        self
    }

    pub fn set_point_2(mut self, value: Point) -> Self {
        self.1 = value;
        self
    }
}

impl From<lvgl::lv_area_t> for Area {
    fn from(value: lvgl::lv_area_t) -> Self {
        Self::new(
            Point::new(value.x1 as i16, value.y1 as i16),
            Point::new(value.x2 as i16, value.y2 as i16),
        )
    }
}

impl AsRef<[u8]> for Area {
    fn as_ref(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self as *const _ as *const u8, core::mem::size_of::<Self>())
        }
    }
}

impl AsRef<Area> for [u8; core::mem::size_of::<Area>()] {
    fn as_ref(&self) -> &Area {
        unsafe { &*(self as *const _ as *const Area) }
    }
}
