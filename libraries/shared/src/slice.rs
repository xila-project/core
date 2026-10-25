pub fn align_slice_to<T>(data: &[u8]) -> Option<&[T]> {
    let (prefix, aligned, suffix) = unsafe { data.align_to::<T>() };

    if prefix.is_empty() && suffix.is_empty() {
        Some(aligned)
    } else {
        log::warning!(
            "Slice alignment failed: prefix size {}, suffix size {}",
            prefix.len(),
            suffix.len()
        );
        None
    }
}
