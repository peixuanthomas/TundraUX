#!/usr/bin/env python3
"""Verify Home ordering and launcher workflows in a real Linux PTY."""
import fcntl
import json
import os
from pathlib import Path
import pty
import re
import select
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time


def main():
    if sys.platform != "linux":
        raise SystemExit("This Launcher test requires Linux.")
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/tundra-shell").resolve()
    cli = binary.with_name("tundra-cli")
    with tempfile.TemporaryDirectory(prefix="tundra-ui-control-") as temporary:
        root = Path(temporary)
        env = os.environ.copy()
        env["TERM"] = "xterm-256color"
        for key in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR"):
            folder = root / key.lower()
            folder.mkdir(mode=0o700)
            env[key] = str(folder)
        configured = subprocess.run([str(cli), "config", "set", "automation.enabled", "true"],
            env=env, capture_output=True, text=True, timeout=15)
        assert configured.returncode == 0, configured.stderr
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))
        before = termios.tcgetattr(slave)
        output = bytearray()
        stopped = threading.Event()

        def drain():
            while not stopped.is_set():
                if select.select([master], [], [], 0.05)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError:
                        return
                    output.extend(data)
                    if len(output) > 2 * 1024 * 1024:
                        del output[:1024 * 1024]

        child = subprocess.Popen([str(binary)], env=env, stdin=slave, stdout=slave, stderr=slave,
            start_new_session=True)
        thread = threading.Thread(target=drain, daemon=True)
        thread.start()

        def ui(*args, okay=True, stdin=None):
            result = subprocess.run([str(cli), "ui", *map(str, args)], env=env,
                input=stdin, capture_output=True, text=True, timeout=10)
            response = json.loads(result.stdout)
            if okay:
                assert result.returncode == 0 and response["ok"], response
            return response

        def snapshot():
            return ui("snapshot")["snapshot"]

        def until(predicate, timeout=15):
            deadline = time.monotonic() + timeout
            last = None
            while time.monotonic() < deadline:
                response = ui("snapshot", okay=False)
                last = response.get("snapshot")
                if last and predicate(last):
                    return last
                assert child.poll() is None, bytes(output[-8000:]).decode(errors="replace")
                time.sleep(0.05)
            raise AssertionError(f"Shell condition timed out: {last and last['page']}")

        def key(name, *options):
            return ui("key", name, *options)["snapshot"]

        def click(identifier, double=False):
            ui("wait", "--id", identifier)
            return ui("click", "--id", identifier, *(["--double"] if double else []))

        def node_by(current, predicate):
            return next(node for node in current["nodes"] if predicate(node))

        batch_children = []
        try:
            if os.geteuid() == 0:
                deadline = time.monotonic() + 5
                while b"any other key cancels: " not in output and time.monotonic() < deadline:
                    time.sleep(0.02)
                assert b"any other key cancels: " in output
                os.write(master, b"y")
            current = until(lambda current: bool(current["nodes"]))
            if "--dump" in sys.argv:
                print(json.dumps(current, ensure_ascii=False, indent=2))
                return
            # Bare PTYs have no graphics responder, so dismiss that real warning first.
            for _ in range(8):
                current = snapshot()
                if current["page"] == "Home" and not any(node["id"].startswith("modal/") for node in current["nodes"]):
                    break
                if any(node["id"].startswith("modal/") and node["actionable"] for node in current["nodes"]):
                    key("Enter")
                elif current["page"] == "FirstRunSetup":
                    key("F9")
                else:
                    key("Enter")
                time.sleep(0.1)
            current = until(lambda current: current["page"] == "Home")
            print("PASS: external CLI completed first-use setup")

            def home_items():
                return [node for node in snapshot()["nodes"]
                        if node["role"] == "button" and "home.entry." in node["id"]]

            def home_edit():
                button = node_by(snapshot(), lambda node: node["role"] == "button"
                                 and node["id"].endswith("home.edit"))
                click(button["id"])

            original_home = [node["name"] for node in home_items()]
            home_edit()
            first, second = home_items()[:2]
            click(first["id"])
            assert snapshot()["page"] == "Home"
            target = second["rect"]
            ui("drag", "--from-id", first["id"], "--to-x", target["x"] + target["width"] - 1,
               "--to-y", target["y"] + 1)
            expected_home = [original_home[1], original_home[0], *original_home[2:]]
            assert [node["name"] for node in home_items()] == expected_home
            state_file = next(root.rglob("state.v1.json"))
            home_orders = [json.loads(value) for name, value in json.loads(state_file.read_text())["values"].items()
                           if name.startswith("home.order.")]
            assert len(home_orders) == 1 and len(home_orders[0]) == len(original_home)
            home_edit()
            key("F2")
            key("Left", "--modifiers", "shift")
            assert [node["name"] for node in home_items()] == original_home
            key("Escape")
            assert snapshot()["page"] == "Home"
            settings = node_by(snapshot(), lambda node: node["role"] == "button"
                               and "home.entry." in node["id"] and "Settings" in node["name"])
            click(settings["id"])
            until(lambda current: current["page"] == "Settings")
            key("Escape")
            assert snapshot()["page"] == "Settings"
            close = node_by(snapshot(), lambda node: "workspace.close." in node["id"] and node["actionable"])
            click(close["id"])
            until(lambda current: current["page"] == "Home")
            assert [node["name"] for node in home_items()] == original_home
            print("PASS: Home edit button, touch drag, saved order, keyboard reorder, and normal activation")

            def window_ids(current):
                return {node["id"] for node in current["nodes"] if node["role"] == "button"
                        and "workspace.taskbar." in node["id"] and not node["id"].endswith(".close")}

            before_command = window_ids(snapshot())

            def open_launcher():
                current = until(lambda current: current["page"] == "Home")
                launcher = node_by(current, lambda node: "home.entry." in node["id"] and node["name"] == "Launcher")
                click(launcher["id"])
                current = until(lambda current: current["page"] == "Launcher"
                    and any("launcher.item." in node["id"] and node["actionable"] for node in current["nodes"]))
                assert not window_ids(current)
                return current

            current = open_launcher()
            ui("text", "Settings")
            until(lambda frame: any(node["id"].endswith("launcher.item.builtin.settings")
                  and node["selected"] for node in frame["nodes"]))
            key("Enter")
            current = until(lambda frame: frame["page"] == "Settings")
            click(node_by(current, lambda node: "workspace.close." in node["id"] and node["actionable"])["id"])
            open_launcher()
            key("Down")

            def toolbar(label):
                current = snapshot()
                button = node_by(current, lambda node: node["role"] == "button" and label in node["name"])
                click(button["id"])

            def items():
                return [node for node in snapshot()["nodes"] if node["role"] == "button" and "launcher.item." in node["id"]]

            first_page = [node["name"] for node in items()]
            scrollable = any(node["role"] == "scrollbar" for node in snapshot()["nodes"])
            point = items()[0]["rect"]
            ui("scroll", "--x", point["x"], "--y", point["y"], "--direction", "down")
            assert ([node["name"] for node in items()] != first_page) == scrollable
            ui("scroll", "--x", point["x"], "--y", point["y"], "--direction", "up", "--count", 10)
            assert [node["name"] for node in items()] == first_page
            print("PASS: mouse wheel respects launcher scrolling bounds")
            toolbar("Organize")
            cards = items()
            first, second = cards[:2]
            target = second["rect"]
            ui("drag", "--from-id", first["id"], "--to-x", target["x"] + target["width"] - 1,
               "--to-y", target["y"] + 1)
            assert items()[0]["name"] == second["name"]
            print("PASS: organize mode drags built-in icons without launching")
            toolbar("Add launcher")
            ui("text", "discarded launcher")
            key("Escape")
            toolbar("Add launcher")
            switch = node_by(snapshot(), lambda node: node["role"] == "checkbox"
                and node["name"] == "Keep screen after exit")
            assert not switch["selected"]
            ui("text", "Custom PTY tool")
            key("Enter")
            ui("text", "printf 'CUSTOM_LAUNCHER_PTY_OK\\n'; exit 75")
            key("Enter")
            key("Space")
            switch = node_by(snapshot(), lambda node: node["role"] == "checkbox"
                and node["name"] == "Keep screen after exit")
            assert switch["selected"]
            key("Enter")
            toolbar("Done organizing")
            key("End")
            current = snapshot()
            assert "Custom PTY tool" in "\n".join(current["lines"])
            import tomllib
            config_file = next(Path(env["XDG_CONFIG_HOME"]).rglob("config.toml"))
            config = tomllib.loads(config_file.read_text())
            custom = next(entry for entry in config["launcher"]["entries"] if entry.get("custom", {}).get("name") == "Custom PTY tool")
            assert custom["custom"]["icon"] == ""
            assert custom["custom"]["wait_for_enter"]
            assert config["launcher"]["order"]
            custom_id = custom["id"]

            def saved_custom():
                config = tomllib.loads(config_file.read_text())
                return next(entry["custom"] for entry in config["launcher"]["entries"] if entry["id"] == custom_id)

            def open_context():
                card = next(node for node in items() if f"launcher.item.{custom_id}" in node["id"])
                ui("wait", "--id", card["id"])
                ui("click", "--id", card["id"], "--button", "right")
                return until(lambda current: not current["animation_running"]
                    and any("launcher.menu.0" in node["id"] for node in current["nodes"]))

            def context_action(label):
                current = open_context()
                button = node_by(current, lambda node: node["role"] == "button" and node["name"] == label)
                click(button["id"])

            current = open_context()
            switch = node_by(current, lambda node: node["role"] == "checkbox"
                and node["name"] == "Keep screen after exit")
            assert switch["selected"]
            for checked in [False, True]:
                click(switch["id"])
                current = until(lambda current: any(
                    node["id"] == switch["id"] and node["selected"] == checked for node in current["nodes"]))
                switch = node_by(current, lambda node: node["id"] == switch["id"])
                assert switch["selected"] == checked
                saved = next(entry for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"]
                    if entry["id"] == custom_id)
                assert saved["custom"]["wait_for_enter"] == checked
            print("PASS: add form and context menu expose a saved on/off switch")
            ui("click", "--x", 1, "--y", 4)
            current = until(lambda current: not current["animation_running"]
                and not any("launcher.menu." in node["id"] for node in current["nodes"]))
            assert current["page"] == "Launcher"
            assert not any("launcher.menu." in node["id"] for node in current["nodes"])
            context_action("Change name")
            key("a", "--modifiers", "ctrl")
            ui("text", "Renamed PTY tool")
            key("F10")
            until(lambda current: saved_custom()["name"] == "Renamed PTY tool"
                  and any(node["name"] == "Renamed PTY tool" for node in current["nodes"]))
            context_action("Change launch command")
            key("a", "--modifiers", "ctrl")
            ui("text", "printf 'EDITED_LAUNCHER_PTY_OK\\n'; exit 75")
            key("F10")
            until(lambda current: saved_custom()["command"] == "printf 'EDITED_LAUNCHER_PTY_OK\\n'; exit 75")
            context_action("Change icon")
            ui("text", "中文x\n★")
            ui("text", "!")
            assert "Icons allow two rows" in "\n".join(snapshot()["lines"])
            key("F10")
            current = until(lambda current: saved_custom()["icon"] == "中文x\n★"
                            and "中文x" in "\n".join(current["lines"]))
            config = tomllib.loads(config_file.read_text())
            custom = next(entry for entry in config["launcher"]["entries"] if entry["id"] == custom_id)
            assert custom["custom"]["name"] == "Renamed PTY tool"
            assert custom["custom"]["icon"] == "中文x\n★"
            assert custom["custom"]["wait_for_enter"]
            print("PASS: context menu closes outside; name, command and bounded Unicode icon persist")
            key("Enter")
            current = until(lambda current: current["page"] == "CommandLine"
                and "EDITED_LAUNCHER_PTY_OK" in "\n".join(current["lines"])
                and "Press Enter to return." in "\n".join(current["lines"]))
            assert any(node["role"] == "terminal" for node in current["nodes"])
            command_windows = window_ids(current) - before_command
            assert len(command_windows) == 1, "Custom commands require an independent AppHost"
            key("Enter")
            until(lambda current: current["page"] == "Home" and window_ids(current) == before_command)
            current = open_launcher()
            print("PASS: custom settings persist; Enter closes retained PTY output and returns to the workspace")
            for code in [0, 9]:
                toolbar("Organize")
                toolbar("Add launcher")
                ui("text", f"Immediate exit {code}")
                key("Tab")
                marker = root / f"immediate-{code}.txt"
                ui("text", f"printf done > {shlex.quote(str(marker))}; exit {code}")
                key("F10")
                toolbar("Done organizing")
                key("End")
                key("Enter")
                until(lambda current: marker.exists() and current["page"] == "Home" and window_ids(current) == before_command)
                assert marker.read_text() == "done"
                current = open_launcher()
            print("PASS: default custom commands exit without Enter after success and failure")
            key("Down")
            key("End")
            context_action("Add to Home")
            key("Escape")
            until(lambda current: current["page"] == "Home")
            key("End")
            shortcut = node_by(snapshot(), lambda node: "home.entry." in node["id"] and node["name"] == "Renamed PTY tool")
            click(shortcut["id"])
            until(lambda current: current["page"] == "CommandLine" and "EDITED_LAUNCHER_PTY_OK" in "\n".join(current["lines"]))
            key("Enter")
            until(lambda current: current["page"] == "Home" and window_ids(current) == before_command)
            ui("click", "--id", shortcut["id"], "--button", "right")
            current = until(lambda current: any(node["name"] == "Remove from Home" and node["actionable"] for node in current["nodes"]))
            click(node_by(current, lambda node: node["name"] == "Remove from Home")["id"])
            assert any(entry["id"] == custom_id for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"])
            open_launcher()
            key("Down")
            key("End")
            print("PASS: a custom app launches from Home and remains installed after removing its shortcut")
            context_action("Delete")
            key("Escape")
            assert any(entry["id"] == custom_id for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"])
            context_action("Delete")
            key("Enter")
            until(lambda current: not any(entry["id"] == custom_id
                for entry in tomllib.loads(config_file.read_text())["launcher"]["entries"]))
            key("Home")
            ui("click", "--id", items()[0]["id"], "--button", "right")
            current = until(lambda current: not current["animation_running"]
                            and any("launcher.menu." in node["id"] for node in current["nodes"]))
            menu_labels = [node["name"] for node in current["nodes"] if "launcher.menu." in node["id"]]
            assert menu_labels and not any(label in menu_labels for label in
                ("Change name", "Change launch command", "Change icon", "Delete"))
            key("Escape")
            print("PASS: deletion confirms or cancels; built-in entries retain their restrictions")
        except Exception:
            print("Last snapshot:", json.dumps(locals().get("current"), ensure_ascii=False), file=sys.stderr)
            visible = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", bytes(output[-16000:]))
            print("Shell status:", child.poll(), "\n", visible.decode(errors="replace"), file=sys.stderr)
            raise
        finally:
            for process in batch_children:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait(timeout=5)
            stopped.set()
            thread.join(timeout=1)
            after = termios.tcgetattr(slave)
            os.close(master)
            os.close(slave)
            assert after == before, "Shell did not restore terminal modes"


if __name__ == "__main__":
    main()
