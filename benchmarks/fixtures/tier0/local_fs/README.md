# Tier 0 Local Filesystem Corpus

This is the tiny deterministic golden corpus for the benchmark scaffold.

It is intentionally small, text-first, and human-auditable. The files are chosen so the first benchmark scenarios can cover:

- exact match lookup
- path-filtered lookup
- nested directory traversal
- unsupported file handling
- later permission and stale-state mutations

The corpus should remain stable unless a benchmark revision explicitly changes it.

