import socket
import sys
import threading
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from reliability_harness import DelayProxy


def free_port() -> int:
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])
    finally:
        sock.close()


class EchoServer:
    def __init__(self) -> None:
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen()
        self.sock.settimeout(0.2)
        self.port = int(self.sock.getsockname()[1])
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def close(self) -> None:
        self.stop.set()
        self.sock.close()
        self.thread.join(timeout=2)

    def _serve(self) -> None:
        while not self.stop.is_set():
            try:
                conn, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            threading.Thread(target=self._echo, args=(conn,), daemon=True).start()

    @staticmethod
    def _echo(conn: socket.socket) -> None:
        with conn:
            data = conn.recv(1024)
            if data:
                conn.sendall(data)
                time.sleep(0.2)


class DelayProxyTests(unittest.TestCase):
    def test_proxy_accepts_connection_after_idle_timeout(self) -> None:
        server = EchoServer()
        proxy = DelayProxy("test-proxy", free_port(), server.port, 0, 0, Path("test_logs/probe/delay-proxy-test.log"))
        try:
            proxy.start()
            time.sleep(0.8)
            with socket.create_connection(("127.0.0.1", proxy.listen_port), timeout=2) as client:
                client.settimeout(2)
                client.sendall(b"ping")
                self.assertEqual(client.recv(4), b"ping")
        finally:
            proxy.stop()
            server.close()


if __name__ == "__main__":
    unittest.main()
