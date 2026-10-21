"""Linux local-channel checks; no SIG client or production files."""
import importlib.util
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest

spec = importlib.util.spec_from_file_location('coordinator', Path(__file__).with_name('enable-recurring-reviews-20261021.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class Boundary(unittest.TestCase):
    def setup_root(self, directory):
        root = Path(directory)
        folder = root / 'fundamental-reload'
        folder.mkdir()
        status = dict(workerPid=os.getpid(), workerStartTicks=int(Path('/proc/self/stat').read_text().rsplit(') ', 1)[1].split()[19]), previousGenerationSha256='a' * 64)
        (folder / 'status.json').write_text(json.dumps(status))
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(folder / 'owner.sock'))
        server.listen()
        self.addCleanup(server.close)
        return root, status, server

    def test_no_request_bytes_before_owner_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            root, status, server = self.setup_root(directory)
            observed, errors = [], []
            def owner():
                try:
                    with server.accept()[0] as peer:
                        peer.settimeout(.04)
                        with self.assertRaises(TimeoutError):
                            peer.recv(1)
                        path = root / 'fundamental-reload/status.json'
                        next_path = path.with_suffix('.next')
                        next_path.write_text(json.dumps(status))
                        next_path.replace(path)
                        peer.settimeout(.2)
                        observed.append(peer.recv(16))
                except BaseException as error:
                    errors.append(error)
            thread = threading.Thread(target=owner)
            thread.start()
            with module.connect_at_boundary(root, status, status, timeout=2) as channel:
                channel.sendall(b'fresh-packet')
            thread.join(2)
            self.assertFalse(thread.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual(observed, [b'fresh-packet'])

    def test_wrong_peer_does_not_send_request(self):
        with tempfile.TemporaryDirectory() as directory:
            root, status, server = self.setup_root(directory)
            with self.assertRaises(AssertionError):
                module.connect_at_boundary(root, dict(status, workerPid=os.getpid() + 1), status, timeout=.1)
            with server.accept()[0] as peer:
                self.assertEqual(peer.recv(1), b'')

    def test_stop_before_boundary_does_not_send_request(self):
        with tempfile.TemporaryDirectory() as directory:
            root, status, server = self.setup_root(directory)
            (root / 'EXECUTION_STOP').write_text('fixture')
            with self.assertRaises(AssertionError):
                module.connect_at_boundary(root, status, status, timeout=.1)
            with server.accept()[0] as peer:
                self.assertEqual(peer.recv(1), b'')


if __name__ == '__main__':
    unittest.main()
