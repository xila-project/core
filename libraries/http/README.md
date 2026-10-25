# xila_http

`no_std` HTTP/1 request writers and response framing with no allocation by
default. Enable `alloc` only for `format_url`; borrowed URL parsing is always
available. Rust 1.90 or later.

`parse_response_head::<N>` borrows contiguous caller storage. It returns `None`
for a partial head, and uses `N * size_of::<httparse::Header>()` stack bytes
while parsing. Bound the caller buffer separately; a partial result with a full
buffer means headers are too large. Informational responses must be skipped by
the caller before delivering the final response. Protocol upgrades are rejected.
Header field names and transfer coding names are matched without regard to case.

`BodyDecoder` consumes fragments of any size and compacts decoded payload in
place, returning the payload length. Its storage is constant (including an
18-byte chunk-size scratch area); call `finish` on EOF to detect truncation.
Chunk extensions and trailers are discarded. HEAD, 1xx, 204 and 304 take priority
over framing fields; Transfer-Encoding takes priority over Content-Length.
The caller supplies HEAD request context. Pipelining and protocol upgrades are
not supported. Transfer codings other than chunked and content encodings are
not decompressed.

The Xila VFS drivers publish the original response head, then **decoded body**
bytes on subsequent reads. Original Transfer-Encoding fields describe the wire
response, not framing remaining in the device body. Consumers must not decode
chunk framing a second time. Reads can split the head; collect until its blank
line. Each opening uses a single 2048-byte response buffer, shared between the
worker and reader with backpressure. HTTP message size does not increase that
allocation. Socket and TLS storage are separate; HTTPS keeps a 16 KiB + 256-byte receive record buffer and a 16 KiB write
record buffer on the heap while connected. Closing cancels the worker. Dropping its transport retires socket storage
for reclamation by the network runner after FIN/RST dispatch (abandoned shutdowns
are bounded to 15 seconds).

Validation: `cargo test -p xila_http --all-features` covers fragmentation,
truncation, framing precedence and allocation-free parsing/writing. Driver tests
exercise polling, buffer limits and peak heap for generated 4 KiB/4 MiB downloads.
