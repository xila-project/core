use xila_http::{BodyDecoder, parse_response_head};

fn decode(wire: &[u8], split: usize, head: bool) -> Result<Vec<u8>, xila_http::Error> {
    let mut buffer = Vec::new();
    let mut decoder = None;
    let mut out = Vec::new();
    for part in [&wire[..split], &wire[split..]] {
        buffer.extend_from_slice(part);
        if decoder.is_none() {
            while let Some(parsed) = parse_response_head::<32>(&buffer, head)? {
                let len = parsed.length;
                let status = parsed.status;
                let body = parsed.body;
                buffer.drain(..len);
                if (100..200).contains(&status) && status != 101 {
                    continue;
                }
                decoder = Some(BodyDecoder::new(body));
                break;
            }
        }
        if let Some(decoder) = decoder.as_mut() {
            let n = decoder.decode(&mut buffer)?;
            out.extend_from_slice(&buffer[..n]);
            buffer.clear();
        }
    }
    decoder.unwrap().finish()?;
    Ok(out)
}

#[test]
fn every_split() {
    for (wire, expected) in [
        (b"HTTP/1.1 200\r\ncOnTeNt-LeNgTh: 5\r\n\r\na\0bc\0".as_slice(), b"a\0bc\0".as_slice()),
        (b"HTTP/1.0 200 OK\r\n\r\nabcdef", b"abcdef"),
        (b"HTTP/1.1 103 Early Hints\r\nLink: x\r\n\r\nHTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 999\r\n\r\nA;foo=bar\r\n0123456789\r\n1\r\nx\r\n0\r\nX-Trailer: ignored\r\n\r\n", b"0123456789x"),
        (b"HTTP/1.1 204 No Content\r\nContent-Length: 100\r\n\r\n", b""),
    ] {
        for split in 0..=wire.len() { assert_eq!(decode(wire, split, false).unwrap(), expected, "split {split}"); }
    }
}

#[test]
fn truncated_and_ambiguous() {
    for wire in [
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\na".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\n",
    ] {
        for split in 0..=wire.len() {
            assert!(decode(wire, split, false).is_err());
        }
    }
    assert!(
        parse_response_head::<32>(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
            false
        )
        .is_err()
    );
    assert!(
        decode(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n", 5, true)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn one_byte_at_a_time() {
    let wire = b"A;extension=ignored\r\n0123456789\r\n0\r\nTrailer: value\r\n\r\n";
    let mut decoder = BodyDecoder::new(xila_http::BodyKind::Chunked);
    let mut result = Vec::new();
    for byte in wire {
        let mut byte = [*byte];
        let n = decoder.decode(&mut byte).unwrap();
        result.extend_from_slice(&byte[..n]);
    }
    decoder.finish().unwrap();
    assert_eq!(result, b"0123456789");
}

#[test]
fn builders_and_line_endings() {
    let mut storage = [0; 256];
    let mut response = xila_http::HttpResponseBuilder::from_buffer(&mut storage);
    response.add_status_code(200).unwrap();
    response.add_header("X", b"y").unwrap();
    response.add_line(b"").unwrap();
    response.add_chunk(b"0123456789").unwrap();
    response.finish_chunks().unwrap();
    let n = response.get_position();
    assert_eq!(
        &storage[..n],
        b"HTTP/1.1 200 \r\nX: y\r\n\r\na\r\n0123456789\r\n0\r\n\r\n"
    );
    assert_eq!(
        xila_http::get_body(b"HTTP/1.0 200\nX: y\n\nbody"),
        Some(b"body".as_slice())
    );
}

#[test]
fn framing_errors_and_no_body_precedence() {
    for wire in [
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked, gzip\r\n\r\n".as_slice(),
        b"HTTP/1.1 200 OK\r\nContent-Length: +5\r\n\r\n",
        b"HTTP/1.1 101 Switching Protocols\r\n\r\n",
    ] {
        assert!(parse_response_head::<32>(wire, false).is_err());
    }
    assert!(parse_response_head::<1>(b"HTTP/1.1 200\r\nX: a\r\nY: b\r\n\r\n", false).is_err());
    assert_eq!(
        parse_response_head::<32>(b"HTTP/1.1 304\r\nContent-Length: garbage\r\n\r\n", false)
            .unwrap()
            .unwrap()
            .body,
        xila_http::BodyKind::Empty
    );
    for wire in [b"g\r\n".as_slice(), b"1\r\naX\n", b"10000000000000000\r\n"] {
        let mut decoder = BodyDecoder::new(xila_http::BodyKind::Chunked);
        assert!(decoder.decode(&mut wire.to_vec()).is_err());
    }
}

#[test]
#[cfg(feature = "alloc")]
fn url_has_no_trailing_separator() {
    assert_eq!(
        xila_http::format_url("https", "example.com", [("a", "1"), ("b", "2")].into_iter()),
        "https://example.com?a=1&b=2"
    );
}

#[test]
fn chunk_extension_whitespace() {
    let mut decoder = BodyDecoder::new(xila_http::BodyKind::Chunked);
    let mut body = b"1 \t;foo=bar\r\na\r\n0\r\n\r\n".to_vec();
    assert_eq!(decoder.decode(&mut body).unwrap(), 1);
    assert_eq!(body[0], b'a');
    decoder.finish().unwrap();
}
