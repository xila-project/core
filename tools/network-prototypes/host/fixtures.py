"""Local-only echo fixtures and client probe; no external traffic."""
import socket
import subprocess
import sys
import threading

listener = socket.socket()
listener.bind(("127.0.0.1", 0))
listener.listen()
udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
udp.bind(("127.0.0.1", 0))


def tcp_echo():
    connection, _ = listener.accept()
    with connection:
        while True:
            data = connection.recv(4096)
            if not data:
                return
            connection.sendall(data)


def udp_echo():
    while True:
        data, peer = udp.recvfrom(65535)
        udp.sendto(data, peer)


threading.Thread(target=tcp_echo, daemon=True).start()
threading.Thread(target=udp_echo, daemon=True).start()
result = subprocess.run([sys.argv[1], str(listener.getsockname()[1]), str(udp.getsockname()[1])], timeout=20, check=False)
sys.exit(result.returncode)
