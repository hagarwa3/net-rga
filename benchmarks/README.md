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
  README.md
  schemas/
    benchmark_case.schema.json
```

## Case schema design rules

- A case describes one logical search workload.
- A case does not embed judged answers directly; it points at a judgment file and case id.
- Run mode lives in the case because latency and cost are only meaningful if provider/cache/backend mode is explicit.
- Cases are versioned so the harness can evolve without making old benchmark data ambiguous.

## Planned next schemas

- `judgment.schema.json`
- `benchmark_result.schema.json`

