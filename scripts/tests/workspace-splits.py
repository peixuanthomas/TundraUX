#!/usr/bin/env python3
"""Exercise incremental splits and app selection in an isolated real PTY."""
import fcntl
import importlib.util
import os
from pathlib import Path
import pty
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "shell_smoke", Path(__file__).resolve().parents[1] / "linux-shell-smoke.py"
)
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def main():
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-shell").resolve()
    cli = binary.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tundra-splits-") as temporary:
        env = dict(os.environ, TERM="xterm-256color", LC_ALL="C.UTF-8")
        for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
            folder = Path(temporary) / key.lower()
            folder.mkdir(mode=0o700)
            env[key] = str(folder)
        for key, value in [("automation.enabled", "true"), ("language", "en-US"), ("motion", "reduced"), ("icon-mode", "ascii")]:
            result = subprocess.run([str(cli), "config", "set", key, value], env=env, capture_output=True, timeout=15)
            assert result.returncode == 0, result.stderr
        master, slave = pty.openpty()
        before = termios.tcgetattr(slave)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 39, 217, 0, 0))
        os.set_blocking(master, False)
        output = bytearray()
        child = subprocess.Popen([str(binary)], env=env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)

        def send(value):
            smoke.write_events_while_draining_output(master, (value,), output, 5)

        def snapshot():
            return smoke.control_snapshot(master, output, child, cli, env, timeout=15)

        def until(predicate, timeout=15):
            deadline = time.monotonic() + timeout
            current = None
            while time.monotonic() < deadline:
                current = snapshot()
                if predicate(current):
                    return current
                smoke.read_available(master, output, 0.02)
            raise AssertionError(f"UI condition timed out: {current}")

        def find(frame, suffix):
            return next(node for node in frame["nodes"] if node["id"].endswith(suffix))

        def tap(frame, suffix):
            node = next(node for node in frame["nodes"] if node["id"].endswith(suffix) and node["actionable"])
            x, y = smoke.control_position(frame, node["id"])
            send(f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode())

        def titles(frame):
            return [node for node in frame["nodes"] if "workspace.title." in node["id"]]

        def chooser(frame):
            return any(node["id"].endswith("workspace.choose.query") for node in frame["nodes"])

        def choose(query, suffix):
            until(chooser)
            send(b"\x1b[200~" + query.encode() + b"\x1b[201~")
            current = until(lambda frame: any(node["id"].endswith(suffix) and node["actionable"] for node in frame["nodes"]))
            tap(current, suffix)
            return until(lambda frame: not chooser(frame))

        def resize(width, height, count):
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
            os.kill(child.pid, signal.SIGWINCH)
            return until(lambda frame: (frame["width"], frame["height"]) == (width, height) and len(titles(frame)) == count)

        try:
            if os.geteuid() == 0:
                assert smoke.wait_for_output(master, output, b"any other key cancels: ", child, 5)
                send(b"y")
            for _ in range(16):
                current = snapshot()
                if current["page"] == "Home" and not any(node["id"].startswith("modal/") for node in current["nodes"]):
                    break
                send(b"\x1b[21~" if current["page"] == "FirstRunSetup" else b"\r")
                smoke.read_available(master, output, 0.1)
            current = until(lambda frame: frame["page"] == "Home")
            launcher = next(node for node in current["nodes"] if "home.entry." in node["id"] and "Launcher" in node["name"])
            tap(current, launcher["id"])
            current = until(lambda frame: frame["page"] == "Launcher" and len(titles(frame)) == 1)
            source = titles(current)[0]["id"].rsplit(".", 1)[1]
            original = titles(current)[0]["rect"]
            assert not any("workspace.maximize." in node["id"] for node in current["nodes"])
            send(b"\x17v")
            current = until(chooser)
            assert find(current, "workspace.choose.query")["rect"]["x"] > 108
            send(b"\x1b")
            current = until(lambda frame: not chooser(frame) and len(titles(frame)) == 1)
            assert find(current, f"workspace.title.{source}")["rect"] == original
            print("PASS: Ctrl+W,v opens the right-hand selector and Escape restores the original pane")

            send(b"\x17v")
            current = choose("Settings", "application.settings")
            current = until(lambda frame: len(titles(frame)) == 2 and frame["page"] == "Settings")
            print("PASS: a searched application starts as an independent second pane")
            settings = next(node["id"].rsplit(".", 1)[1] for node in titles(current) if node["id"] != f"workspace.title.{source}" and not node["id"].endswith(f"workspace.title.{source}"))
            left = find(current, f"workspace.title.{source}")["rect"]
            tap(current, f"workspace.split.{settings}")
            current = until(lambda frame: any(node["id"].endswith("workspace.left-right") for node in frame["nodes"]))
            assert not find(current, "workspace.left-right")["enabled"]
            assert find(current, "workspace.top-bottom")["enabled"]
            print("PASS: the split menu disables the repeated axis")
            send(b"\x1b")
            until(lambda frame: not any(node["id"].endswith("workspace.top-bottom") for node in frame["nodes"]))
            send(b"\x17s")
            current = choose("Explorer", "application.explorer")
            current = until(lambda frame: len(titles(frame)) == 3)
            print("PASS: the second pane splits into a T layout")
            assert find(current, f"workspace.title.{source}")["rect"] == left
            tap(current, f"workspace.title.{source}")
            send(b"\x17s")
            current = choose("Settings", "application.settings")
            current = until(lambda frame: len(titles(frame)) == 4)
            focused = next(node for node in titles(current) if node["selected"])
            focused_id = focused["id"].rsplit(".", 1)[1]
            assert len({node["id"] for node in titles(current)}) == 4
            tap(current, f"workspace.split.{focused_id}")
            current = until(lambda frame: any(node["id"].endswith("workspace.top-bottom") for node in frame["nodes"]))
            assert not find(current, "workspace.left-right")["enabled"]
            assert not find(current, "workspace.top-bottom")["enabled"]
            send(b"\x1b")
            until(lambda frame: not any(node["id"].endswith("workspace.top-bottom") for node in frame["nodes"]))
            resize(108, 20, 1)
            current = resize(217, 39, 4)
            assert any(node["id"].endswith(f"workspace.title.{focused_id}") and node["selected"] for node in titles(current))
            print("PASS: T layout, four independent panes, disabled actions, and resize restoration")

            tap(current, f"workspace.split.{focused_id}")
            current = until(lambda frame: any(node["id"].endswith(f"workspace.maximize.{focused_id}") for node in frame["nodes"]))
            tap(current, f"workspace.maximize.{focused_id}")
            current = until(lambda frame: len(titles(frame)) == 1)
            tap(current, f"workspace.taskbar.{source}")
            current = until(lambda frame: len(titles(frame)) == 1 and frame["page"] == "Launcher")
            send(b"\x17v")
            current = choose("Settings", f"window.{settings}")
            current = until(lambda frame: len(titles(frame)) == 2)
            assert any(node["id"].endswith(f"workspace.title.{settings}") for node in titles(current))
            print("PASS: full screen clears the layout; minimized-window selection restores the same instance")

            send(b"\x17s")
            choose("Command Line", "builtin.command-line")
            until(lambda frame: frame["page"] == "CommandLine")
            send(b"\x17vtundra_native_check")
            current = until(lambda frame: "vtundra_native_check" in "\n".join(frame["lines"]))
            assert not chooser(current)
            assert len(titles(current)) == 3
            send(b"\x03")
            print("PASS: the native terminal receives Ctrl+W without entering workspace split mode")
        finally:
            if child.poll() is None:
                os.kill(child.pid, signal.SIGTERM)
                deadline = time.monotonic() + 8
                while child.poll() is None and time.monotonic() < deadline:
                    smoke.read_available(master, output, 0.05)
                if child.poll() is None:
                    child.kill()
                child.wait(timeout=5)
            assert termios.tcgetattr(slave) == before, "terminal settings were not restored"
            os.close(master)
            os.close(slave)


if __name__ == "__main__":
    main()
