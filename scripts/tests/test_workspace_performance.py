"""Check complete PTY frame timing, VT state, and optional trace isolation."""
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "workspace_performance", Path(__file__).with_name("workspace-performance.py")
)
performance = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(performance)


class FrameBoundaryTests(unittest.TestCase):
    def test_split_frame_tail_does_not_finish_early(self):
        screen = performance.Screen(8, 2)
        self.assertEqual(screen.feed(b"\x1b[1;1Hready\x1b[?2"), [])
        frames = screen.feed(b"5l")
        self.assertEqual(len(frames), 1)
        self.assertEqual("".join(cell[0] for cell in frames[0][0]), "ready   ")

    def test_style_preserves_foreground_across_background_change(self):
        screen = performance.Screen(3, 1)
        frames = screen.feed(b"\x1b[38;2;1;2;3m\x1b[48;2;4;5;6mA\x1b[49mB\x1b[0mC\x1b[?25l")
        self.assertEqual(frames[0][0][0][1], ((2, 1, 2, 3), (2, 4, 5, 6), ()))
        self.assertEqual(frames[0][0][1][1], ((2, 1, 2, 3), None, ()))
        self.assertEqual(frames[0][0][2][1], (None, None, ()))

    def test_visible_cursor_frame_waits_for_its_final_position(self):
        screen = performance.Screen(8, 2)
        self.assertEqual(screen.feed(b"ready\x1b[?25h"), [])
        frames = screen.feed(b"\x1b[1;6H")
        self.assertEqual(len(frames), 1)
        self.assertEqual((screen.x, screen.y), (5, 0))

    def test_synchronized_frame_waits_for_the_end_across_every_byte(self):
        screen = performance.Screen(8, 2)
        data = "\x1b[?2026h\x1b[2J\x1b[1;1H中ready\x1b[?25l\x1b[?25h\x1b[2;1Hdone\x1b[?2026l".encode()
        for byte in data[:-1]:
            self.assertEqual(screen.feed(bytes((byte,))), [])
        frames = screen.feed(data[-1:])
        self.assertEqual(len(frames), 1)
        self.assertEqual("".join(cell[0] for cell in frames[0][0]), "中 ready ")
        self.assertEqual("".join(cell[0] for cell in frames[0][1]), "done    ")

    def test_unicode_and_cursor_diffs_survive_chunk_boundaries(self):
        screen = performance.Screen(8, 2)
        data = "\x1b[1;1H中Ω\x1b[2;3Hok\x1b[?25l".encode()
        frames = []
        for byte in data:
            frames.extend(screen.feed(bytes((byte,))))
        self.assertEqual(len(frames), 1)
        self.assertEqual("".join(cell[0] for cell in frames[0][0]), "中 Ω     ")
        self.assertEqual("".join(cell[0] for cell in frames[0][1]), "  ok    ")

    def test_snapshot_text_preserves_wide_cell_positions_for_sealed_validation(self):
        screen = performance.Screen(8, 2)
        rendered = screen.feed("\x1b[1;1HA中Ω\x1b[2;1HZ\x1b[?25l".encode())[0]
        snapshot = {"width": 8, "height": 2, "lines": ["A中Ω", "Z"]}
        self.assertEqual(performance.snapshot_cells(snapshot), rendered)

    def test_blank_or_unchanged_regions_cannot_prove_feedback(self):
        session = performance.Session.__new__(performance.Session)
        rect = {"x": 0, "y": 0, "width": 3, "height": 1}
        cells = (((" ", ()), (" ", ()), (" ", ())),)
        session.frames = [{"observed_ns": 100, "byte_offset": 1, "cells": cells}]
        with self.assertRaisesRegex(AssertionError, "Blank"):
            session.match_output(90, {}, rect)
        before = performance.region(cells, rect, True)
        with self.assertRaisesRegex(AssertionError, "no visible feedback"):
            session.match_output(90, {}, rect, True, before)

    def test_background_footer_changes_do_not_delay_business_frame(self):
        session = performance.Session.__new__(performance.Session)
        screen = performance.Screen(6, 2)
        first = screen.feed(b"\x1b[1;1HSearch\x1b[2;1Hwait\x1b[?25l")[0]
        later = screen.feed(b"\x1b[2;1Hdone\x1b[?25l")[0]
        session.frames = [{"observed_ns": 100, "byte_offset": 30, "cells": first},
                          {"observed_ns": 200, "byte_offset": 60, "cells": later}]
        frame = {"number": 2, "page": "Launcher", "nodes": [{
            "id": "page/search", "actionable": True, "role": "textbox", "text": "Search",
            "rect": {"x": 0, "y": 0, "width": 6, "height": 1}}]}
        self.assertEqual(session.match_output(90, frame)["output_ns"], 100)

    def test_live_metric_values_do_not_delay_interactive_dashboard(self):
        session = performance.Session.__new__(performance.Session)
        screen = performance.Screen(8, 3)
        first = screen.feed(b"\x1b[1;1HCPU\x1b[2;1HLoading\x1b[3;1HRefresh\x1b[?25l")[0]
        later = screen.feed(b"\x1b[2;1H50%    \x1b[?25l")[0]
        session.frames = [{"observed_ns": 100, "byte_offset": 50, "cells": first},
                          {"observed_ns": 200, "byte_offset": 70, "cells": later}]
        frame = {"number": 2, "page": "SystemStatus", "nodes": [{
            "id": "page/system-status.widget.Cpu", "actionable": True, "role": "button", "text": "CPU\n50%",
            "rect": {"x": 0, "y": 0, "width": 8, "height": 2}}, {
            "id": "page/refresh", "actionable": True, "role": "button", "text": "Refresh",
            "rect": {"x": 0, "y": 2, "width": 8, "height": 1}}]}
        self.assertEqual(session.match_output(90, frame)["output_ns"], 100)


class SessionEnvironmentTests(unittest.TestCase):
    def session_environment(self, detailed_trace=None):
        captured = {}

        class StopBeforeLaunch(Exception):
            pass

        def capture_environment(*args, **kwargs):
            captured.update(kwargs["env"])
            raise StopBeforeLaunch()

        with tempfile.TemporaryDirectory(prefix="tundra-perf-trace-test-") as folder:
            output = Path(folder)
            session = performance.Session.__new__(performance.Session)
            options = {} if detailed_trace is None else {"detailed_trace": detailed_trace}
            with patch.dict(os.environ, {"TUNDRA_PERF_TRACE": "/inherited/trace.jsonl"}), \
                    patch.object(performance.subprocess, "run", return_value=SimpleNamespace(returncode=0, stderr=b"")), \
                    patch.object(performance.subprocess, "Popen", side_effect=capture_environment):
                try:
                    with self.assertRaises(StopBeforeLaunch):
                        session.__init__(output / "tundra-shell", output, "test", **options)
                    self.assertEqual(os.environ["TUNDRA_PERF_TRACE"], "/inherited/trace.jsonl")
                finally:
                    for name in ("raw", "frame_index"):
                        stream = getattr(session, name, None)
                        if stream is not None:
                            stream.close()
                    for name in ("master", "slave"):
                        descriptor = getattr(session, name, None)
                        if descriptor is not None:
                            os.close(descriptor)
                    self.addCleanup(session.temporary.cleanup)
            return captured, str(output / "test.trace.jsonl")

    def test_macos_session_home_contains_readable_trash(self):
        with patch.object(performance.sys, "platform", "darwin"):
            environment, _ = self.session_environment()
        home = Path(environment["HOME"])
        self.assertNotEqual(home, Path.home())
        self.assertEqual(list((home / ".Trash").iterdir()), [])

    def test_default_session_removes_inherited_detailed_trace(self):
        environment, _ = self.session_environment()
        self.assertIsNone(environment.get("TUNDRA_PERF_TRACE"))

    def test_detailed_trace_explicitly_uses_this_runs_output(self):
        environment, expected = self.session_environment(True)
        self.assertEqual(environment.get("TUNDRA_PERF_TRACE"), expected)


class CaptureFixture(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="tundra-capture-test-")
        self.addCleanup(self.temporary.cleanup)
        self.clock = SimpleNamespace(now=0)
        self.session = performance.Session.__new__(performance.Session)
        self.session.output = Path(self.temporary.name)
        self.session.label = "test"
        self.session.frames = []
        self.session.frame_number = 0
        self.session.capture_timeout = .005
        self.session.capture_active = self.session.pty_eof = False
        self.session.rpc_count = self.session.rpc_response_bytes = 0
        self.session.capture_audit = []
        self.clock_patch = patch.object(performance.time, "monotonic_ns", lambda: self.clock.now)
        self.clock_patch.start()
        self.addCleanup(self.clock_patch.stop)
        self.rect = {"x": 0, "y": 0, "width": 8, "height": 1}

    @staticmethod
    def cells(text, clock="clock"):
        return tuple(tuple((char, ()) for char in row.ljust(8)[:8]) for row in (text, clock))

    def drain_frames(self, values):
        pending = iter(values)
        last = values[-1]

        def drain(timeout=0):
            nonlocal last
            self.clock.now += 1_000_000
            last = next(pending, last)
            self.session.frame_number += 1
            self.session.frames.append({"observed_ns": self.clock.now,
                "byte_offset": self.session.frame_number * 10, "frame_number": self.session.frame_number,
                "cells": self.cells(last, str(self.session.frame_number))})
        self.session.drain = drain

    def snapshot(self, text, clock="clock"):
        return {"number": 20, "page": "Launcher", "width": 8, "height": 2,
                "lines": [text, clock], "nodes": [{"id": "page/search", "role": "textbox",
                "actionable": True, "text": text, "rect": self.rect}]}

    def reply(self, snapshot):
        def control(action):
            self.assertFalse(self.session.capture_active)
            self.session.rpc_count += 1
            return snapshot
        self.session.control = control


class CaptureTests(CaptureFixture):
    def test_known_target_waits_across_reads_without_rpc_or_process_queries(self):
        self.drain_frames(["wait", "wait", "ready"])
        cue = performance.frame_cue(self.cells("ready", "different"), [self.rect])
        captured = self.session.capture_output(lambda: 0, cue)
        self.assertEqual(captured["rpc_count"], 0)
        self.assertEqual(captured["sealed_frame_number"], 3)
        self.assertFalse(self.session.capture_active)
        # No child/process object exists: capture must not inspect /proc or poll a PID.

    def test_footer_only_changes_cannot_complete_keyboard_focus_capture(self):
        def cells(style, footer):
            return (tuple((char, (style,)) for char in "search  "), self.cells("", footer)[1])
        candidates = iter([cells("focused", "update1"), cells("focused", "update2"), cells("unfocused", "update3")])

        def drain(timeout):
            self.clock.now += 1_000_000
            self.session.frame_number += 1
            self.session.frames.append({"observed_ns": self.clock.now, "frame_number": self.session.frame_number,
                                        "byte_offset": 10, "cells": next(candidates)})
        self.session.drain = drain
        capture = self.session.capture_output(lambda: 0,
            performance.frame_cue(cells("unfocused", "old"), [self.rect], True))
        self.assertEqual(capture["sealed_frame_number"], 3)

    def test_discovery_keeps_real_first_sample_and_ignores_continuous_clock_changes(self):
        self.drain_frames(["ready"])
        captured = self.session.capture_output(lambda: 0)
        self.assertEqual(len(captured["frames"]), 5)
        snapshot = self.snapshot("ready", "later")
        self.reply(snapshot)
        verified = self.session.verify_capture(captured, lambda frame: frame["page"] == "Launcher")
        timing = self.session.captured_timing(captured, verified)
        self.assertEqual(timing["latency_ms"], 1)
        self.assertEqual(timing["output_frame_number"], 1)
        self.assertEqual(self.session.rpc_count, 1)

    def test_rpc_during_capture_is_blocked_before_connect_and_retained_as_failure(self):
        def accidental_rpc(timeout):
            self.session.control({"command": "snapshot"})
        self.session.drain = accidental_rpc
        with patch.object(performance.socket, "socket") as socket:
            with self.assertRaisesRegex(AssertionError, "forbidden"):
                self.session.capture_output(lambda: 0)
            socket.assert_not_called()
        failures = list(map(json.loads,
                            (self.session.output / "failed-attempts.jsonl").read_text().splitlines()))
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0]["started_ns"], 0)
        self.assertEqual(failures[0]["rpc_count"], 0)
        self.assertFalse(failures[0]["completed"])

    def test_resource_probe_during_capture_is_blocked_before_proc_access(self):
        self.session.drain = lambda timeout: self.session.resources()
        with patch.object(performance.Path, "iterdir") as probe:
            with self.assertRaisesRegex(AssertionError, "probes are forbidden"):
                self.session.capture_output(lambda: 0)
            probe.assert_not_called()

    def test_timeout_is_retained_without_retry_or_replacing_slow_attempt(self):
        self.drain_frames(["wait"])
        raw = self.session.output / "test.pty"
        raw.write_bytes(b"preserved")
        cue = performance.frame_cue(self.cells("ready"), [self.rect])
        with self.assertRaisesRegex(AssertionError, "timeout"):
            self.session.capture_output(lambda: 0, cue, "slow-input")
        self.assertEqual(self.session.frame_number, 5)
        self.assertEqual(raw.read_bytes(), b"preserved")
        failures = (self.session.output / "failed-attempts.jsonl").read_text().splitlines()
        self.assertEqual(len(failures), 1)
        self.assertIn('"operation": "slow-input"', failures[0])

    def test_verification_cannot_supply_a_business_frame_after_the_seal(self):
        self.drain_frames(["old"])
        capture = self.session.capture_output(lambda: 0,
            performance.frame_cue(self.cells("old"), [self.rect]))
        snapshot = self.snapshot("new")

        def control(action):
            self.session.rpc_count += 1
            self.session.frames.append({"observed_ns": self.clock.now + 1,
                "byte_offset": 20, "frame_number": 2, "cells": self.cells("new")})
            return snapshot
        self.session.control = control
        verified = self.session.verify_capture(capture, lambda frame: True)
        with self.assertRaisesRegex(AssertionError, "not present before"):
            self.session.captured_timing(capture, verified)
        self.assertEqual(capture["sealed_frame_number"], 1)
        self.assertEqual(len(capture["frames"]), 1)
        self.assertEqual(len(self.session.frames), 2)


class NativeVimCueTests(CaptureFixture):
    @staticmethod
    def vim_cells(intro=""):
        rows = [""] + ["~"] * 6 + ["[Scratch] 0,0-1 All", ""]
        if intro:
            rows[3] = "~" + intro.center(59)
        return tuple(tuple((char, ()) for char in row.ljust(60)[:60]) for row in rows)

    def test_intro_variants_share_the_first_buffer_cue(self):
        rect = {"x": 0, "y": 0, "width": 60, "height": 9}
        variants = [self.vim_cells(text) for text in ("", "Help children in Uganda!", "Sponsor Vim development!")]
        for cells in variants:
            self.assertTrue(performance.vim_buffer_ready(cells, rect))
            self.assertFalse(performance.vim_buffer_ready(cells, rect, performance.region(cells, rect)))
        self.assertEqual(len({performance.signature(cells, performance.vim_ready_rects(rect)) for cells in variants}), 1)
        self.assertEqual(len({performance.region(cells, rect) for cells in variants}), 3)

    def test_partial_eob_or_command_prompt_is_not_a_ready_buffer(self):
        rect = {"x": 0, "y": 0, "width": 60, "height": 9}
        partial = list(self.vim_cells())
        partial[2] = tuple((" ", ()) for _ in range(60))
        self.assertFalse(performance.vim_buffer_ready(tuple(partial), rect))
        blocked = list(self.vim_cells())
        blocked[-1] = tuple((char, ()) for char in "Press ENTER to continue".ljust(60))
        self.assertFalse(performance.vim_buffer_ready(tuple(blocked), rect))

    def test_later_intro_change_cannot_delay_the_sealed_first_buffer(self):
        rect = {"x": 0, "y": 0, "width": 60, "height": 9}
        partial = list(self.vim_cells())
        partial[2] = tuple((" ", ()) for _ in range(60))
        candidates = iter([tuple(partial), self.vim_cells("Sponsor Vim development!")])

        def drain(timeout):
            self.clock.now += 1_000_000
            self.session.frame_number += 1
            self.session.frames.append({"observed_ns": self.clock.now, "frame_number": self.session.frame_number,
                                        "byte_offset": 10, "cells": next(candidates)})
        self.session.drain = drain
        capture = self.session.capture_output(lambda: 0,
            lambda candidate: performance.vim_buffer_ready(candidate["cells"], rect))
        snapshot = {"number": 30, "page": "Editor", "width": 60, "height": 9,
                    "lines": ["".join(cell[0] for cell in row) for row in self.vim_cells()],
                    "nodes": [{"id": "page/terminal", "role": "terminal", "text": "~", "actionable": True, "rect": rect}]}
        self.reply(snapshot)
        verified = self.session.verify_capture(capture, lambda frame: True)
        timing = self.session.captured_timing(capture, verified, performance.vim_ready_rects(rect))
        self.assertEqual(timing["output_frame_number"], 2)
        self.assertEqual(timing["latency_ms"], 2)

    def test_backtracking_cannot_choose_an_earlier_blocked_command_or_nonempty_first_line(self):
        rect = {"x": 0, "y": 0, "width": 60, "height": 9}
        nonempty = list(self.vim_cells())
        nonempty[0] = tuple((char, ()) for char in "loading".ljust(60))
        blocked = list(self.vim_cells())
        blocked[-1] = tuple((char, ()) for char in "Press ENTER to continue".ljust(60))
        candidates = iter([tuple(nonempty), tuple(blocked), self.vim_cells()])

        def drain(timeout):
            self.clock.now += 1_000_000
            self.session.frame_number += 1
            self.session.frames.append({"observed_ns": self.clock.now, "frame_number": self.session.frame_number,
                                        "byte_offset": 10, "cells": next(candidates)})
        self.session.drain = drain
        capture = self.session.capture_output(lambda: 0,
            lambda candidate: performance.vim_buffer_ready(candidate["cells"], rect))
        snapshot = {"number": 30, "page": "Editor", "width": 60, "height": 9,
                    "lines": ["".join(cell[0] for cell in row) for row in self.vim_cells()],
                    "nodes": [{"id": "page/terminal", "role": "terminal", "text": "~", "actionable": True, "rect": rect}]}
        self.reply(snapshot)
        verified = self.session.verify_capture(capture, lambda frame: True)
        timing = self.session.captured_timing(capture, verified, performance.vim_ready_rects(rect))
        self.assertEqual(timing["output_frame_number"], 3)
        self.assertEqual(timing["latency_ms"], 3)


class NewAppSourceTests(CaptureFixture):
    def test_activation_waits_for_different_page_closed_old_id_and_absent_old_pixels(self):
        target = {"id": "home.entry.launcher", "name": "Launcher", "role": "button",
                  "actionable": True, "rect": self.rect}

        def home(page="Home", old_window=False):
            return {"page": page, "nodes": [target] + ([{"id": "workspace.taskbar.1"}] if old_window else [])}

        self.session.business_templates = {("Launcher", None): (self.cells("ready"), [self.rect])}
        self.session.new_process_pids = {"1": [100]}
        self.session.snapshot = lambda: home()
        checked = []
        preparations = 0

        def prepare(predicate, timeout=20):
            nonlocal preparations
            preparations += 1
            if preparations == 1:
                self.assertTrue(predicate(home()))
                return home()
            for frame, text in ((home(old_window=True), "home"), (home(page="Launcher"), "home"),
                                (home(), "ready"), (home(), "home")):
                self.session.frame_number += 1
                self.session.frames = [{"cells": self.cells(text), "frame_number": self.session.frame_number,
                                        "observed_ns": -1, "byte_offset": 0}]
                checked.append(predicate(frame))
                if checked[-1]:
                    return frame
            self.fail("No valid pre-action source")
        self.session.until = prepare
        self.drain_frames(["ready"])
        activations = []

        def activate(frame, identifier):
            self.assertTrue(self.session.capture_active)
            self.assertEqual(checked, [False, False, False, True])
            activations.append(identifier)
            return 0
        self.session.tap = activate
        points = iter([{"processes": [{"pid": 1}]}, {"processes": [{"pid": 1}, {"pid": 2}]}])

        def resources():
            self.assertFalse(self.session.capture_active)
            return next(points)
        self.session.resources = resources
        verified = self.snapshot("ready")
        verified["nodes"].append({"id": "workspace.title.2", "selected": True})
        self.reply(verified)
        _, timing = self.session.open_app("Launcher")
        self.assertEqual(activations, ["home.entry.launcher"])
        self.assertEqual(timing["activation_context"]["page_before_action"], "Home")
        self.assertFalse(timing["activation_context"]["known_business_signature_visible_before_action"])
        self.assertEqual(timing["activation_context"]["previous_measured_window_ids_absent"], ["1"])
        self.assertEqual(timing["process_identity"]["new_process_pids"], [2])


class MouseFocusPreparationTests(unittest.TestCase):
    def test_keyboard_padding_style_is_not_cached_as_mouse_focus_after_clear(self):
        rect = {"x": 0, "y": 0, "width": 16, "height": 1}

        def cells(focused, keyboard):
            text = ("Search: " + ("_" if focused else "")).ljust(16)
            return (tuple((char, ("accent" if focused and keyboard and index > 8 else "text",))
                          for index, char in enumerate(text)),)

        class Fixture:
            def __init__(self):
                self.focused = self.keyboard = False
                self.frames = [{"cells": cells(False, False)}]
                self.pending = []
                self.actions = []
                self.stale_text_rejections = 0

            def until(self, predicate):
                for _ in range(10):
                    if self.pending:
                        self.frames.append({"cells": self.pending.pop(0)})
                    text = "Search: " + ("_" if self.focused else "")
                    frame = {"width": 16, "height": 1, "lines": [text], "nodes": [{
                        "id": "page/launcher.search", "role": "textbox", "actionable": True,
                        "focused": self.focused, "text": text, "rect": rect}]}
                    if predicate(frame):
                        return frame
                    self.stale_text_rejections += 1
                raise AssertionError("Preparation never aligned its snapshot and VT state")

            def tap(self, frame, identifier):
                self.actions.append("mouse")
                previous = self.frames[-1]["cells"]
                self.focused, self.keyboard = True, False
                self.pending = [previous, cells(True, False)]

            def send(self, value):
                self.actions.append(value)
                previous = self.frames[-1]["cells"]
                self.keyboard = True
                if value == b"\t":
                    self.focused = False
                self.pending = [previous, cells(self.focused, True)]

        session = Fixture()
        with patch.object(performance.time, "sleep") as sleep:
            current = performance.prepare_mouse_search(session)
            sleep.assert_not_called()
        prepared = session.frames[-1]
        mouse = cells(True, False)
        keyboard = cells(True, True)
        self.assertEqual(performance.region(mouse, rect), performance.region(keyboard, rect))
        self.assertNotEqual(performance.region(mouse, rect, True), performance.region(keyboard, rect, True))
        self.assertEqual(prepared["cells"], mouse)
        self.assertTrue(performance.frame_cue(mouse, [rect], True)(prepared))
        self.assertFalse(performance.frame_cue(keyboard, [rect], True)(prepared))
        self.assertTrue(current["nodes"][0]["focused"])
        self.assertEqual(session.actions, ["mouse", b"\x01\x7f", b"\t", "mouse"])
        self.assertGreaterEqual(session.stale_text_rejections, 2)


class ReportConditionTests(unittest.TestCase):
    def test_incompatible_comparisons_are_rejected_before_any_process_starts(self):
        with tempfile.TemporaryDirectory(prefix="tundra-report-condition-") as folder:
            root = Path(folder)
            binary = root / "tundra-shell"
            binary.touch()
            (root / "tundra-cli").touch()
            baseline = root / "baseline.json"
            compatible = {"trace_enabled": False, "summaries": {}, "acceptance_eligible": True,
                          "observation_condition": "zero-rpc-sealed-pty-capture-v1",
                          "native_vim_endpoint": performance.NATIVE_VIM_ENDPOINT,
                          "input_focus_endpoint": performance.FOCUS_ENDPOINT}
            cases = [("observation_condition", "polling-active", "observation conditions"),
                     ("trace_enabled", True, "detailed-trace settings"),
                     ("native_vim_endpoint", "full-terminal-sha", "native Vim ready definitions"),
                     ("input_focus_endpoint", "whole-page", "keyboard focus definitions"),
                     ("acceptance_eligible", False, "Diagnostic compiler profiles")]
            for index, (field, value, error) in enumerate(cases):
                with self.subTest(field=field):
                    baseline.write_text(json.dumps(dict(compatible, **{field: value})))
                    output = root / f"output-{index}"
                    argv = ["workspace-performance.py", str(binary), "--output", str(output), "--compare", str(baseline)]
                    with patch.object(performance.sys, "argv", argv), \
                            patch.object(performance.platform, "platform", return_value="test"), \
                            patch.object(performance.subprocess, "Popen") as process:
                        with self.assertRaisesRegex(AssertionError, error):
                            performance.main()
                        process.assert_not_called()
                    self.assertFalse((output / "samples.jsonl").exists())

    def test_completed_small_run_reports_condition_trace_and_insufficient_samples(self):
        with tempfile.TemporaryDirectory(prefix="tundra-report-condition-") as folder:
            root = Path(folder)
            binary = root / "tundra-shell"
            binary.touch()
            (root / "tundra-cli").touch()
            output = root / "output"
            argv = ["workspace-performance.py", str(binary), "--output", str(output),
                    "--phase", "launch", "--application", "Launcher", "--launch-samples", "1"]

            def launch(*args, **kwargs):
                self.assertFalse(kwargs["detailed_trace"])
                args[4]({"kind": "launch", "application": "Launcher",
                         "mode": "first_open_in_shell", "latency_ms": 1})

            read_text = Path.read_text

            def fixture_text(path, *args, **kwargs):
                if path == Path("/proc/loadavg"):
                    return "0.00 0.00 0.00 1/1 1"
                return read_text(path, *args, **kwargs)

            with patch.object(performance.sys, "argv", argv), \
                    patch.object(performance.sys, "stdout", io.StringIO()), \
                    patch.object(performance.platform, "platform", return_value="test"), \
                    patch.object(Path, "read_text", fixture_text), \
                    patch.object(performance.subprocess, "run", return_value=SimpleNamespace(stdout="")), \
                    patch.object(performance.subprocess, "Popen") as process, \
                    patch.object(performance, "launches", side_effect=launch):
                performance.main()
                process.assert_not_called()
            report = json.loads((output / "report.json").read_text())
            self.assertTrue(report["completed"])
            self.assertFalse(report["trace_enabled"])
            self.assertEqual(report["observation_condition"], "zero-rpc-sealed-pty-capture-v1")
            self.assertFalse(report["acceptance"]["launch/Launcher/all_new_processes"]["enough_samples"])
            self.assertEqual(report["failed_attempts"], [])


if __name__ == "__main__":
    unittest.main()
