#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BACKEND_MODE="${1:-disabled}"
OUTPUT_PATH="${2:-$ROOT_DIR/benchmarks/results/tier0_local_fs_${BACKEND_MODE}.json}"

cd "$ROOT_DIR"

python3 benchmarks/materialize_tier0_corpus.py
python3 benchmarks/harness.py run --backend-mode "$BACKEND_MODE" --output "$OUTPUT_PATH"
