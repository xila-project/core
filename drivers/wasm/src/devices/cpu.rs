use alloc::string::{String, ToString};
use file_system::{
    DirectBaseOperations, DirectCharacterDevice, Error, MountOperations, Result, Size,
};

pub struct CpuInformationsDevice;

fn get_hardware_concurrency() -> Option<u32> {
    let window = web_sys::window()?;
    let navigator = window.navigator();
    let value = js_sys::Reflect::get(
        &navigator,
        &wasm_bindgen::JsValue::from_str("hardwareConcurrency"),
    )
    .ok()?;
    let value = value.as_f64()?;

    if !value.is_finite() || value <= 0.0 {
        return None;
    }

    Some(value as u32)
}

fn build_cpuinfo_text(architecture: &str, hardware_concurrency: Option<u32>) -> String {
    let cores = hardware_concurrency
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let mut content = String::new();
    content.push_str("processor\t: 0\n");
    content.push_str("vendor_id\t: unknown\n");
    content.push_str("model name\t: unknown\n");
    content.push_str("cpu MHz\t: unknown\n");
    content.push_str("cpu cores\t: ");
    content.push_str(&cores);
    content.push('\n');
    content.push_str("architecture\t: ");
    content.push_str(architecture);
    content.push_str("\n\n");
    content
}

fn render_cpuinfo_text() -> String {
    build_cpuinfo_text("wasm32", get_hardware_concurrency())
}

impl DirectBaseOperations for CpuInformationsDevice {
    fn read(
        &self,
        _cx: &mut core::task::Context<'_>,
        buffer: &mut [u8],
        absolute_position: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| {
            let content = render_cpuinfo_text();
            let bytes = content.as_bytes();

            let start = usize::try_from(absolute_position).unwrap_or(usize::MAX);
            if start >= bytes.len() {
                return Ok(0);
            }

            let length = buffer.len().min(bytes.len() - start);
            buffer[..length].copy_from_slice(&bytes[start..start + length]);

            Ok(length)
        })
    }

    fn write(
        &self,
        _cx: &mut core::task::Context<'_>,
        _: &[u8],
        _: Size,
    ) -> core::task::Poll<Result<usize>> {
        file_system::ready(|| Err(Error::UnsupportedOperation))
    }
}

impl MountOperations for CpuInformationsDevice {}

impl DirectCharacterDevice for CpuInformationsDevice {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_uses_unknown_for_missing_values() {
        let output = build_cpuinfo_text("wasm32", None);
        assert!(output.contains("vendor_id\t: unknown"));
        assert!(output.contains("model name\t: unknown"));
        assert!(output.contains("cpu MHz\t: unknown"));
        assert!(output.contains("cpu cores\t: unknown"));
        assert!(output.contains("architecture\t: wasm32"));
    }

    #[test]
    fn formatter_includes_detected_cores() {
        let output = build_cpuinfo_text("wasm32", Some(8));
        assert!(output.contains("cpu cores\t: 8"));
    }

    #[test]
    fn read_uses_absolute_offset_and_handles_eof() {
        let device = CpuInformationsDevice;
        let mut buffer = [0u8; 24];
        let first = device.read_sync(&mut buffer, 0).unwrap();
        assert!(first > 0);

        let eof = device.read_sync(&mut buffer, Size::MAX).unwrap();
        assert_eq!(eof, 0);
    }

    #[test]
    fn write_is_not_supported() {
        let device = CpuInformationsDevice;
        let result = device.write_sync(b"ignored", 0);
        assert!(matches!(result, Err(Error::UnsupportedOperation)));
    }
}
