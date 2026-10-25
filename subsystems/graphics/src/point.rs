use super::lvgl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i16,
    pub y: i16,
}

impl Point {
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }

    pub const fn get_x(&self) -> i16 {
        self.x
    }

    pub const fn get_y(&self) -> i16 {
        self.y
    }

    pub fn split(self) -> (i16, i16) {
        (self.x, self.y)
    }

    pub fn set_x(mut self, value: i16) -> Self {
        self.x = value;
        self
    }

    pub fn set_y(mut self, value: i16) -> Self {
        self.y = value;
        self
    }

    pub fn set(mut self, x: i16, y: i16) -> Self {
        self.x = x;
        self.y = y;
        self
    }

    pub fn get_distance(&self, other: Point) -> f32 {
        let x = (self.x - other.x) as f32;
        let y = (self.y - other.y) as f32;
        (x * x + y * y).sqrt()
    }

    pub const fn scale(mut self, factor: f64) -> Self {
        self.x = (self.x as f64 * factor).round() as i16;
        self.y = (self.y as f64 * factor).round() as i16;
        self
    }
}

impl From<(i16, i16)> for Point {
    fn from((x, y): (i16, i16)) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (i16, i16) {
    fn from(point: Point) -> Self {
        point.split()
    }
}

impl From<Point> for lvgl::lv_point_t {
    fn from(point: Point) -> Self {
        Self {
            x: point.x as i32,
            y: point.y as i32,
        }
    }
}
