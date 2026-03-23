#!/usr/bin/env python3

from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parent
TARGET = ROOT / "data" / "tier0" / "local_fs"

FILES = {
    "README.md": """# Tier 0 Local Filesystem Corpus

This is the tiny deterministic golden corpus for the benchmark scaffold.

It is generated locally and intentionally kept out of git. The files are chosen so the first benchmark scenarios can cover:

- exact match lookup
- path-filtered lookup
- nested directory traversal
- unsupported file handling
- later permission and stale-state mutations
""",
    "docs/exact/riverglass.txt": """Project Riverglass launch packet

The unique verification token for this corpus is riverglass.
Only this file contains the exact token riverglass.
Agents should be able to find it with a simple literal search.
""",
    "docs/reports/q1-summary.md": """# Q1 Summary

Project Orchid launched on schedule.
The launch checklist was completed by the field operations group.
Revenue attribution remains provisional pending finance review.
""",
    "docs/logs/app.log": """2026-03-22T10:00:00Z INFO startup complete
2026-03-22T10:01:00Z INFO ingestion succeeded for tenant alpha
2026-03-22T10:02:00Z WARN retrying stale manifest refresh
2026-03-22T10:03:00Z ERROR unable to open invoice-7781.pdf due to denied
""",
    "docs/notes/archive/todo.txt": """TODO

- rename the Orion checklist after legal review
- archive the 2025 pilot notes
- confirm path-filter benchmark coverage
""",
    "docs/spreadsheets/budget.csv": """department,quarter,budget
research,Q1,120000
sales,Q1,90000
operations,Q1,110000
""",
    "docs/private/secret.txt": """This file is part of the Tier 0 corpus.
Later freshness and permission scenarios may revoke access to this path.
The private marker token is nebula-flag.
""",
    "media/demo.mp4": """FAKE_MP4_BINARY_PLACEHOLDER
This file exists to exercise unsupported media handling in benchmark scenarios.
""",
}


def main() -> int:
    for relative_path, content in FILES.items():
        destination = TARGET / relative_path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(content, encoding="utf-8")
    print(f"materialized Tier 0 corpus at {TARGET}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

