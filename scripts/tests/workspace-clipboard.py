#!/usr/bin/env python3
"""Verify Copy/Cut completion before an immediate paste in another real AppHost."""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "workspace_performance", Path(__file__).with_name("workspace-performance.py")
)
performance = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(performance)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    session = performance.Session(args.binary.resolve(), args.output, "clipboard", 217, 39)

    def control(command, **values):
        frame = session.control(dict(command=command, **values))
        assert frame is not None, f"Control command was not ready: {command}"
        return frame

    def click(identifier):
        return control("click", target={"id": identifier})

    def key(value, modifiers=()):
        return control("key", key=value, modifiers=list(modifiers))

    def navigate(path):
        key("l", ("ctrl",))
        control("text", text=str(path))
        key("enter")

    def select_file(name):
        frame = session.until(lambda frame: any(
            item["role"] == "row" and item["id"].startswith("page/")
            and name in item["text"] for item in frame["nodes"]
        ))
        row = next(item for item in frame["nodes"] if item["role"] == "row"
                   and item["id"].startswith("page/") and name in item["text"])
        return click(row["id"])

    def text(frame):
        return "\n".join(frame["lines"])

    results = []
    try:
        home = Path(session.env["HOME"])
        source, target = home / "clipboard-source", home / "clipboard-target"
        source.mkdir()
        target.mkdir()
        cases = (("copy-control", "c", False), ("cut-control", "x", False),
                 ("copy-visible", "c", True))
        for label, _, _ in cases:
            (source / f"{label}-源.txt").write_text(f"{label}: e\u0301 中文\n")
        frame, _ = session.open_app("Explorer")
        first = next(item for item in performance.titles(frame) if item["selected"])["id"]
        session.snapshot()
        navigate(source)
        session.send(b"\x17v")
        performance.choose(session, "Explorer", "application.explorer")
        frame = session.snapshot()
        second = next(item for item in performance.titles(frame) if item["selected"])["id"]
        assert first != second, "The check requires independent Explorer AppHosts"
        navigate(target)

        for label, command, physical in cases:
            name = f"{label}-源.txt"
            expected = (source / name).read_bytes()
            click(first)
            select_file(name)
            if physical:
                session.send(b"\x03")
                frame = session.until(lambda frame: "Copied selection." in text(frame))
            else:
                frame = key(command, ("ctrl",))
                assert ("Copied selection." if command == "c" else "Cut selection.") in text(frame), frame
            completed = time.monotonic_ns()
            click(second)
            paste_started = time.monotonic_ns()
            key("v", ("ctrl",))
            session.until(lambda _: (target / name).is_file()
                          and (target / name).read_bytes() == expected
                          and (command != "x" or not (source / name).exists()))
            assert command == "x" or (source / name).read_bytes() == expected
            results.append({"case": label, "source": name,
                            "completion_to_paste_ms": (paste_started - completed) / 1_000_000})
            print(f"PASS: {label}: completed Copy/Cut then immediate cross-window Paste", flush=True)
        (args.output / "clipboard-results.json").write_text(
            json.dumps(results, ensure_ascii=False, indent=2) + "\n"
        )
    finally:
        session.close()


if __name__ == "__main__":
    main()
