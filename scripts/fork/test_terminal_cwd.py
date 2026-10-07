"""用已构建 CLI 验证启动和 resume 的 OSC 7 本机目录契约，不访问真实凭据或模型。"""

import fcntl
import json
import os
import pty
import re
import select
import struct
import subprocess
import tempfile
import termios
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse


class MockResponses(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{"data":[]}')

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", "0")))
        events = [
            {"type": "response.created", "response": {"id": "cwd-test"}},
            {
                "type": "response.output_item.done",
                "item": {
                    "type": "message",
                    "role": "assistant",
                    "id": "cwd-message",
                    "content": [{"type": "output_text", "text": "CWD_TEST_SAVED"}],
                },
            },
            {
                "type": "response.completed",
                "response": {
                    "id": "cwd-test",
                    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
                },
            },
        ]
        body = "".join(
            f"event: {event['type']}\ndata: {json.dumps(event)}\n\n" for event in events
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class TerminalProcess:
    def __init__(self, command, cwd, env):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
        try:
            self.process = subprocess.Popen(
                command,
                cwd=cwd,
                env=env,
                stdin=slave,
                stdout=slave,
                stderr=slave,
                start_new_session=True,
            )
        except BaseException:
            os.close(self.master)
            raise
        finally:
            os.close(slave)
        self.output = bytearray()
        self.answered = {}

    def wait_for(self, predicate, timeout=25):
        deadline = time.monotonic() + timeout
        while not predicate() and time.monotonic() < deadline:
            if self.process.poll() is not None:
                break
            if not select.select([self.master], [], [], 0.1)[0]:
                continue
            try:
                self.output.extend(os.read(self.master, 65536))
            except OSError:
                break
            for query, answer in [
                (b"\x1b[6n", b"\x1b[1;1R"),
                (b"\x1b[?u", b"\x1b[?7u\x1b[?1;2c"),
                (b"\x1b]11;?", b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
            ]:
                count = self.output.count(query)
                if count > self.answered.get(query, 0):
                    os.write(self.master, answer)
                    self.answered[query] = count
        if not predicate():
            raise AssertionError("CLI 未完成隔离的目录验收场景")

    def uris(self):
        return [
            item.decode()
            for item in re.findall(
                rb"\x1b\]7;([^\x1b\x07]*)(?:\x1b\\|\x07)", self.output
            )
        ]

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()
        os.close(self.master)


@unittest.skipUnless(
    os.environ.get("CODEX_FORK_TEST_BINARY"), "需要已经构建的 fork CLI"
)
class TerminalCwdTests(unittest.TestCase):
    def test_startup_and_resume_use_local_authority_and_saved_directory(self):
        binary = str(Path(os.environ["CODEX_FORK_TEST_BINARY"]).resolve())
        server = ThreadingHTTPServer(("127.0.0.1", 0), MockResponses)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        with tempfile.TemporaryDirectory(prefix="fork-terminal-cwd-") as temporary:
            root = Path(temporary).resolve()
            home, saved, launch = root / "home", root / "中文 session", root / "launch"
            for directory in (home, saved, launch):
                directory.mkdir()
            (home / "config.toml").write_text(
                'model = "gpt-6-sol"\nmodel_provider = "test"\n'
                "analytics.enabled = false\nfeatures.daemon_auto_start = false\n"
                "features.shell_snapshot = false\n"
                'tui.resume_cwd = "session"\n'
                '[model_providers.test]\nname = "Mock"\nwire_api = "responses"\n'
                "requires_openai_auth = false\nsupports_websockets = false\n"
                f'base_url = "http://127.0.0.1:{server.server_port}/v1"\n'
                f"[projects.{json.dumps(str(saved), ensure_ascii=False)}]\n"
                'trust_level = "trusted"\n'
            )
            env = {**os.environ, "CODEX_HOME": str(home), "TERM": "xterm-256color"}
            for key in ("OPENAI_API_KEY", "TMUX", "STY"):
                env.pop(key, None)
            options = [binary, "--no-daemon", "--no-alt-screen"]
            initial = TerminalProcess(
                options + ["-C", str(saved), "Save cwd history"], launch, env
            )
            try:
                initial.wait_for(lambda: bool(initial.uris()))
                self.assert_local_cwd(initial, saved)
                initial.wait_for(lambda: b"CWD_TEST_SAVED" in initial.output)
                initial.wait_for(
                    lambda: bool(list((home / "sessions").rglob("*.jsonl")))
                )
                rollout = next((home / "sessions").rglob("*.jsonl"))
                thread_id = json.loads(rollout.read_text().splitlines()[0])["payload"][
                    "id"
                ]
            finally:
                initial.close()
            # 使用另一启动目录并显式采用保存目录；非空 prompt 跳过升级提示，只调用 mock。
            resumed = TerminalProcess(
                options + ["resume", thread_id, "Verify resumed cwd"], launch, env
            )
            try:
                resumed.wait_for(lambda: bool(resumed.uris()))
                self.assert_local_cwd(resumed, saved)
            finally:
                resumed.close()

    def assert_local_cwd(self, process, expected):
        uri = urlparse(process.uris()[-1])
        self.assertEqual(uri.scheme, "file")
        self.assertEqual(
            uri.hostname, "localhost", "Ghostty 必须能接受 OSC 7 的本机 authority"
        )
        self.assertEqual(unquote(uri.path), str(expected))


if __name__ == "__main__":
    unittest.main()
