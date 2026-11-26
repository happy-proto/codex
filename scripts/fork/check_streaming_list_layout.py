#!/usr/bin/env -S uv run --script
#
# /// script
# requires-python = ">=3.15"
# dependencies = [
#     "pyte>=0.8.2",
#     "typer>=0.29.2",
# ]
# ///


"""以真实 CLI、PTY 和隔离 Responses 服务验证流式列表布局。"""

import errno
import fcntl
import json
import os
import pty
import select
import signal
import struct
import tempfile
import termios
import threading
import time
from pathlib import Path
from typing import Annotated

import pyte
import typer
from test_terminal_cwd import LoopbackHTTPServer, MockResponses

WIDTH = 80
HEIGHT = 32
FIRST = "Layout beginning.\n\n- First short item."
LAST = "\n- Second short item.\n"
END = "\nLayout finished.\n"
RELEASE_LONG = threading.Event()
RELEASE_END = threading.Event()
RELEASE_DONE = threading.Event()


class Mock(MockResponses):
    def do_POST(self):
        """按终端画面捕获进度释放模型响应。"""
        body = self.rfile.read(int(self.headers["Content-Length"])).decode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()

        def emit(kind, **payload):
            self.wfile.write(
                ("data: " + json.dumps(dict(type=kind, **payload)) + "\n\n").encode()
            )
            self.wfile.flush()

        def item(text):
            return {
                "type": "message",
                "role": "assistant",
                "id": "answer",
                "content": [{"type": "output_text", "text": text}],
            }

        emit("response.created", response={"id": "resp-layout"})
        if "Layout probe" in body and 'thread_source\\":\\"system' not in body:
            emit("response.output_item.added", item=item(""))
            emit("response.output_text.delta", delta=FIRST)
            assert RELEASE_LONG.wait(30)
            emit("response.output_text.delta", delta=LAST)
            assert RELEASE_END.wait(30)
            emit("response.output_text.delta", delta=END)
            assert RELEASE_DONE.wait(30)
            emit("response.output_item.done", item=item(FIRST + LAST + END))
        else:
            emit("response.output_item.done", item=item('{"title":"Layout probe"}'))
        emit(
            "response.completed",
            response={
                "id": "resp-layout",
                "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0},
            },
        )

    def do_GET(self):
        self.send_error(404)


def probe(binary: Path) -> None:
    """通过终端画面对照首个列表项的三个阶段。"""
    server = LoopbackHTTPServer(("127.0.0.1", 0), Mock)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="codex-stream-layout-") as root:
        root = Path(root)
        workspace = root / "workspace"
        workspace.mkdir()
        (root / "auth.json").write_text('{"OPENAI_API_KEY":"local-layout-test"}')
        (root / "config.toml").write_text(f"""
model = "gpt-5.6-terra"
model_provider = "layout_mock"
check_for_update_on_startup = false
suppress_unstable_features_warning = true
analytics.enabled = false
features.daemon_auto_start = false
notice.model_migrations."gpt-5.6-terra" = "gpt-6-sol"
[model_providers.layout_mock]
name = "Layout mock"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
[tui]
fullscreen_transcript = true
alternate_screen = "always"
[projects."{workspace}"]
trust_level = "trusted"
""")
        binary = str(Path(binary).resolve())
        pid, fd = pty.fork()
        if pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", HEIGHT, WIDTH, 0, 0))
            os.chdir(workspace)
            env = {
                key: os.environ[key]
                for key in ("PATH", "HOME", "LANG", "TMPDIR")
                if key in os.environ
            }
            env.update(CODEX_HOME=str(root), TERM="xterm-256color")
            os.execve(binary, [binary, "--no-daemon", "Layout probe"], env)
        screen = pyte.Screen(WIDTH, HEIGHT)
        stream = pyte.ByteStream(screen)
        try:

            def pump(seconds):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    if select.select(
                        [fd], [], [], min(0.05, max(0, deadline - time.monotonic()))
                    )[0]:
                        try:
                            data = os.read(fd, 65536)
                        except OSError as error:
                            if error.errno == errno.EIO:
                                break
                            raise
                        stream.feed(data)

            def capture(marker):
                deadline = time.monotonic() + 35
                while not any(marker in line for line in screen.display):
                    assert time.monotonic() < deadline, "\n".join(screen.display)
                    pump(0.1)
                pump(0.5)  # Drain remaining writes from the terminal frame.
                return screen.display.copy()

            partial = capture("First short item.")
            RELEASE_LONG.set()
            newline = capture("Second short item.")
            RELEASE_END.set()
            capture("Layout finished.")
            RELEASE_DONE.set()
            completed = capture("Worked for")

            def paragraph_and_first_item(frame):
                start = next(
                    i for i, line in enumerate(frame) if "Layout beginning." in line
                )
                end = next(
                    i for i, line in enumerate(frame) if "First short item." in line
                )
                return [line.rstrip() for line in frame[start : end + 1]]

            frames = [
                paragraph_and_first_item(frame)
                for frame in (partial, newline, completed)
            ]
            for stage, frame in zip(
                ("partial", "newline", "completed"), frames, strict=True
            ):
                print(f"{stage}: {frame!r}")
            assert frames[0] == frames[1] == frames[2], "首个列表项在收到换行后发生位移"
        finally:
            RELEASE_LONG.set()
            RELEASE_END.set()
            RELEASE_DONE.set()
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            os.waitpid(pid, 0)
            os.close(fd)
            server.shutdown()
            server.server_close()


def main(
    binary: Annotated[
        Path,
        typer.Argument(
            exists=True, file_okay=True, dir_okay=False, help="已构建的 Codex CLI"
        ),
    ],
) -> None:
    """验证首个列表项在收到换行及回复完成时保持原位。"""
    probe(binary)


if __name__ == "__main__":
    app = typer.Typer(pretty_exceptions_enable=False)
    app.command()(main)
    app()
