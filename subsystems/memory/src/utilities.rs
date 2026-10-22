pub const fn round_to_page_size(size: usize, page_size: usize) -> usize {
    (size + page_size - 1) & !(page_size - 1) // Round up to the nearest page size
}
