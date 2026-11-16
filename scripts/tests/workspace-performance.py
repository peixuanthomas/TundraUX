#!/usr/bin/env python3
"""Measure real PTY app output and visible input feedback in isolated Linux sessions.

Snapshots locate controls and verify results after an action. Timing endpoints are
PTY frame tails, never CLI responses. Raw samples and PTY streams are preserved.
"""
from __future__ import annotations

import argparse
import codecs
from collections import defaultdict
import fcntl
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import pty
import re
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unicodedata

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "shell_smoke", Path(__file__).resolve().parents[1] / "linux-shell-smoke.py"
)
smoke = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(smoke)

APPLICATIONS = ("Launcher", "Settings", "Explorer", "Clock", "Editor", "SystemStatus")
LABELS = {"SystemStatus": "System Status", "CommandLine": "Command Line"}
CSI = re.compile(r"\x1b\[([0-?]*)([ -/]*)([@-~])")
CAPTURE_TIMEOUT_SECONDS = 20.0
NATIVE_VIM_ENDPOINT = "changed-empty-vt-eob-scratch-v1"
FOCUS_ENDPOINT = "launcher-search-field-focus-style-v1"


class Screen:
    """Decode the cursor, color and cell operations emitted by Ratatui/Crossterm."""

    def __init__(self, width, height):
        self.width, self.height = width, height
        self.cells = [[(" ", ())] * width for _ in range(height)]
        self.x = self.y = 0
        self.style = ()
        self.foreground = self.background = None
        self.attributes = set()
        self.saw_rgb = False
        self.await_cursor_position = False
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.pending = ""

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        index = 0
        tails = []
        while index < len(self.pending):
            char = self.pending[index]
            if char == "\x1b":
                if index + 1 == len(self.pending):
                    break
                if self.pending[index + 1] == "[":
                    match = CSI.match(self.pending, index)
                    if match is None:
                        break
                    raw, _, command = match.groups()
                    if raw == "?25" and command == "l":
                        self.await_cursor_position = False
                        tails.append(tuple(tuple(row) for row in self.cells))
                    elif raw == "?25" and command == "h":
                        self.await_cursor_position = True
                    if re.fullmatch(r"[0-9;]*", raw):
                        values = [int(value or "0") for value in raw.split(";")]
                        amount = values[0] or 1
                        if command in "Hf":
                            self.y = amount - 1
                            self.x = ((values[1] if len(values) > 1 else 1) or 1) - 1
                            if self.await_cursor_position:
                                tails.append(tuple(tuple(row) for row in self.cells))
                                self.await_cursor_position = False
                        elif command == "G":
                            self.x = amount - 1
                        elif command == "d":
                            self.y = amount - 1
                        elif command in "ABCD":
                            self.x += amount * ((command == "C") - (command == "D"))
                            self.y += amount * ((command == "B") - (command == "A"))
                        elif command == "J" and values[0] in (2, 3):
                            self.cells = [[(" ", ())] * self.width for _ in range(self.height)]
                        elif command == "K" and 0 <= self.y < self.height:
                            start, end = ((0, self.width) if values[0] == 2 else
                                          ((0, self.x + 1) if values[0] == 1 else (self.x, self.width)))
                            for column in range(max(0, start), min(self.width, end)):
                                self.cells[self.y][column] = (" ", self.style)
                        elif command == "m":
                            self.set_style(values)
                    index = match.end()
                    continue
                if self.pending[index + 1] in "]P_^":
                    end = re.search(r"\x07|\x1b\\", self.pending[index + 2:])
                    if end is None:
                        break
                    index += 2 + end.end()
                    continue
                index += 2
                continue
            if char == "\r":
                self.x = 0
            elif char == "\n":
                self.y += 1
            elif char == "\b":
                self.x = max(0, self.x - 1)
            elif char.isprintable() and not unicodedata.combining(char):
                width = 2 if unicodedata.east_asian_width(char) in "WF" else 1
                if 0 <= self.y < self.height and 0 <= self.x < self.width:
                    self.cells[self.y][self.x] = (char, self.style)
                    if width == 2 and self.x + 1 < self.width:
                        self.cells[self.y][self.x + 1] = (" ", self.style)
                self.x += width
            index += 1
        self.pending = self.pending[index:]
        return tails

    def set_style(self, values):
        index = 0
        while index < len(values):
            value = values[index]
            if value == 0:
                self.foreground = self.background = None
                self.attributes.clear()
            elif value in (38, 48):
                length = 2 if values[index + 1] == 5 else 4
                self.saw_rgb |= length == 4
                color = tuple(values[index + 1:index + 1 + length])
                if value == 38:
                    self.foreground = color
                else:
                    self.background = color
                index += length
            elif 30 <= value <= 37 or 90 <= value <= 97:
                self.foreground = value
            elif 40 <= value <= 47 or 100 <= value <= 107:
                self.background = value
            elif value == 39:
                self.foreground = None
            elif value == 49:
                self.background = None
            elif value in (22, 23, 24, 25, 27, 28, 29):
                for attribute in {22: (1, 2), 23: (3,), 24: (4,), 25: (5, 6),
                                  27: (7,), 28: (8,), 29: (9,)}[value]:
                    self.attributes.discard(attribute)
            else:
                self.attributes.add(value)
            index += 1
        self.style = (self.foreground, self.background, tuple(sorted(self.attributes)))


def region(cells, rect, styled=False):
    rows = cells[rect["y"]:rect["y"] + rect["height"]]
    clipped = tuple(tuple(row[rect["x"]:rect["x"] + rect["width"]]) for row in rows)
    return clipped if styled else tuple("".join(cell[0] for cell in row) for row in clipped)


def node(frame, suffix):
    return next(item for item in frame["nodes"] if item["id"].endswith(suffix))


def titles(frame):
    return [item for item in frame["nodes"] if "workspace.title." in item["id"]]


def focus_state(frame):
    return [(item["id"], item.get("focused", False), item.get("selected", False))
            for item in frame["nodes"] if item["id"].startswith("page/")
            and (item.get("focused") or item.get("selected"))]


def percentile(values, fraction):
    return round(sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)], 3)


def statistics(values):
    return {"count": len(values), "p50_ms": percentile(values, .50),
            "p95_ms": percentile(values, .95), "p99_ms": percentile(values, .99),
            "max_ms": round(max(values), 3)}


def business_rects(frame):
    rects = [dict(item["rect"], height=1) if "system-status.widget." in item["id"] else item["rect"]
             for item in frame["nodes"]
             if item["id"].startswith("page/") and item["actionable"]
             and item["role"] in ("button", "textbox", "tab", "checkbox")
             and item["text"].strip()]
    if not rects:
        rects = [item["rect"] for item in frame["nodes"]
                 if item["id"].startswith("page/") and item["actionable"]
                 and item["role"] == "terminal" and item["text"].strip()]
    assert rects, "A business frame requires nonempty interactive application controls"
    return rects


def signature(cells, rects, styled=False):
    return tuple(region(cells, rect, styled) for rect in rects)


def snapshot_cells(frame):
    screen = Screen(frame["width"], frame["height"])
    for row, line in enumerate(frame["lines"]):
        screen.feed(f"\x1b[{row + 1};1H".encode() + line.encode())
    return tuple(tuple(row) for row in screen.cells)


def frame_cue(cells, rects, styled=False):
    expected = signature(cells, rects, styled)
    def matches(candidate):
        return signature(candidate["cells"], rects, styled) == expected
    matches.description = {"rects": rects, "styled": styled,
                           "sha256": hashlib.sha256(json.dumps(expected, ensure_ascii=False).encode()).hexdigest()}
    return matches


def search_state_observed(session, frame, focused):
    field = node(frame, "page/launcher.search")
    if not field["actionable"] or field["focused"] != focused or not session.frames:
        return False
    if field["text"].rsplit(":", 1)[-1].strip() not in ("", "_"):
        return False
    return region(session.frames[-1]["cells"], field["rect"]) == region(snapshot_cells(frame), field["rect"])


def prepare_mouse_search(session):
    current = session.until(lambda frame: any(item["id"] == "page/launcher.search" and item["actionable"]
                                             for item in frame["nodes"]))
    session.tap(current, "page/launcher.search")
    session.until(lambda frame: node(frame, "page/launcher.search")["focused"])
    session.send(b"\x01\x7f")
    session.until(lambda frame: search_state_observed(session, frame, True))
    # Empty keyboard input can change only the padding foreground. Observe a
    # complete unfocused frame, then a real mouse-focused frame before caching.
    session.send(b"\t")
    current = session.until(lambda frame: search_state_observed(session, frame, False))
    session.tap(current, "page/launcher.search")
    return session.until(lambda frame: search_state_observed(session, frame, True))


def vim_terminal_rect(page_rect):
    return dict(page_rect, y=page_rect["y"] + 1, height=page_rect["height"] - 1)


def vim_ready_rects(rect):
    # The empty buffer has one editable line, EOB rows, a Scratch status
    # line and a command line. Intro text may occupy the middle columns.
    return ([dict(rect, height=1)]
            + [dict(rect, y=rect["y"] + row, width=1, height=1) for row in range(1, rect["height"] - 2)]
            + [dict(rect, y=rect["y"] + rect["height"] - 2, height=1),
               dict(rect, y=rect["y"] + rect["height"] - 1, height=1)])


def vim_buffer_ready(cells, rect, before=None):
    rows = region(cells, rect)
    if len(rows) < 5 or rows == before or rows[0].strip() or rows[-1].strip():
        return False
    return (all(row.startswith("~") for row in rows[1:-2])
            and "[Scratch]" in rows[-2] and "0,0-1" in rows[-2]
            and re.search(r"\bAll\b", rows[-2]) is not None)


class Session:
    def __init__(self, binary, output, label, width=120, height=36, motion="full", detailed_trace=False):
        self.binary, self.cli = binary, binary.with_name("tundra-cli")
        self.output, self.label = output, label
        self.capture_timeout = CAPTURE_TIMEOUT_SECONDS
        self.capture_active = False
        self.rpc_count = self.rpc_response_bytes = 0
        self.capture_audit = []
        self.business_templates = {}
        self.new_process_pids = {}
        self.temporary = tempfile.TemporaryDirectory(prefix="tundra-perf-")
        root = Path(self.temporary.name)
        self.env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", LC_ALL="C.UTF-8",
                        TUNDRA_VIM_CLEAN="1", NO_COLOR="")
        vim = binary.parent / "vim" / "vim"
        if vim.is_file():
            self.env["TUNDRA_VIM"] = str(vim)
        self.env["HOME"] = str(root / "home")
        Path(self.env["HOME"]).mkdir()
        for name in ("CONFIG", "DATA", "CACHE", "STATE", "RUNTIME"):
            folder = root / name.lower()
            folder.mkdir(mode=0o700)
            self.env[f"XDG_{name}_HOME" if name != "RUNTIME" else "XDG_RUNTIME_DIR"] = str(folder)
        self.trace_path = output / f"{label}.trace.jsonl"
        self.env.pop("TUNDRA_PERF_TRACE", None)
        if detailed_trace:
            self.env["TUNDRA_PERF_TRACE"] = str(self.trace_path)
        for field, value in (("automation.enabled", "true"), ("language", "en-US"),
                             ("motion", motion), ("icon-mode", "ascii")):
            result = subprocess.run([str(self.cli), "config", "set", field, value],
                                    env=self.env, capture_output=True, timeout=15)
            assert result.returncode == 0, result.stderr
        self.master, self.slave = pty.openpty()
        self.before = termios.tcgetattr(self.slave)
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        os.set_blocking(self.master, False)
        self.screen = Screen(width, height)
        self.raw = (output / f"{label}.pty").open("wb")
        self.frame_index = (output / f"{label}.frames.jsonl").open("w")
        self.frame_number = 0
        self.frames = []
        self.bytes = 0
        self.pty_eof = False
        self.info = None
        self.child = subprocess.Popen([str(binary)], env=self.env, cwd=root / "home",
                                      stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                      start_new_session=True)
        try:
            if os.geteuid() == 0:
                deadline = time.monotonic() + 5
                seen = b""
                while b"any other key cancels: " not in seen and time.monotonic() < deadline:
                    seen += self.drain(.05)
                assert b"any other key cancels: " in seen
                self.send(b"y")
            deadline = time.monotonic() + 30
            while self.info is None and time.monotonic() < deadline:
                files = list(Path(self.env["XDG_CACHE_HOME"]).rglob("shell-control/*.json"))
                if files:
                    self.info = json.loads(files[0].read_text())
                else:
                    self.drain(.02)
            assert self.info is not None, "Shell control registration timed out"
            for _ in range(20):
                current = self.snapshot()
                if current["page"] == "Home" and not self.has_modal(current):
                    break
                self.send(b"\x1b[21~" if current["page"] == "FirstRunSetup" else b"\r")
                self.drain(.08)
            self.until(lambda frame: frame["page"] == "Home" and not self.has_modal(frame))
            assert self.screen.saw_rgb, "Truecolor output is required for comparable focus measurements"
        except BaseException:
            self.close()
            raise

    def drain(self, timeout=0):
        if not select.select([self.master], [], [], timeout)[0]:
            return b""
        try:
            data = os.read(self.master, 256 * 1024)
        except BlockingIOError:
            return b""
        except OSError:
            self.pty_eof = True
            return b""
        if not data:
            self.pty_eof = True
            return b""
        observed = time.monotonic_ns()
        self.raw.write(data)
        self.bytes += len(data)
        for index, cells in enumerate(self.screen.feed(data)):
            self.frame_number += 1
            boundary = {"observed_ns": observed, "byte_offset": self.bytes,
                        "frame_in_read": index, "frame_number": self.frame_number}
            self.frame_index.write(json.dumps(boundary) + "\n")
            self.frames.append(dict(boundary, cells=cells))
        return data

    def send(self, value):
        self.drain()
        started = time.monotonic_ns()
        os.write(self.master, value)
        return started

    def control(self, action):
        assert not self.capture_active, "Automation RPC is forbidden during PTY capture"
        assert self.info
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(self.info["endpoint"])
            connection.sendall(json.dumps(dict(protocol=1, **action)).encode() + b"\n")
            self.rpc_count += 1
            connection.setblocking(False)
            reply = b""
            deadline = time.monotonic() + 20
            while b"\n" not in reply and time.monotonic() < deadline:
                ready, _, _ = select.select([self.master, connection], [], [], .1)
                if self.master in ready:
                    self.drain()
                if connection in ready:
                    chunk = connection.recv(256 * 1024)
                    if not chunk:
                        raise ConnectionResetError("Shell closed control connection")
                    reply += chunk
                assert self.child.poll() is None, f"Shell exited: {self.child.returncode}"
            assert b"\n" in reply, "Shell control timed out"
            self.rpc_response_bytes += len(reply)
            response = json.loads(reply)
            if response.get("error", {}).get("code") == "not_ready":
                return None
            assert response["ok"], response
            self.drain()
            return response["snapshot"]

    def snapshot(self):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            try:
                snapshot = self.control({"command": "snapshot"})
            except (ConnectionResetError, BrokenPipeError, FileNotFoundError, ConnectionRefusedError):
                files = list(Path(self.env["XDG_CACHE_HOME"]).rglob("shell-control/*.json"))
                if files:
                    self.info = json.loads(files[-1].read_text())
                snapshot = None
            if snapshot is not None:
                return snapshot
            self.drain(.005)
        raise AssertionError("Shell did not publish its first snapshot")

    def until(self, predicate, timeout=20):
        deadline = time.monotonic() + timeout
        current = None
        while time.monotonic() < deadline:
            current = self.snapshot()
            if predicate(current):
                return current
        path = self.output / f"{self.label}.last-snapshot.json"
        path.write_text(json.dumps(current, ensure_ascii=False, indent=2) + "\n")
        raise AssertionError(f"Condition timed out; see {path}")

    @staticmethod
    def has_modal(frame):
        return any(item["id"].startswith("modal/") for item in frame["nodes"])

    def tap(self, frame, suffix, button=0):
        target = node(frame, suffix)
        assert target["actionable"], target
        x, y = smoke.control_position(frame, target["id"])
        return self.send(f"\x1b[<{button};{x};{y}M\x1b[<{button};{x};{y}m".encode())

    def failed_attempt(self, capture, reason):
        record = {key: value for key, value in capture.items() if key != "frames"}
        record.setdefault("sealed_ns", time.monotonic_ns())
        record.setdefault("sealed_frame_number", self.frame_number)
        record["raw_byte_offset_at_failure"] = getattr(self, "bytes", 0)
        if record.get("started_ns") is not None:
            record["capture_elapsed_ms"] = (record["sealed_ns"] - record["started_ns"]) / 1e6
            record["deadline_ns"] = record["started_ns"] + int(record["timeout_seconds"] * 1e9)
        record.update(session_label=self.label, reason=reason, completed=False,
                      raw_pty=f"{self.label}.pty", frame_index=f"{self.label}.frames.jsonl")
        with (self.output / "failed-attempts.jsonl").open("a") as stream:
            stream.write(json.dumps(record) + "\n")

    def capture_output(self, action, cue=None, operation="feedback"):
        assert not self.capture_active
        self.frames.clear()
        before_rpc = self.rpc_count
        capture = {"operation": operation, "started_ns": None, "frames": (),
                   "timeout_seconds": self.capture_timeout, "discovery": cue is None,
                   "cue_signature": getattr(cue, "description", None)}
        self.capture_active = True
        try:
            capture["started_ns"] = action()
            deadline_ns = capture["started_ns"] + int(self.capture_timeout * 1e9)
            checked = 0
            matched = None
            while time.monotonic_ns() < deadline_ns:
                self.drain(min(.02, max(0, (deadline_ns - time.monotonic_ns()) / 1e9)))
                if self.pty_eof:
                    raise AssertionError("PTY closed during output capture")
                if cue is not None:
                    for candidate in self.frames[checked:]:
                        if capture["started_ns"] <= candidate["observed_ns"] <= deadline_ns and cue(candidate):
                            matched = candidate
                            break
                    checked = len(self.frames)
                    if matched is not None:
                        break
            capture["sealed_ns"] = time.monotonic_ns()
            capture["rpc_count"] = self.rpc_count - before_rpc
            assert capture["rpc_count"] == 0, "Automation ran during physical output capture"
            cutoff = matched["frame_number"] if matched is not None else self.frame_number
            capture["sealed_frame_number"] = cutoff
            capture["frames"] = tuple(item for item in self.frames
                                      if capture["started_ns"] <= item["observed_ns"] <= deadline_ns
                                      and item["frame_number"] <= cutoff)
            if cue is not None and matched is None:
                raise AssertionError("PTY target did not arrive before the capture timeout")
            assert capture["frames"], "No completed PTY frame in capture window"
        except BaseException as error:
            capture.setdefault("sealed_ns", time.monotonic_ns())
            capture["rpc_count"] = self.rpc_count - before_rpc
            self.capture_active = False
            self.failed_attempt(capture, str(error))
            raise
        finally:
            self.capture_active = False
        return capture

    def verify_capture(self, capture, predicate):
        before_rpc = self.rpc_count
        try:
            frame = self.control({"command": "snapshot"})
            assert frame is not None and predicate(frame), "Post-capture state verification failed"
            assert self.rpc_count - before_rpc == 1, "Verification must use one snapshot request"
            return frame
        except BaseException as error:
            self.failed_attempt(capture, str(error))
            raise

    def captured_timing(self, capture, frame, rect=None, styled=False, changed_from=None):
        try:
            rects = business_rects(frame) if rect is None else ([rect] if isinstance(rect, dict) else rect)
            # A later verification reply cannot lend its new business content to
            # an older/blank frame in the sealed interval.
            expected_text = signature(snapshot_cells(frame), rects)
            assert any(line.strip() for area in expected_text for line in area), "Blank verified business content"
            assert signature(capture["frames"][-1]["cells"], rects) == expected_text, \
                "Verified business content was not present before the capture was sealed"
            timing = self.match_output(capture["started_ns"], frame, rect, styled, changed_from,
                                       captured_frames=capture["frames"])
            assert timing["output_frame_number"] <= capture["sealed_frame_number"]
            timing["observation"] = {key: capture[key] for key in
                                     ("sealed_ns", "sealed_frame_number", "rpc_count", "discovery")}
            self.capture_audit.append(dict(operation=capture["operation"], **timing["observation"]))
            return timing
        except BaseException as error:
            self.failed_attempt(capture, str(error))
            raise

    def match_output(self, started, frame, rect=None, styled=False, changed_from=None, captured_frames=None):
        frames = self.frames if captured_frames is None else captured_frames
        assert frames, "No completed PTY frame"
        if rect is None:
            rect = business_rects(frame)
        rects = [rect] if isinstance(rect, dict) else rect

        def clipped(cells):
            return tuple(region(cells, area, styled) for area in rects)

        expected = clipped(frames[-1]["cells"])
        if not styled:
            assert any(line.strip() for area in expected for line in area), "Blank region cannot prove a business frame"
        if changed_from is not None:
            assert expected != (changed_from,), "Action produced no visible feedback"
        matches = [item for item in frames if item["observed_ns"] >= started
                   and clipped(item["cells"]) == expected]
        assert matches, "No complete PTY frame matches the verified business content"
        matched = matches[0]
        return {"latency_ms": (matched["observed_ns"] - started) / 1_000_000,
                "action_ns": started, "output_ns": matched["observed_ns"],
                "output_byte_offset": matched["byte_offset"],
                "output_frame_number": matched.get("frame_number"),
                "verification": {"snapshot": frame["number"], "page": frame["page"],
                                 "rect": rect, "styled": styled,
                                 "content_sha256": hashlib.sha256(json.dumps(expected, ensure_ascii=False).encode()).hexdigest()}}

    def open_app(self, app, timed=True, terminal_marker=None):
        current = self.snapshot()
        label = LABELS.get(app, app)
        home = next((item for item in current["nodes"] if "home.entry." in item["id"]
                     and item["name"] == label), None)
        if app == "Clock":
            target = node(current, "chrome/shell.status.time")
        elif home is None:
            if current["page"] == "Home":
                launcher = next(item for item in current["nodes"] if "home.entry." in item["id"]
                                and item["name"] == "Launcher")
                self.tap(current, launcher["id"])
                current = self.until(lambda frame: frame["page"] == "Launcher")
            assert current["page"] == "Launcher", current["page"]
            target = next(item for item in current["nodes"] if item["role"] == "button"
                          and item["name"] == label)
        else:
            target = home
        target_id = target["id"]
        current = self.until(lambda frame: any(item["id"] == target_id and item["actionable"]
                                             for item in frame["nodes"]))
        def ready(frame):
            return (frame["page"] == app and bool(titles(frame))
                    and any(item["id"].startswith("page/") and item["actionable"] and item["text"].strip()
                            for item in frame["nodes"])
                    and (terminal_marker is None or any(item["role"] == "terminal" and terminal_marker in item["text"]
                                                         for item in frame["nodes"])))

        if not timed:
            self.tap(current, target_id)
            return self.until(ready), None
        key = (app, terminal_marker)
        template = self.business_templates.get(key)
        cue = frame_cue(*template) if template else None
        previous_ids = set(self.new_process_pids)

        def source_ready(frame):
            return (frame["page"] != app
                    and any(item["id"] == target_id and item["actionable"] for item in frame["nodes"])
                    and not any(item["id"].endswith(f"workspace.taskbar.{identifier}")
                                for identifier in previous_ids for item in frame["nodes"])
                    and bool(self.frames) and (cue is None or not cue(self.frames[-1])))

        current = self.until(source_ready)
        activation_context = {"page_before_action": current["page"], "target_id": target_id,
                              "previous_measured_window_ids_absent": sorted(previous_ids),
                              "known_business_signature_visible_before_action": False if cue else None}
        before_pids = {item["pid"] for item in self.resources()["processes"]}
        capture = self.capture_output(lambda: self.tap(current, target_id), cue, f"new/{app}")
        result = self.verify_capture(capture, ready)
        rects = ([next(item["rect"] for item in result["nodes"] if item["role"] == "terminal")]
                 if terminal_marker is not None else business_rects(result))
        timing = self.captured_timing(capture, result, rects)
        self.business_templates[key] = (capture["frames"][-1]["cells"], rects)
        after_pids = {item["pid"] for item in self.resources()["processes"]}
        new_pids = sorted(after_pids - before_pids)
        if not new_pids:
            self.failed_attempt(capture, "New window has no newly observed process")
            raise AssertionError("New window has no newly observed process")
        timing["process_identity"] = {"new_process_pids": new_pids}
        timing["activation_context"] = activation_context
        identifier = next(item["id"].rsplit(".", 1)[1] for item in titles(result) if item["selected"])
        self.new_process_pids[identifier] = new_pids
        return result, timing

    def resources(self):
        assert not self.capture_active, "Process/resource probes are forbidden during PTY capture"
        records = []
        ancestors = {self.child.pid}
        remaining = {}
        for entry in Path("/proc").iterdir():
            if not entry.name.isdecimal():
                continue
            try:
                fields = (entry / "stat").read_text().rsplit(") ", 1)[1].split()
                remaining[int(entry.name)] = fields
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                pass
        for _ in range(10):
            found = {pid for pid, fields in remaining.items() if int(fields[1]) in ancestors}
            if found <= ancestors:
                break
            ancestors.update(found)
        ticks = os.sysconf("SC_CLK_TCK")
        page_size = os.sysconf("SC_PAGE_SIZE")
        for pid in ancestors:
            fields = remaining.get(pid)
            if fields:
                records.append({"pid": pid, "cpu_seconds": (int(fields[11]) + int(fields[12])) / ticks,
                                "rss_bytes": int(fields[21]) * page_size})
        return {"processes": records, "rss_bytes": sum(item["rss_bytes"] for item in records),
                "cpu_seconds": sum(item["cpu_seconds"] for item in records)}

    def close(self):
        try:
            smoke.signal_process_group(self.child, signal.SIGTERM)
            deadline = time.monotonic() + 10
            while self.child.poll() is None and time.monotonic() < deadline:
                self.drain(.05)
            smoke.signal_process_group(self.child, signal.SIGKILL)
            self.child.wait(timeout=5)
            self.drain()
            result = subprocess.run([str(self.cli), "logs", "query", "--module", "ux.workspace",
                                     "--limit", "10000", "--format", "jsonl"],
                                    env=self.env, capture_output=True, timeout=15)
            assert result.returncode == 0, result.stderr
            (self.output / f"{self.label}.startup.jsonl").write_bytes(result.stdout)
        finally:
            (self.output / f"{self.label}.observation.json").write_text(json.dumps({
                "condition": "zero automation RPC during sealed physical-output capture",
                "total_control_requests": self.rpc_count,
                "total_control_response_bytes": self.rpc_response_bytes,
                "captures": self.capture_audit,
            }, indent=2) + "\n")
            self.raw.close()
            self.frame_index.close()
            after = termios.tcgetattr(self.slave)
            os.close(self.master)
            os.close(self.slave)
            self.temporary.cleanup()
            assert after == self.before, "Shell failed to restore terminal attributes"


def launches(binary, output, samples, motion, write, applications=APPLICATIONS, detailed_trace=False):
    for app in applications:
        session = Session(binary, output, f"launch-{app}", motion=motion, detailed_trace=detailed_trace)
        seen = set()
        try:
            for index in range(samples):
                current, timing = session.open_app(app)
                title = next(item for item in titles(current) if item["selected"])
                identifier = title["id"].rsplit(".", 1)[1]
                assert identifier not in seen, "Reused window cannot count as a new AppHost"
                seen.add(identifier)
                write(dict(kind="launch", application=app, index=index, session=identifier,
                           mode="first_open_in_shell" if index == 0 else "repeat_new_process",
                           layout="single", size="120x36", **timing, resources=session.resources()))
                restore_rects = business_rects(current)
                restore_cue = frame_cue(session.frames[-1]["cells"], restore_rects)
                expected_pids = set(session.new_process_pids[identifier])
                session.tap(current, f"workspace.minimize.{identifier}")
                current = session.until(lambda frame: not any(item["id"].endswith(f"workspace.title.{identifier}")
                                                              for item in titles(frame))
                                        and bool(session.frames) and not restore_cue(session.frames[-1]))
                capture = session.capture_output(
                    lambda: session.tap(current, f"workspace.taskbar.{identifier}"),
                    restore_cue, f"restore-visible/{app}")
                verification_started = time.monotonic_ns()
                before_rpc = session.rpc_count
                selected = lambda frame: any(
                    item["id"].endswith(f"workspace.title.{identifier}") and item["selected"] for item in titles(frame))
                ready = lambda frame: selected(frame) and frame["page"] == app and any(
                    item["id"].startswith("page/") and item["actionable"] for item in frame["nodes"])
                current = session.verify_capture(capture, selected)
                # Retained content can be visible before fresh viewport/fence
                # readiness. This check is auxiliary and cannot move the seal.
                try:
                    if not ready(current):
                        current = session.until(ready)
                    restored_pids = {item["pid"] for item in session.resources()["processes"]}
                    assert expected_pids <= restored_pids, "Restore did not retain the same process"
                except BaseException as error:
                    session.failed_attempt(capture, f"Post-seal auxiliary restore validation failed: {error}")
                    raise
                timing = session.captured_timing(capture, current, restore_rects)
                timing["endpoint_kind"] = "restored_business_content_visible"
                timing["post_capture_validation"] = {
                    "current_viewport_actionable": True, "same_process_pids": sorted(expected_pids),
                    "elapsed_ms": (time.monotonic_ns() - verification_started) / 1e6,
                    "snapshot_requests": session.rpc_count - before_rpc,
                    "note": "Post-seal observation only; not an exact AppHost ready time or part of visible latency.",
                }
                write(dict(kind="launch", application=app, index=index, session=identifier,
                           mode="restore_existing_process", layout="single", size="120x36", **timing))
                session.tap(current, f"workspace.close.{identifier}")
                session.until(lambda frame: not any(item["id"].endswith(f"workspace.taskbar.{identifier}")
                                                    for item in frame["nodes"]))
            print(f"PASS: {app}: {samples} independent AppHosts", flush=True)
        finally:
            session.close()


def choose(session, query, suffix):
    session.until(lambda frame: any(item["id"].endswith("workspace.choose.query") for item in frame["nodes"]))
    session.send(b"\x1b[200~" + query.encode() + b"\x1b[201~")
    current = session.until(lambda frame: any(item["id"].endswith(suffix) and item["actionable"] for item in frame["nodes"]))
    session.tap(current, suffix)
    return session.until(lambda frame: not any(item["id"].endswith("workspace.choose.query") for item in frame["nodes"]))


def children(binary, output, samples, motion, write, detailed_trace=False):
    for app in ("CommandLine", "Vim"):
        session = Session(binary, output, f"child-{app}", motion=motion, detailed_trace=detailed_trace)
        try:
            for index in range(samples):
                if app == "CommandLine":
                    current, timing = session.open_app("CommandLine", terminal_marker=">>")
                else:
                    current, _ = session.open_app("Editor", timed=False)
                    rect = vim_terminal_rect(node(current, "page")["rect"])
                    before = region(session.frames[-1]["cells"], rect)
                    cue = lambda candidate: vim_buffer_ready(candidate["cells"], rect, before)
                    capture = session.capture_output(lambda: session.send(b"\x1b[19~"), cue, "native-ready/Vim")
                    current = session.verify_capture(capture, lambda frame: any(
                        item["role"] == "terminal" and "~" in item["text"] for item in frame["nodes"]))
                    terminal = next(item for item in current["nodes"] if item["role"] == "terminal")
                    if terminal["rect"] != rect or not vim_buffer_ready(snapshot_cells(current), rect, before):
                        session.failed_attempt(capture, "Post-capture native Vim buffer validation failed")
                        raise AssertionError("Post-capture native Vim buffer validation failed")
                    timing = session.captured_timing(capture, current, vim_ready_rects(rect))
                    timing["verification"].update(native_endpoint=NATIVE_VIM_ENDPOINT,
                                                  native_buffer_rect=rect, intro_ignored=True)
                write(dict(kind="child-ready", application=app, index=index,
                           ready="visible REPL prompt" if app == "CommandLine" else "visible native Vim buffer",
                           **timing, resources=session.resources()))
                identifier = next(item for item in titles(current) if item["selected"])["id"].rsplit(".", 1)[1]
                if app == "Vim":
                    session.send(b"\x1b[19~")
                    current = session.until(lambda frame: not any(item["role"] == "terminal" for item in frame["nodes"]))
                    session.tap(current, f"workspace.close.{identifier}")
                else:
                    session.send(b"exit\r")
                session.until(lambda frame: not any(item["id"].endswith(f"workspace.taskbar.{identifier}")
                                                    for item in frame["nodes"]))
            print(f"PASS: {app}: {samples} child-ready samples", flush=True)
        finally:
            session.close()


def interactions(binary, output, samples, motion, write, single_samples=None, detailed_trace=False):
    for count, width, height in ((1, 120, 36), (2, 217, 39), (4, 217, 39)):
        case_samples = single_samples if count == 1 and single_samples is not None else samples
        session = Session(binary, output, f"interact-{count}", width, height, motion, detailed_trace=detailed_trace)
        try:
            current, _ = session.open_app("Launcher", timed=False)
            original = titles(current)[0]["id"].rsplit(".", 1)[1]
            if count > 1:
                session.send(b"\x17v")
                current = choose(session, "Launcher", "application.launcher")
            if count == 4:
                session.send(b"\x17s")
                current = choose(session, "Launcher", "application.launcher")
                session.tap(current, f"workspace.title.{original}")
                session.send(b"\x17s")
                current = choose(session, "Launcher", "application.launcher")
            current = session.until(lambda frame: len(titles(frame)) == count and frame["page"] == "Launcher")
            selected_titles = {}
            if count > 1:
                initially_selected = next(item["id"] for item in titles(current) if item["selected"])
                for identifier in [item["id"] for item in titles(current)]:
                    session.tap(current, identifier)
                    current = session.until(lambda frame: node(frame, identifier)["selected"])
                    selected_titles[identifier] = (session.frames[-1]["cells"], [node(current, identifier)["rect"]])
                session.tap(current, initially_selected)
                current = session.until(lambda frame: node(frame, initially_selected)["selected"])
            for index in range(case_samples):
                # Each sample is one physical input, then complete output, then verification.
                current = prepare_mouse_search(session)
                search_rect = node(current, "page/launcher.search")["rect"]
                focused_cells = session.frames[-1]["cells"]
                marker = chr(0x0400 + index % 200)
                capture = session.capture_output(lambda: session.send(marker.encode()),
                    lambda candidate: marker in "\n".join(region(candidate["cells"], search_rect)),
                    f"{count}-window/input")
                current = session.verify_capture(capture, lambda frame: marker in node(frame, "page/launcher.search")["text"])
                timing = session.captured_timing(capture, current, search_rect)
                write(dict(kind="interaction", action="input", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                session.send(b"\x01\x7f\t")
                current = session.until(lambda frame: search_state_observed(session, frame, False))
                rect = node(current, "page/launcher.search")["rect"]
                before = region(session.frames[-1]["cells"], rect, True)
                unfocused_cells = session.frames[-1]["cells"]
                unfocused_state = focus_state(current)
                capture = session.capture_output(lambda: session.tap(current, "page/launcher.search"),
                    frame_cue(focused_cells, [rect], True), f"{count}-window/click")
                current = session.verify_capture(capture, lambda frame: node(frame, "page/launcher.search")["focused"])
                timing = session.captured_timing(capture, current, rect, True, before)
                write(dict(kind="interaction", action="click", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                rect = search_rect
                before = region(session.frames[-1]["cells"], rect, True)
                capture = session.capture_output(lambda: session.send(b"\t"),
                    frame_cue(unfocused_cells, [rect], True), f"{count}-window/focus")
                current = session.verify_capture(capture, lambda frame:
                    not node(frame, "page/launcher.search")["focused"] and focus_state(frame) == unfocused_state)
                timing = session.captured_timing(capture, current, rect, True, before)
                timing["verification"]["logical_focus_state"] = unfocused_state
                write(dict(kind="interaction", action="focus", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                if count > 1:
                    other = next(item for item in titles(current) if not item["selected"])
                    rect = other["rect"]
                    before = region(session.frames[-1]["cells"], rect, True)
                    identifier = other["id"]
                    capture = session.capture_output(lambda: session.tap(current, identifier),
                        frame_cue(*selected_titles[identifier], styled=True), f"{count}-window/pane-focus")
                    current = session.verify_capture(capture, lambda frame: node(frame, identifier)["selected"])
                    timing = session.captured_timing(capture, current, rect, True, before)
                    write(dict(kind="interaction", action="pane-focus", index=index,
                               layout=f"{count}-window", size=f"{width}x{height}", **timing))
            write(dict(kind="resources", layout=f"{count}-window", resources=session.resources()))
            print(f"PASS: {count} windows: {case_samples} samples per interaction type", flush=True)
        finally:
            session.close()


def report(samples, metadata):
    groups = defaultdict(list)
    resources = defaultdict(list)
    for sample in samples:
        if sample["kind"] == "launch":
            key = f"launch/{sample['application']}/{sample['mode']}"
            if sample["mode"] != "restore_existing_process":
                groups[f"launch/{sample['application']}/all_new_processes"].append(sample["latency_ms"])
        elif sample["kind"] == "interaction":
            key = f"interaction/{sample['layout']}/{sample['action']}"
        elif sample["kind"] == "child-ready":
            key = f"child-ready/{sample['application']}"
        elif sample["kind"] == "resources":
            resources[sample["layout"]].append(sample["resources"])
            continue
        else:
            continue
        groups[key].append(sample["latency_ms"])
        if "resources" in sample:
            resources[key].append(sample["resources"])
    resource_summaries = {}
    for key, values in resources.items():
        resource_summaries[key] = {
            "max_tree_rss_bytes": max(value["rss_bytes"] for value in values),
            "max_live_tree_cpu_seconds": max(value["cpu_seconds"] for value in values),
            "note": "Live process-tree point samples; exited workers are excluded from CPU totals.",
        }
    return dict(metadata, summaries={key: statistics(values) for key, values in sorted(groups.items())},
                resources=resource_summaries)


def diagnostics(output):
    durations = defaultdict(list)
    for path in output.glob("*.startup.jsonl"):
        for line in path.read_text().splitlines():
            event = json.loads(line)
            context = event.get("context", {})
            for field, value in event.get("message_args", {}).items():
                if field.endswith("_us") and isinstance(value, (int, float)):
                    durations[f"{context.get('app')}/{context.get('operation')}/{field}"].append(value / 1000)
    for path in output.glob("*.trace.jsonl"):
        for line in path.read_text().splitlines():
            event = json.loads(line)
            for field, value in event.items():
                if field.endswith("_us") and isinstance(value, (int, float)):
                    durations[f"{path.stem}/{event['event']}/{field}"].append(value / 1000)
    return {key: statistics(values) for key, values in sorted(durations.items())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--launch-samples", type=int, default=50)
    parser.add_argument("--interaction-samples", type=int, default=100)
    parser.add_argument("--single-interaction-samples", type=int,
                        help="Override only the single-window interaction count for diagnostic runs")
    parser.add_argument("--child-samples", type=int, default=10)
    parser.add_argument("--phase", choices=("launch", "interaction", "child", "all"), default="all")
    parser.add_argument("--application", choices=APPLICATIONS, action="append",
                        help="Limit new-window series to these applications; other phases are unchanged")
    parser.add_argument("--motion", choices=("full", "reduced"), default="full")
    parser.add_argument("--detailed-trace", action="store_true",
                        help="Record optional per-frame/input timing logs for separate diagnostics; adds observation overhead")
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--enforce-budgets", action="store_true")
    args = parser.parse_args()
    assert args.launch_samples >= 1 and args.interaction_samples >= 1 and args.child_samples >= 1
    assert args.single_interaction_samples is None or args.single_interaction_samples >= 1
    binary = args.binary.resolve()
    assert binary.is_file() and binary.with_name("tundra-cli").is_file()
    args.output.mkdir(parents=True, exist_ok=False)
    stat = binary.stat()
    metadata = {"binary": str(binary), "binary_size": stat.st_size,
                "binary_mtime_ns": stat.st_mtime_ns,
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "platform": platform.platform(), "python": platform.python_version(),
                "motion": args.motion, "terminal": "xterm-256color/truecolor",
                "trace_enabled": args.detailed_trace,
                "observation_condition": "zero-rpc-sealed-pty-capture-v1",
                "capture_timeout_seconds": CAPTURE_TIMEOUT_SECONDS,
                "unknown_signature_discovery_seconds": CAPTURE_TIMEOUT_SECONDS,
                "native_vim_endpoint": NATIVE_VIM_ENDPOINT,
                "input_focus_endpoint": FOCUS_ENDPOINT,
                "restore_endpoint": "Complete business content visible again; viewport/fence readiness is verified after sealing",
                "resource_sample_timing": "Process-tree points outside capture, after validation; not endpoint CPU/RSS or peak memory",
                "measurement": "Python monotonic_ns from physical PTY action to the first matching complete Ratatui output tail",
                "boundary": "Ratatui buffer diff followed by final cursor state and position commands and flush; snapshot verification is excluded",
                "limitation": "PTY output completion is measured; terminal emulator pixel presentation is not measured",
                "requested_phase": args.phase, "completed": False,
                "applications_requested": args.application or list(APPLICATIONS),
                "launch_samples_requested": args.launch_samples,
                "child_samples_requested": args.child_samples,
                "single_window_interaction_samples_requested": args.single_interaction_samples or args.interaction_samples,
                "interaction_samples_requested_per_type": args.interaction_samples}
    environment = binary.parent / "build-environment.json"
    if environment.is_file():
        metadata["build_environment"] = json.loads(environment.read_text())
    metadata["acceptance_eligible"] = (not args.detailed_trace
        and metadata.get("build_environment", {}).get("acceptance_eligible", True))
    if args.compare:
        comparison = json.loads(args.compare.read_text())
        assert comparison.get("observation_condition") == metadata["observation_condition"], \
            "Cannot compare different automation observation conditions"
        assert comparison.get("trace_enabled") == metadata["trace_enabled"], \
            "Cannot compare different detailed-trace settings"
        assert comparison.get("native_vim_endpoint") == metadata["native_vim_endpoint"], \
            "Cannot compare different native Vim ready definitions"
        assert comparison.get("input_focus_endpoint") == metadata["input_focus_endpoint"], \
            "Cannot compare different keyboard focus definitions"
        assert metadata["acceptance_eligible"] and comparison.get("acceptance_eligible"), \
            "Diagnostic compiler profiles and detailed tracing cannot certify a baseline comparison"
    limits = {}
    for name in ("cpu.max", "memory.max", "cpuset.cpus.effective"):
        path = Path("/sys/fs/cgroup") / name
        if path.is_file():
            limits[name] = path.read_text().strip()
    metadata["environment_preflight"] = {
        "captured_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "loadavg": Path("/proc/loadavg").read_text().strip(), "cgroup_limits": limits,
        "processes": subprocess.run(["ps", "-eo", "pid,ppid,comm,pcpu,stat", "--sort=-pcpu"],
                                    capture_output=True, text=True, check=True).stdout,
    }
    (args.output / "environment-preflight.json").write_text(
        json.dumps(metadata["environment_preflight"], indent=2) + "\n"
    )
    samples = []
    started = time.monotonic()
    with (args.output / "samples.jsonl").open("w") as raw:
        def write(sample):
            samples.append(sample)
            raw.write(json.dumps(sample, ensure_ascii=False) + "\n")
            raw.flush()
        try:
            if args.phase in ("launch", "all"):
                launches(binary, args.output, args.launch_samples, args.motion, write,
                         args.application or APPLICATIONS, detailed_trace=args.detailed_trace)
            if args.phase in ("interaction", "all"):
                interactions(binary, args.output, args.interaction_samples, args.motion, write,
                             args.single_interaction_samples, detailed_trace=args.detailed_trace)
            if args.phase in ("child", "all"):
                children(binary, args.output, args.child_samples, args.motion, write, detailed_trace=args.detailed_trace)
            metadata["completed"] = True
        finally:
            result = report(samples, dict(metadata, elapsed_seconds=round(time.monotonic() - started, 3)))
            result["stage_timings"] = diagnostics(args.output)
            failures = args.output / "failed-attempts.jsonl"
            result["failed_attempts"] = [json.loads(line) for line in failures.read_text().splitlines()] if failures.exists() else []
            result["acceptance"] = {}
            for key, stats in result["summaries"].items():
                if key.startswith("interaction/") or key.endswith("restore_existing_process"):
                    budget = 50
                elif key.endswith("all_new_processes"):
                    budget = 100
                else:
                    continue
                result["acceptance"][key] = {"budget_ms": budget,
                    "passed": stats["p95_ms"] <= budget,
                    "enough_samples": stats["count"] >= (50 if key.startswith("launch/") else 100)}
                if key.endswith("restore_existing_process"):
                    result["acceptance"][key]["endpoint_kind"] = "restored_business_content_visible"
                    result["acceptance"][key]["exact_interactive_readiness_measured"] = False
            (args.output / "report.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
            checks = result["acceptance"].values()
            lines = [f"Requested run completed: {result['completed']} (workflow completion only).", "",
                     f"trace_enabled: {result['trace_enabled']}; normal product logs are retained.", "",
                     f"Build/trace acceptance_eligible: {result['acceptance_eligible']} (sample-size and budget checks remain separate).", "",
                     f"Observation condition: {result['observation_condition']}. Failed attempts retained: {len(result['failed_attempts'])}.", "",
                     "Restore measures business content becoming visible again. Current viewport/fence readiness and same-process checks occur after sealing; their duration is auxiliary.", "",
                     "Vim uses the first changed empty buffer with complete EOB rows and the Scratch status line, ignoring intro variants. Keyboard focus uses only the search-field style and verifies the logical target afterward.", "",
                     f"Measured budget groups: {len(result['acceptance'])}; "
                     f"enough samples: {sum(item['enough_samples'] for item in checks)}; "
                     f"within P95 budget: {sum(item['passed'] for item in checks)}.", "",
                     "Reduced samples or a partial matrix do not certify overall acceptance. "
                     "See report.json for each group's sample-size and budget checks.", "",
                     "| Case | N | P50 ms | P95 ms | P99 ms |", "| --- | ---: | ---: | ---: | ---: |"]
            for key, stats in result["summaries"].items():
                lines.append(f"| {key} | {stats['count']} | {stats['p50_ms']} | {stats['p95_ms']} | {stats['p99_ms']} |")
            if args.compare:
                baseline = json.loads(args.compare.read_text())
                lines.extend(["", "| Case | Baseline P95 ms | Candidate P95 ms | Change ms |",
                              "| --- | ---: | ---: | ---: |"])
                for key, stats in result["summaries"].items():
                    old = baseline["summaries"].get(key)
                    if old:
                        lines.append(f"| {key} | {old['p95_ms']} | {stats['p95_ms']} | {stats['p95_ms'] - old['p95_ms']:.3f} |")
            lines.extend(["", metadata["measurement"] + ".", "", metadata["limitation"] + "."])
            (args.output / "report.md").write_text("\n".join(lines) + "\n")
    print(json.dumps(result, ensure_ascii=False))
    if args.enforce_budgets:
        assert result["completed"] and not result["failed_attempts"] and result["acceptance_eligible"] \
            and result["acceptance"] and all(item["passed"] and item["enough_samples"]
                                            for item in result["acceptance"].values()), "Performance budgets failed; see report.json"


if __name__ == "__main__":
    main()
