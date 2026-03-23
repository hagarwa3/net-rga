#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_PATH="${1:-$ROOT_DIR/benchmarks/results/tier0_local_fs.json}"

cd "$ROOT_DIR"

python3 benchmarks/materialize_tier0_corpus.py
python3 benchmarks/harness.py run --output "$OUTPUT_PATH"

