use std::path::Path;

pub use graphics::ScreenshotDimensions;

use super::{FramebufferSnapshot, framebuffer_snapshot};

pub async fn write_png(path: &Path) -> Result<ScreenshotDimensions, String> {
    if path.as_os_str().is_empty() {
        return Err("Screenshot path is empty.".to_string());
    }

    let snapshot = framebuffer_snapshot().await?;
    let bytes = encode_png(&snapshot)?;
    std::fs::write(path, bytes).map_err(|error| format!("Screenshot write failed: {error}"))?;
    Ok(ScreenshotDimensions {
        width: snapshot.width,
        height: snapshot.height,
    })
}

pub fn encode_png(snapshot: &FramebufferSnapshot) -> Result<Vec<u8>, String> {
    let expected = (snapshot.width as usize)
        .checked_mul(snapshot.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Framebuffer dimensions overflow.".to_string())?;
    if snapshot.width == 0 || snapshot.height == 0 || snapshot.rgba.len() != expected {
        return Err("Framebuffer dimensions do not match pixel storage.".to_string());
    }

    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, snapshot.width, snapshot.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("PNG header failed: {error}"))?;
        writer
            .write_image_data(&snapshot.rgba)
            .map_err(|error| format!("PNG encoding failed: {error}"))?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_has_expected_dimensions_and_rgba_data() {
        let snapshot = FramebufferSnapshot {
            width: 1,
            height: 1,
            rgba: alloc::vec![0x12, 0x34, 0x56, 0x78],
        };
        let bytes = encode_png(&snapshot).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = alloc::vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (1, 1));
        assert_eq!(&pixels[..4], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn png_rejects_inconsistent_snapshot() {
        let snapshot = FramebufferSnapshot {
            width: 2,
            height: 1,
            rgba: alloc::vec![0; 4],
        };
        assert!(encode_png(&snapshot).is_err());
    }
}
