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
                    if raw == "?25" and command in "hl":
                        tails.append(tuple(tuple(row) for row in self.cells))
                    if re.fullmatch(r"[0-9;]*", raw):
                        values = [int(value or "0") for value in raw.split(";")]
                        amount = values[0] or 1
                        if command in "Hf":
                            self.y = amount - 1
                            self.x = ((values[1] if len(values) > 1 else 1) or 1) - 1
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


def percentile(values, fraction):
    return round(sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)], 3)


def statistics(values):
    return {"count": len(values), "p50_ms": percentile(values, .50),
            "p95_ms": percentile(values, .95), "p99_ms": percentile(values, .99),
            "max_ms": round(max(values), 3)}


class Session:
    def __init__(self, binary, output, label, width=120, height=36, motion="full"):
        self.binary, self.cli = binary, binary.with_name("tundra-cli")
        self.output, self.label = output, label
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
        self.frames = []
        self.bytes = 0
        self.info = None
        self.child = subprocess.Popen([str(binary)], env=self.env, cwd=root / "home",
                                      stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                      start_new_session=True)
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

    def drain(self, timeout=0):
        if not select.select([self.master], [], [], timeout)[0]:
            return b""
        try:
            data = os.read(self.master, 256 * 1024)
        except (BlockingIOError, OSError):
            return b""
        observed = time.monotonic_ns()
        self.raw.write(data)
        self.bytes += len(data)
        for cells in self.screen.feed(data):
            self.frames.append({"observed_ns": observed, "byte_offset": self.bytes, "cells": cells})
        return data

    def send(self, value):
        self.drain()
        started = time.monotonic_ns()
        os.write(self.master, value)
        return started

    def control(self, action):
        assert self.info
        with socket.socket(socket.AF_UNIX) as connection:
            connection.connect(self.info["endpoint"])
            connection.sendall(json.dumps(dict(protocol=1, **action)).encode() + b"\n")
            connection.setblocking(False)
            reply = b""
            deadline = time.monotonic() + 20
            while b"\n" not in reply and time.monotonic() < deadline:
                ready, _, _ = select.select([self.master, connection], [], [], .1)
                if self.master in ready:
                    self.drain()
                if connection in ready:
                    chunk = connection.recv(256 * 1024)
                    assert chunk, "Shell closed control connection"
                    reply += chunk
                assert self.child.poll() is None, f"Shell exited: {self.child.returncode}"
            assert b"\n" in reply, "Shell control timed out"
            response = json.loads(reply)
            if response.get("error", {}).get("code") == "not_ready":
                return None
            assert response["ok"], response
            self.drain()
            return response["snapshot"]

    def snapshot(self):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            snapshot = self.control({"command": "snapshot"})
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

    def match_output(self, started, frame, rect=None, styled=False, changed_from=None):
        assert self.frames, "No completed PTY frame"
        rect = rect or node(frame, "page")["rect"]
        expected = region(self.frames[-1]["cells"], rect, styled)
        if not styled:
            assert any(line.strip() for line in expected), "Blank region cannot prove a business frame"
        if changed_from is not None:
            assert expected != changed_from, "Action produced no visible feedback"
        matches = [item for item in self.frames if item["observed_ns"] >= started
                   and region(item["cells"], rect, styled) == expected]
        assert matches, "No complete PTY frame matches the verified business content"
        matched = matches[0]
        return {"latency_ms": (matched["observed_ns"] - started) / 1_000_000,
                "action_ns": started, "output_ns": matched["observed_ns"],
                "output_byte_offset": matched["byte_offset"],
                "verification": {"snapshot": frame["number"], "page": frame["page"],
                                 "rect": rect, "styled": styled,
                                 "content_sha256": hashlib.sha256(json.dumps(expected, ensure_ascii=False).encode()).hexdigest()}}

    def open_app(self, app):
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
        self.frames.clear()
        started = self.tap(current, target["id"])
        result = self.until(lambda frame: frame["page"] == app and bool(titles(frame))
                            and any(item["id"].startswith("page/") and item["actionable"]
                                    for item in frame["nodes"]))
        return result, self.match_output(started, result)

    def resources(self):
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
            self.raw.close()
            after = termios.tcgetattr(self.slave)
            os.close(self.master)
            os.close(self.slave)
            self.temporary.cleanup()
            assert after == self.before, "Shell failed to restore terminal attributes"


def launches(binary, output, samples, motion, write):
    for app in APPLICATIONS:
        session = Session(binary, output, f"launch-{app}", motion=motion)
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
                session.tap(current, f"workspace.minimize.{identifier}")
                current = session.until(lambda frame: not any(item["id"].endswith(f"workspace.title.{identifier}")
                                                              for item in titles(frame)))
                session.frames.clear()
                started = session.tap(current, f"workspace.taskbar.{identifier}")
                current = session.until(lambda frame: frame["page"] == app
                                        and any(item["id"].endswith(f"workspace.title.{identifier}")
                                                and item["selected"] for item in titles(frame)))
                timing = session.match_output(started, current)
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


def children(binary, output, samples, motion, write):
    for app in ("CommandLine", "Vim"):
        session = Session(binary, output, f"child-{app}", motion=motion)
        try:
            for index in range(samples):
                if app == "CommandLine":
                    current, opened = session.open_app("CommandLine")
                    started = opened["action_ns"]
                    current = session.until(lambda frame: any(item["role"] == "terminal"
                                            and ">>" in item["text"] for item in frame["nodes"]))
                else:
                    current, _ = session.open_app("Editor")
                    session.frames.clear()
                    started = session.send(b"\x1b[19~")
                    current = session.until(lambda frame: any(item["role"] == "terminal"
                                            and "~" in item["text"] for item in frame["nodes"]))
                terminal = next(item for item in current["nodes"] if item["role"] == "terminal")
                timing = session.match_output(started, current, terminal["rect"])
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


def interactions(binary, output, samples, motion, write):
    for count, width, height in ((1, 120, 36), (2, 217, 39), (4, 217, 39)):
        session = Session(binary, output, f"interact-{count}", width, height, motion)
        try:
            current, _ = session.open_app("Launcher")
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
            for index in range(samples):
                # Each sample is one physical input, then complete output, then verification.
                current = session.snapshot()
                session.tap(current, "page/launcher.search")
                session.until(lambda frame: node(frame, "page/launcher.search")["focused"])
                session.send(b"\x15")
                current = session.until(lambda frame: node(frame, "page/launcher.search")["text"].rsplit(":", 1)[-1].strip() in ("", "_"))
                marker = chr(0x0400 + index % 200)
                session.frames.clear()
                started = session.send(marker.encode())
                current = session.until(lambda frame: marker in node(frame, "page/launcher.search")["text"])
                timing = session.match_output(started, current, node(current, "page/launcher.search")["rect"])
                write(dict(kind="interaction", action="input", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                session.send(b"\x15\t")
                current = session.until(lambda frame: not node(frame, "page/launcher.search")["focused"])
                rect = node(current, "page/launcher.search")["rect"]
                before = region(session.frames[-1]["cells"], rect, True)
                session.frames.clear()
                started = session.tap(current, "page/launcher.search")
                current = session.until(lambda frame: node(frame, "page/launcher.search")["focused"])
                timing = session.match_output(started, current, rect, True, before)
                write(dict(kind="interaction", action="click", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                rect = node(current, "page")["rect"]
                before = region(session.frames[-1]["cells"], rect, True)
                session.frames.clear()
                started = session.send(b"\t")
                current = session.until(lambda frame: not node(frame, "page/launcher.search")["focused"])
                timing = session.match_output(started, current, rect, True, before)
                write(dict(kind="interaction", action="focus", index=index, layout=f"{count}-window",
                           size=f"{width}x{height}", **timing))
                if count > 1:
                    other = next(item for item in titles(current) if not item["selected"])
                    rect = other["rect"]
                    before = region(session.frames[-1]["cells"], rect, True)
                    session.frames.clear()
                    started = session.tap(current, other["id"])
                    identifier = other["id"]
                    current = session.until(lambda frame: node(frame, identifier)["selected"])
                    timing = session.match_output(started, current, rect, True, before)
                    write(dict(kind="interaction", action="pane-focus", index=index,
                               layout=f"{count}-window", size=f"{width}x{height}", **timing))
            write(dict(kind="resources", layout=f"{count}-window", resources=session.resources()))
            print(f"PASS: {count} windows: {samples} samples per interaction type", flush=True)
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
    parser.add_argument("--child-samples", type=int, default=10)
    parser.add_argument("--phase", choices=("launch", "interaction", "child", "all"), default="all")
    parser.add_argument("--motion", choices=("full", "reduced"), default="full")
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--enforce-budgets", action="store_true")
    args = parser.parse_args()
    assert args.launch_samples >= 1 and args.interaction_samples >= 1 and args.child_samples >= 1
    binary = args.binary.resolve()
    assert binary.is_file() and binary.with_name("tundra-cli").is_file()
    args.output.mkdir(parents=True, exist_ok=False)
    stat = binary.stat()
    metadata = {"binary": str(binary), "binary_size": stat.st_size,
                "binary_mtime_ns": stat.st_mtime_ns,
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "platform": platform.platform(), "python": platform.python_version(),
                "motion": args.motion, "terminal": "xterm-256color/truecolor",
                "measurement": "Python monotonic_ns from physical PTY action to the first matching complete Ratatui output tail",
                "boundary": "Ratatui buffer diff followed by Crossterm Hide/Show cursor and flush; snapshot verification is excluded",
                "limitation": "PTY output completion is measured; terminal emulator pixel presentation is not measured",
                "requested_phase": args.phase, "completed": False,
                "launch_samples_requested": args.launch_samples,
                "child_samples_requested": args.child_samples,
                "interaction_samples_requested_per_type": args.interaction_samples}
    environment = binary.parent / "build-environment.json"
    if environment.is_file():
        metadata["build_environment"] = json.loads(environment.read_text())
    samples = []
    started = time.monotonic()
    with (args.output / "samples.jsonl").open("w") as raw:
        def write(sample):
            samples.append(sample)
            raw.write(json.dumps(sample, ensure_ascii=False) + "\n")
            raw.flush()
        try:
            if args.phase in ("launch", "all"):
                launches(binary, args.output, args.launch_samples, args.motion, write)
            if args.phase in ("interaction", "all"):
                interactions(binary, args.output, args.interaction_samples, args.motion, write)
            if args.phase in ("child", "all"):
                children(binary, args.output, args.child_samples, args.motion, write)
            metadata["completed"] = True
        finally:
            result = report(samples, dict(metadata, elapsed_seconds=round(time.monotonic() - started, 3)))
            result["stage_timings"] = diagnostics(args.output)
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
            (args.output / "report.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
            lines = ["| Case | N | P50 ms | P95 ms | P99 ms |", "| --- | ---: | ---: | ---: | ---: |"]
            for key, stats in result["summaries"].items():
                lines.append(f"| {key} | {stats['count']} | {stats['p50_ms']} | {stats['p95_ms']} | {stats['p99_ms']} |")
            if args.compare:
                baseline = json.loads(args.compare.read_text())
                lines.extend(["", "| Case | Baseline P95 ms | Final P95 ms | Change ms |",
                              "| --- | ---: | ---: | ---: |"])
                for key, stats in result["summaries"].items():
                    old = baseline["summaries"].get(key)
                    if old:
                        lines.append(f"| {key} | {old['p95_ms']} | {stats['p95_ms']} | {stats['p95_ms'] - old['p95_ms']:.3f} |")
            lines.extend(["", metadata["measurement"] + ".", "", metadata["limitation"] + "."])
            (args.output / "report.md").write_text("\n".join(lines) + "\n")
    print(json.dumps(result, ensure_ascii=False))
    if args.enforce_budgets:
        assert result["acceptance"] and all(item["passed"] and item["enough_samples"]
                                            for item in result["acceptance"].values()), "Performance budgets failed; see report.json"


if __name__ == "__main__":
    main()
