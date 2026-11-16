"""Check complete PTY frame timing, VT state, and optional trace isolation."""
import importlib.util
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

    def test_unicode_and_cursor_diffs_survive_chunk_boundaries(self):
        screen = performance.Screen(8, 2)
        data = "\x1b[1;1H中Ω\x1b[2;3Hok\x1b[?25l".encode()
        frames = []
        for byte in data:
            frames.extend(screen.feed(bytes((byte,))))
        self.assertEqual(len(frames), 1)
        self.assertEqual("".join(cell[0] for cell in frames[0][0]), "中 Ω     ")
        self.assertEqual("".join(cell[0] for cell in frames[0][1]), "  ok    ")

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


class SessionTraceTests(unittest.TestCase):
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
                    session.temporary.cleanup()
            return captured, str(output / "test.trace.jsonl")

    def test_default_session_removes_inherited_detailed_trace(self):
        environment, _ = self.session_environment()
        self.assertIsNone(environment.get("TUNDRA_PERF_TRACE"))

    def test_detailed_trace_explicitly_uses_this_runs_output(self):
        environment, expected = self.session_environment(True)
        self.assertEqual(environment.get("TUNDRA_PERF_TRACE"), expected)


if __name__ == "__main__":
    unittest.main()
