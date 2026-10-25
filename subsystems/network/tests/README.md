# Local HTTPS interoperability test

Run `cargo test -p network --test https -- --ignored --nocapture` with Python 3
(including its `ssl` module) and OpenSSL installed. The test is ignored by default
because it launches these external tools. It generates an ephemeral ECDSA
certificate and starts a local TLS 1.3 server; the child process and certificate
files are cleaned up on exit.

The client is the production `HttpsClientDevice`, including request worker,
`network::Manager`, smoltcp TCP, embedded-tls and the VFS polling interface. A
byte relay bridges the isolated Xila loopback TCP connection to the host's
loopback TLS server. No TLS termination occurs in the relay and no external
network access is required.

Cases cover Content-Length, chunked framing/extensions/trailers, close-delimited
responses, an informational head followed by a final head, a 256 KiB response sent in full 16 KiB plaintext TLS records,
and truncated Content-Length/chunked bodies. Framed responses keep the server
connection open until the client closes, checking completion without EOF.

The test uses the existing TLS provider configuration and checks handshake,
transport and framing. It does not establish certificate authentication,
public-server compatibility or embedded hardware correctness.
