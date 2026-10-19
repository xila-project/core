use file_system::{
    DirectBaseOperations, DirectCharacterDevice, Error, MountOperations, Result, Size,
};
use std::collections::BTreeMap;

const EMBEDDED_FIELDS: [&str; 6] = [
    "processor",
    "vendor_id",
    "model name",
    "cpu MHz",
    "cpu cores",
    "architecture",
];

pub struct CpuInformationsDevice;

fn parse_proc_cpuinfo(content: &str) -> Vec<BTreeMap<String, String>> {
    let mut entries = Vec::new();
    let mut current = BTreeMap::new();

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            if !current.is_empty() {
                entries.push(current);
                current = BTreeMap::new();
            }
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            current.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    if !current.is_empty() {
        entries.push(current);
    }

    entries
}

fn format_entry(
    entry: &BTreeMap<String, String>,
    index: usize,
    architecture: &str,
    fallback_cores: &str,
) -> String {
    let mut block = String::new();

    for key in EMBEDDED_FIELDS {
        let value = match key {
            "processor" => entry.get(key).cloned().unwrap_or_else(|| index.to_string()),
            "architecture" => architecture.to_string(),
            "cpu cores" => entry
                .get(key)
                .cloned()
                .unwrap_or_else(|| fallback_cores.to_string()),
            _ => entry
                .get(key)
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
        };

        block.push_str(key);
        block.push_str("\t: ");
        block.push_str(&value);
        block.push('\n');
    }

    block.push('\n');
    block
}

fn build_cpuinfo_text_from_proc(
    content: &str,
    architecture: &str,
    available_cores: Option<usize>,
) -> String {
    let parsed = parse_proc_cpuinfo(content);
    let fallback_cores = available_cores
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    if parsed.is_empty() {
        return format_entry(&BTreeMap::new(), 0, architecture, &fallback_cores);
    }

    let mut output = String::new();
    for (index, entry) in parsed.iter().enumerate() {
        output.push_str(&format_entry(entry, index, architecture, &fallback_cores));
    }

    output
}

fn render_cpuinfo_text() -> String {
    let architecture = std::env::consts::ARCH;
    let cores = std::thread::available_parallelism().ok().map(usize::from);

    if cfg!(target_os = "linux")
        && let Ok(content) = std::fs::read_to_string("/proc/cpuinfo")
    {
        return build_cpuinfo_text_from_proc(&content, architecture, cores);
    }

    build_cpuinfo_text_from_proc("", architecture, cores)
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
    fn parser_extracts_embedded_subset_fields() {
        let sample = "processor\t: 0\nvendor_id\t: ARM\nmodel name\t: Cortex-A53\ncpu MHz\t\t: 1200.0\ncpu cores\t: 4\n\n";
        let output = build_cpuinfo_text_from_proc(sample, "aarch64", Some(4));
        assert!(output.contains("processor\t: 0"));
        assert!(output.contains("vendor_id\t: ARM"));
        assert!(output.contains("model name\t: Cortex-A53"));
        assert!(output.contains("cpu MHz\t: 1200.0"));
        assert!(output.contains("cpu cores\t: 4"));
        assert!(output.contains("architecture\t: aarch64"));
    }

    #[test]
    fn parser_falls_back_to_unknown_for_missing_values() {
        let output = build_cpuinfo_text_from_proc("", "aarch64", None);
        assert!(output.contains("vendor_id\t: unknown"));
        assert!(output.contains("cpu MHz\t: unknown"));
        assert!(output.contains("cpu cores\t: unknown"));
    }

    #[test]
    fn read_uses_absolute_offset_and_handles_eof() {
        let device = CpuInformationsDevice;
        let mut buffer = [0u8; 32];
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
