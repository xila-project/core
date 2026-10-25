pub use debug_protocol::{
    InputState, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, MAX_TEXT_BYTES, ProtocolError, Request,
};

pub fn parse_request(bytes: &[u8]) -> Result<Request, ProtocolError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(ProtocolError::RequestTooLarge);
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|_| ProtocolError::InvalidJson)?;
    debug_protocol::validate_request(&request)?;
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_accepts_requests_and_rejects_bad_payloads() {
        assert_eq!(
            parse_request(br#"{"op":"pointer_move","x":4,"y":9}"#),
            Ok(Request::PointerMove { x: 4, y: 9 })
        );
        assert_eq!(
            parse_request(b"not json").unwrap_err(),
            ProtocolError::InvalidJson
        );
        assert_eq!(
            parse_request(&alloc::vec![b' '; MAX_REQUEST_BYTES + 1]).unwrap_err(),
            ProtocolError::RequestTooLarge
        );
    }

    #[test]
    fn parser_bounds_text_and_requires_explicit_screenshot_path() {
        let long_text = alloc::format!(
            "{{\"op\":\"text\",\"text\":\"{}\"}}",
            "a".repeat(MAX_TEXT_BYTES + 1)
        );
        assert_eq!(
            parse_request(long_text.as_bytes()).unwrap_err(),
            ProtocolError::TextTooLarge
        );
        assert_eq!(
            parse_request(br#"{"op":"screenshot","path":""}"#).unwrap_err(),
            ProtocolError::EmptyScreenshotPath
        );
    }
}
