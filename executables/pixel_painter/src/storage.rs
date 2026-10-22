use pixel_painter::drawing::FILE_SIZE;
use std::{
    fs::{self},
    io::{self, Read, Write},
};

#[cfg(target_arch = "wasm32")]
pub fn default_path() -> Option<String> {
    std::env::var("XILA_APPLICATION_DATA")
        .ok()
        .map(|directory| format!("{directory}/drawing.xpnt"))
}

pub fn read(path: &str) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(FILE_SIZE + 1);
    fs::File::open(path)?
        .take((FILE_SIZE + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn write(path: &str, bytes: &[u8]) -> io::Result<()> {
    // WASI currently has no rename/sync support. Report failures and retain the
    // in-memory drawing; successful flush is not a power-loss durability guarantee.
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixel_painter::drawing::Drawing;

    #[test]
    fn storage_round_trip_and_bounded_read() {
        let directory = std::env::temp_dir().join(format!("xila-painter-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("drawing.xpnt");
        let path = path.to_str().unwrap();
        let mut drawing = Drawing::new();
        drawing.begin_stroke(20, 30, 3, 5);
        drawing.end_stroke();
        write(path, &drawing.encode()).unwrap();
        let bytes = read(path).unwrap();
        let mut loaded = Drawing::new();
        loaded.replace_from_bytes(&bytes).unwrap();
        assert_eq!(drawing.pixels(), loaded.pixels());
        fs::write(path, vec![0; FILE_SIZE * 2]).unwrap();
        assert_eq!(read(path).unwrap().len(), FILE_SIZE + 1);
        assert!(write(directory.join("missing/file").to_str().unwrap(), &bytes).is_err());
        assert!(drawing.is_dirty());
        fs::remove_dir_all(directory).unwrap();
    }
}
