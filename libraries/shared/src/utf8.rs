pub fn utf8_chunks(mut bytes: &[u8]) -> impl Iterator<Item = &str> {
    core::iter::from_fn(move || {
        if bytes.is_empty() {
            return None;
        }

        match str::from_utf8(bytes) {
            Ok(s) => {
                // whole remaining slice is valid
                bytes = &[];
                Some(s)
            }
            Err(e) => {
                let valid = e.valid_up_to();

                if valid > 0 {
                    // return the valid prefix
                    let s = unsafe {
                        // safe because `valid` bytes were confirmed valid UTF-8
                        str::from_utf8_unchecked(&bytes[..valid])
                    };
                    bytes = &bytes[e.error_len().map_or(valid + 1, |len| valid + len)..];
                    Some(s)
                } else {
                    // skip the invalid byte
                    bytes = &bytes[1..];
                    // continue the iterator until we find a valid chunk
                    // (returning None here would stop early)
                    Some("") // or continue looping: but iterators can't loop
                }
            }
        }
    })
}
