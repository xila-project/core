"""Local TLS 1.3 fixtures for the ignored end-to-end HTTPS device test."""
import socket
import ssl
import sys
import threading

context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.minimum_version = ssl.TLSVersion.TLSv1_3
context.maximum_version = ssl.TLSVersion.TLSv1_3
context.load_cert_chain(sys.argv[1], sys.argv[2])
listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen()
print(listener.getsockname()[1], flush=True)


def serve(raw):
    try:
        with context.wrap_socket(raw, server_side=True) as connection:
            connection.settimeout(10)
            request = b""
            while b"\r\n\r\n" not in request:
                request += connection.recv(4096)
            path = request.split(b" ")[1]
            if path == b"/chunked":
                response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 999\r\n\r\n5;ext=yes\r\nhello\r\n6\r\n world\r\n0\r\nTrailer: ignored\r\n\r\n"
            elif path == b"/close":
                response = b"HTTP/1.0 200 OK\r\n\r\nhello world"
            elif path == b"/truncated":
                response = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort"
            elif path == b"/truncated-chunk":
                response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n"
            elif path == b"/large":
                connection.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 262144\r\n\r\n")
                for _ in range(16):
                    connection.sendall(b"x" * 16384)
                # Keep the connection open until the client completes framing.
                connection.settimeout(30)
                connection.recv(1)
                return
            elif path == b"/interim":
                response = b"HTTP/1.1 103 Early Hints\r\nLink: </test>\r\n\r\nHTTP/1.1 200\r\ncOnTeNt-LeNgTh: 11\r\n\r\nhello world"
            else:
                response = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\nhello world"
            for start in range(0, len(response), 7):
                connection.sendall(response[start:start + 7])
            if path in (b"/length", b"/chunked", b"/interim"):
                connection.settimeout(30)
                connection.recv(1)
            else:
                try:
                    connection.unwrap()  # Send TLS close_notify before TCP EOF.
                except (OSError, ssl.SSLError):
                    pass
    except (OSError, ssl.SSLError):
        pass  # Clients deliberately abort once framing completes.


while True:
    raw, _ = listener.accept()
    threading.Thread(target=serve, args=(raw,), daemon=True).start()
