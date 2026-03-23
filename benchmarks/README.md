# Net-RGA Benchmarks

This directory holds the benchmark scaffold for `net-rga`.

The scaffold is intentionally split into separate schema families:

- benchmark case schema: defines the query to run, the run mode, and where to find expected judgments
- judgment schema: defines relevant documents, anchors, snippets, and coverage expectations
- result schema: defines machine-readable outputs emitted by the harness

This separation keeps benchmark inputs reusable. The same query case should be able to run:

- against multiple provider modes
- against multiple corpus builds
- with warm or cold manifest/cache states
- with or without an optional backend

## Current layout

```text
benchmarks/
  harness.py
  materialize_tier0_corpus.py
  README.md
  schemas/
    benchmark_case.schema.json
    benchmark_result.schema.json
    judgment.schema.json
```

## Case schema design rules

- A case describes one logical search workload.
- A case does not embed judged answers directly; it points at a judgment file and case id.
- Run mode lives in the case because latency and cost are only meaningful if provider/cache/backend mode is explicit.
- Cases are versioned so the harness can evolve without making old benchmark data ambiguous.

## Benchmark data management

Benchmark metadata stays in git:

- schemas
- cases
- judgments
- mutation definitions
- corpus generators and manifests

Generated corpus data and benchmark result artifacts do not stay in git:

- `benchmarks/data/`
- `benchmarks/results/`

This keeps the repo small while still preserving reproducibility.

Long term, the intended model is:

- tiny deterministic corpora can be materialized locally from tracked generators
- larger corpora should be versioned by manifest and stored outside git
- benchmark runs should record the corpus version or manifest id they used

## Planned next schemas

- tiny golden corpus inputs
- first benchmark cases and judgments

## Minimal harness

The first harness is intentionally small and dependency-free.

Current commands:

```bash
python3 benchmarks/materialize_tier0_corpus.py
python3 benchmarks/harness.py run
python3 benchmarks/harness.py compare path/to/before.json path/to/after.json
./benchmarks/run_tier0.sh
```

The initial harness is only expected to support the Tier 0 local-filesystem golden corpus. It exists to give the project stable machine-readable baseline results before the main Rust implementation lands.

The same scaffold is wired into a lightweight GitHub Actions workflow so the Tier 0 benchmark can run in fast CI before the main engine exists.
