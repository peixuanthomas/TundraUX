"""Check that PTY timing accepts complete visible frames and preserves VT state."""
import importlib.util
from pathlib import Path
import unittest

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


if __name__ == "__main__":
    unittest.main()
