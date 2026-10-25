use alloc::{vec, vec::Vec};

pub const WIDTH: usize = 160;
pub const HEIGHT: usize = 100;
pub const PIXELS: usize = WIDTH * HEIGHT;
pub const FILE_SIZE: usize = 8 + PIXELS;
// White also serves as the eraser. Palette indexes are the on-disk pixel format.
pub const PALETTE: [u32; 8] = [
    0xffffff, 0x202124, 0xe53935, 0x1976d2, 0x388e3c, 0xffb300, 0x8e24aa, 0x795548,
];
const HEADER: [u8; 8] = [b'X', b'P', b'N', b'T', 1, WIDTH as u8, HEIGHT as u8, 0];

pub struct Drawing {
    pixels: Vec<u8>,
    previous: Vec<u8>,
    undo_available: bool,
    dirty: bool,
    previous_dirty: bool,
    last: Option<(i32, i32)>,
}

impl Default for Drawing {
    fn default() -> Self {
        Self::new()
    }
}

impl Drawing {
    pub fn new() -> Self {
        Self {
            pixels: vec![0; PIXELS],
            previous: vec![0; PIXELS],
            undo_available: false,
            dirty: false,
            previous_dirty: false,
            last: None,
        }
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn snapshot(&mut self) {
        self.previous.copy_from_slice(&self.pixels);
        self.previous_dirty = self.dirty;
        self.undo_available = true;
    }

    pub fn begin_stroke(&mut self, x: i32, y: i32, color: u8, diameter: u8) {
        if !Self::contains(x, y) {
            return;
        }
        self.snapshot();
        self.last = Some((x, y));
        self.continue_stroke(x, y, color, diameter);
    }

    fn contains(x: i32, y: i32) -> bool {
        (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y)
    }

    pub fn continue_stroke(&mut self, x: i32, y: i32, color: u8, diameter: u8) {
        // Validate before rasterization so arbitrary coordinates cannot produce unbounded work.
        if !Self::contains(x, y) || color as usize >= PALETTE.len() {
            self.last = None;
            return;
        }
        let Some((mut from_x, mut from_y)) = self.last else {
            return;
        };
        let dx = (x - from_x).abs();
        let dy = -(y - from_y).abs();
        let sx = if from_x < x { 1 } else { -1 };
        let sy = if from_y < y { 1 } else { -1 };
        let mut error = dx + dy;
        let radius = i32::from(diameter.clamp(1, 5)) / 2;
        loop {
            for py in (from_y - radius).max(0)..=(from_y + radius).min(HEIGHT as i32 - 1) {
                for px in (from_x - radius).max(0)..=(from_x + radius).min(WIDTH as i32 - 1) {
                    let pixel = &mut self.pixels[py as usize * WIDTH + px as usize];
                    self.dirty |= *pixel != color;
                    *pixel = color;
                }
            }
            if from_x == x && from_y == y {
                break;
            }
            let twice_error = error * 2;
            if twice_error >= dy {
                error += dy;
                from_x += sx;
            }
            if twice_error <= dx {
                error += dx;
                from_y += sy;
            }
        }
        self.last = Some((x, y));
    }

    pub fn end_stroke(&mut self) {
        self.last = None;
    }

    pub fn clear(&mut self) {
        self.end_stroke();
        self.snapshot();
        self.dirty |= self.pixels.iter().any(|&pixel| pixel != 0);
        self.pixels.fill(0);
    }

    pub fn undo(&mut self) -> bool {
        self.end_stroke();
        if !self.undo_available {
            return false;
        }
        core::mem::swap(&mut self.pixels, &mut self.previous);
        self.dirty = self.previous_dirty;
        self.undo_available = false;
        true
    }

    pub fn mark_saved(&mut self) {
        // If undo is retained after saving, its content differs from the saved image.
        self.previous_dirty = self.previous != self.pixels;
        self.dirty = false;
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(FILE_SIZE);
        bytes.extend_from_slice(&HEADER);
        bytes.extend_from_slice(&self.pixels);
        bytes
    }

    pub fn replace_from_bytes(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.len() != FILE_SIZE || bytes[..8] != HEADER {
            return Err("Invalid Pixel Painter file");
        }
        if bytes[8..]
            .iter()
            .any(|&pixel| pixel as usize >= PALETTE.len())
        {
            return Err("Invalid palette index");
        }
        self.end_stroke();
        self.pixels.copy_from_slice(&bytes[8..]);
        self.undo_available = false;
        self.dirty = false;
        Ok(())
    }
}

/// Letterboxed viewport derived from the current LVGL content area, including rotation/resizing.
#[derive(Clone, Copy)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Viewport {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        let width = width.max(0);
        let height = height.max(0);
        let fitted_width =
            i64::from(width).min(i64::from(height) * WIDTH as i64 / HEIGHT as i64) as i32;
        let fitted_height =
            i64::from(height).min(i64::from(width) * HEIGHT as i64 / WIDTH as i64) as i32;
        Self {
            x: x + (width - fitted_width) / 2,
            y: y + (height - fitted_height) / 2,
            width: fitted_width,
            height: fitted_height,
        }
    }

    pub fn map(self, x: i32, y: i32) -> Option<(i32, i32)> {
        let dx = i64::from(x) - i64::from(self.x);
        let dy = i64::from(y) - i64::from(self.y);
        if self.width <= 0
            || self.height <= 0
            || dx < 0
            || dy < 0
            || dx >= i64::from(self.width)
            || dy >= i64::from(self.height)
        {
            return None;
        }
        Some((
            (dx * WIDTH as i64 / i64::from(self.width)) as i32,
            (dy * HEIGHT as i64 / i64::from(self.height)) as i32,
        ))
    }
}
