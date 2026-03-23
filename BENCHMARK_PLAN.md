# Net-RGA Benchmark Plan

Status: Draft

This document defines the benchmark strategy for `net-rga`. The goal is to make performance, retrieval quality, operational correctness, and downstream usefulness measurable from the start so new features do not silently make search slower, more expensive, or less trustworthy.

Benchmarking is a `P0` workstream for this project, not a later optimization task. `net-rga` is building a search system over local filesystems, object stores, and document-heavy corpora, so correctness and speed regressions will be easy to introduce and hard to notice unless they are measured continuously.

## Why Net-RGA Needs Its Own Benchmark Suite

There are useful public benchmark families for retrieval, long-context reasoning, document QA, and ANN/vector search, but none directly measure the combined behavior that `net-rga` cares about:

- provider-backed search rather than purely local corpora
- mixed-format document extraction
- grep-like exact lexical workflows
- manifest-based planning and partial local state
- freshness drift, permission failures, and partial coverage
- anchor/snippet quality rather than only document-level relevance

The right strategy is to create a native `net-rga` benchmark suite and selectively borrow ideas, metrics, and datasets from public work such as `BEIR`, `LongBench`, `RULER`, `InfiniteBench`, `DocVQA/DocCVQA`, `M-BEIR`, and `ANN-Benchmarks`.

## Benchmark Objectives

The benchmark suite must answer these questions after every meaningful change:

- Did query latency improve or regress?
- Did the system fetch fewer bytes or make fewer remote calls for the same task?
- Did retrieval quality improve, stay flat, or regress?
- Did anchor/snippet quality improve or regress?
- Did extraction changes alter search correctness on PDFs, Office docs, or compressed text?
- Did coverage reporting remain truthful under stale state, deletion, or permission loss?
- Did warm-cache and imported-bundle flows improve?
- Did one provider improve while another regressed?
- Did improvements in search quality translate into better downstream agent usefulness?

## Requirements For A Good Benchmark

A good `net-rga` benchmark must be:

- Provider-realistic: measure local filesystem and remote provider behavior separately and together.
- Mixed-format: include plain text, compressed text, PDFs, presentations, spreadsheets, documents, archives, binaries, and unsupported media.
- Ground-truthed: every query must have judged expected results.
- Anchor-aware: evaluate page, slide, sheet, cell, chunk, or line-like location quality, not only document-level hits.
- Cost-aware: treat remote ops, bytes fetched, extraction work, and cache utilization as first-class metrics.
- Freshness-aware: simulate deletions, updates, permission changes, and stale manifests.
- Reproducible: fixed corpora, fixed judgments, explicit warm/cold run modes, fixed benchmark seeds.
- Tiered: small enough for CI, large enough for realistic regression detection, and extensible to slower nightly runs.
- Explainable: failures should be attributable to planner logic, extraction behavior, provider latency, backend/index logic, or output formatting.
- Agent-relevant: track whether retrieved results actually improve downstream context gathering and answering behavior.

## Non-Goals

This benchmark suite is not intended to:

- rank foundation models in the abstract
- replace public retrieval or document-understanding benchmarks
- optimize solely for synthetic “needle in haystack” performance
- measure only index throughput while ignoring end-to-end search behavior
- collapse exact lexical search and semantic retrieval into one blended score

## Benchmark Score Families

The benchmark suite should report four score families.

### 1. Latency

- `p50` and `p95` time-to-first-candidate
- `p50` and `p95` time-to-first-verified-match
- `p50` and `p95` time-to-top-k-verified-matches
- total query latency
- sync latency
- import/bootstrap latency from bundle

### 2. Cost

- bytes fetched from provider
- number of `list` calls
- number of `stat` calls
- number of `read` calls
- extracted bytes processed
- cache hit rate
- manifest hit ratio for pruning
- local CPU time spent in extraction
- local storage consumed by manifest, cache, and index artifacts
- optional backend build/update time

### 3. Retrieval Quality

- Recall@k
- MRR
- nDCG
- exact lexical correctness
- false positive rate
- anchor accuracy
- snippet usefulness score

### 4. Operational Correctness

- complete vs partial coverage correctness
- stale/deleted/denied handling accuracy
- import/export fidelity
- deterministic output stability
- provider parity for equivalent corpora

## Benchmark Tiers

The suite should be organized into four tiers so it can run at different cadences.

### Tier 0: Micro

Purpose:

- run in CI on every change
- catch correctness regressions quickly

Properties:

- very small corpus
- deterministic
- no external cloud dependency

Measures:

- exact match correctness
- anchor rendering correctness
- planner pruning behavior
- JSON output shape stability
- coverage reporting correctness on small scenarios

### Tier 1: Mixed-Small

Purpose:

- run regularly in CI or pre-merge
- validate realistic mixed-format search behavior

Properties:

- `1k` to `10k` documents
- local filesystem and S3-compatible variants
- includes common business-document types

Measures:

- latency under warm and cold cache modes
- bytes fetched
- extraction correctness
- retrieval quality for judged queries
- output stability across providers

### Tier 2: Mixed-Medium

Purpose:

- run nightly or on release branches
- catch planner, cache, and provider regressions that do not show up on tiny corpora

Properties:

- tens of thousands of objects
- skewed file sizes
- duplicate and near-duplicate documents
- nested prefixes and noisy corpora

Measures:

- planner quality
- time-to-first-verified-match
- remote-op efficiency
- optional index usefulness
- bundle bootstrap usefulness

### Tier 3: Freshness-Chaos

Purpose:

- validate truthfulness and resilience under drift

Properties:

- scripted mutations between sync and query
- deletions, updates, RBAC failures, transient read failures

Measures:

- partial coverage honesty
- stale-state detection
- failure accounting
- behavior consistency after re-sync

## Corpus Design

The suite should include both synthetic and semi-realistic corpora.

### Corpus classes

- `text`: source-like text, notes, markdown, logs
- `compressed`: gzip and zip-wrapped text
- `pdf`: OCR-free text PDFs and layout-heavy PDFs
- `office-doc`: `.docx`, `.pptx`, `.xlsx`
- `structured`: CSV, TSV, JSON
- `binary`: unsupported blobs
- `media`: images and videos that are metadata-only in v0

### Corpus properties to vary

- number of documents
- average file size
- long-tail size distribution
- nesting depth and prefix layout
- metadata richness
- duplication and near-duplication
- language and encoding variation
- number of supported vs unsupported files

### Data sources

Use a mix of:

- handcrafted synthetic documents for precise judgments
- transformed public corpora for realistic scale
- benchmark-inspired long documents and document collections

The suite should avoid requiring large proprietary datasets for baseline execution.

## Benchmark Data Management

Benchmark metadata should stay in git:

- schemas
- benchmark cases
- judgments
- mutation definitions
- corpus generators and manifests

Benchmark search corpora and result artifacts should not live in git by default:

- generated corpora go under an ignored local data directory
- benchmark result files go under an ignored local results directory
- downloaded or transformed source corpora go under an ignored local cache directory

Long-term management model:

- tiny deterministic corpora are generated locally from tracked scripts or manifests
- medium and large corpora are versioned by manifest id and stored outside git
- every tracked corpus definition should record provenance, license constraints, generator version, and expected checksums
- every benchmark report records the corpus version or manifest identifier it used
- if shared benchmark data becomes necessary, use an external artifact store rather than bloating the source repo

### Benchmark data lifecycle

The benchmark suite should treat benchmark data like build artifacts, not source code.

Required long-term workflow:

1. Track only benchmark definitions in git.
   This includes schemas, cases, judgments, mutation specs, generator scripts, and corpus manifests.
2. Materialize corpora into ignored local directories.
   Generated corpora belong under `benchmarks/data/`. Downloaded or transformed source corpora belong under `benchmarks/cache/`.
3. Identify every corpus build with a stable corpus or manifest id.
   A manifest id should be enough to recover the exact generator inputs, public source references, and expected file checksums.
4. Emit corpus identity in every benchmark result.
   A result file is incomplete if it does not say which corpus build, provider mode, cache mode, and git revision produced it.
5. Share larger corpora through an external artifact store when needed.
   The repository should keep the manifest, provenance, and checksums, but not the raw benchmark blobs.

### Benchmark data guardrails

- Do not commit raw benchmark corpora or benchmark result files.
- Prefer synthetic data and transformed public datasets over proprietary document collections.
- Keep provenance and license notes with each tracked corpus manifest or generator definition.
- Avoid opaque one-off datasets that cannot be recreated or checksum-verified.
- Preserve deterministic seeds for synthetic corpus generation whenever practical.
- Make it possible to rebuild tiny CI corpora locally without network access.
- Assume medium and large corpora may need retention, pruning, and mirroring policies outside the source repo.

## Query Taxonomy

The benchmark must evaluate more than “find this keyword.”

### Lexical query types

- exact token lookup
- multi-token phrase lookup
- regex match
- path-filtered search
- file-type-constrained search
- metadata-constrained search
- top-k early-stop search

### Retrieval difficulty types

- single obvious target
- many near-miss documents
- duplicated content in many locations
- relevant result only in extracted document text
- relevant result only in a narrow anchor such as a page or cell range
- multiple acceptable answers with ranked relevance

### Operational query types

- query immediately after sync
- query on stale manifest
- query after permissions changed
- query with and without optional backend/index
- query after bundle import in a fresh environment

## Anchor And Snippet Evaluation

Because `net-rga` uses anchor-native internals, benchmark judgments must go beyond document IDs.

Each judged result should be able to express:

- target document id
- acceptable anchor set
- preferred anchor
- expected snippet terms or phrase window
- tolerance for line-like vs page-like rendering

Anchor accuracy should be measured at multiple granularities:

- exact anchor match
- same document, wrong anchor
- nearby anchor within tolerance
- missing anchor

Snippet usefulness should be judged based on whether the snippet is sufficient to identify why the result is relevant without opening the full document.

## Freshness And Failure Scenarios

Freshness is part of correctness, not a side concern.

Required mutation scenarios:

- object deleted after sync
- object updated after sync with changed content
- object renamed or moved
- permissions revoked after sync
- transient provider read failure
- partial sync that leaves some prefixes stale
- imported bundle with missing optional artifacts

Required assertions:

- verified matches remain truthful
- stale candidates are not silently treated as complete coverage
- partial coverage is surfaced consistently
- counters for deleted, denied, stale, unsupported, and failed items are accurate

## Provider Matrix

The benchmark suite should run the same logical workloads against multiple provider modes.

### Required v0 modes

- local filesystem
- S3-compatible object store

### Required run modes

- cold manifest, cold cache
- warm manifest, cold content cache
- warm manifest, warm cache
- imported bundle bootstrap
- no optional backend/index
- optional backend/index enabled

This matrix is critical because some changes will improve warm performance while harming cold-start behavior, or help local filesystems while hurting remote providers.

## Golden Scenario Catalog

The first benchmark release should include a small catalog of named scenarios.

### 1. Needle In Many PDFs

One fact exists in one of many visually similar PDFs.

Measures:

- retrieval precision
- PDF extraction quality
- anchor accuracy at page/span level
- time-to-first-verified-hit

### 2. Same Title, Different Document

Many documents share similar titles or filenames, but only one contains the target content.

Measures:

- metadata-vs-content disambiguation
- false positive control

### 3. Spreadsheet Anchor

The answer exists in a specific sheet and cell range.

Measures:

- spreadsheet extraction
- anchor fidelity
- snippet usefulness

### 4. Regex Over Mixed Text

A regex matches across a mix of plain text and extracted document text.

Measures:

- lexical correctness
- planner efficiency without assuming a backend can do regex well

### 5. Cheap First Hit

Several valid hits exist, but one is much cheaper to verify.

Measures:

- candidate ranking
- time-to-first-verified-match
- bytes fetched

### 6. Stale Manifest Truthfulness

Manifest says an item exists, but it has been deleted or permissions changed.

Measures:

- partial coverage honesty
- stale/deleted/denied accounting

### 7. Bundle Bootstrap

A fresh environment imports a bundle and immediately serves queries.

Measures:

- startup latency
- bundle fidelity
- warm-start productivity

### 8. Local/Remote Parity

Equivalent corpus is run once from local filesystem and once from S3-compatible storage.

Measures:

- retrieval parity
- provider-induced latency differences
- remote-op cost

### 9. Unsupported Media Noise

A corpus contains many images/videos/binaries near relevant documents.

Measures:

- planner selectivity
- unsupported-file handling
- coverage accounting

### 10. Agent Context Utility

Search results are used to answer a grounded question or perform a task.

Measures:

- whether the returned top-k snippets are actually useful
- whether search improvements translate into better downstream task outcomes

## Judgment Format

The benchmark harness should use explicit, versioned judgments.

Each query should include:

- stable query id
- corpus id
- query text
- query mode: fixed string, regex, path-filtered, metadata-filtered
- optional provider constraints
- optional run-mode constraints

Each judgment should include:

- relevant document ids
- relevance grade per document
- acceptable anchors
- preferred anchors
- snippet expectations
- whether complete coverage is required for the scenario

This format must support both strict exact-match checks and graded retrieval metrics.

## Harness Outputs

Every benchmark run should emit machine-readable artifacts.

Required outputs:

- benchmark manifest with corpus version, code revision, provider mode, and run mode
- query-level metrics
- aggregate metrics
- resource/cost counters
- coverage correctness counters
- failures and unsupported-case summaries

Recommended formats:

- JSONL for per-query results
- JSON summary for aggregate metrics
- Markdown report for humans

## Regression Gates

The benchmark suite should define explicit regression policy rather than only generating dashboards.

### Hard fail conditions

- exact lexical correctness regression on golden micro scenarios
- output schema breakage
- anchor rendering regression beyond allowed tolerance
- coverage truthfulness regression

### Soft fail or warning conditions

- `p95` latency regression beyond threshold
- bytes fetched regression beyond threshold
- Recall@k or MRR drop beyond threshold
- cache hit rate regression
- import/bootstrap regression

Thresholds should be tier-specific. CI thresholds should be conservative enough to avoid flakiness; nightly thresholds can be tighter and more comprehensive.

## Benchmark Harness Architecture

The harness should be built as a first-party project asset, not an ad hoc script pile.

Core components:

- corpus builder
- provider loader
- mutation runner for freshness scenarios
- query runner
- judge/evaluator
- metrics aggregator
- report generator

The harness should support:

- deterministic seeded corpus generation
- replayable provider mutations
- local and S3-compatible execution
- warm/cold cache orchestration
- optional backend toggles

## CI And Release Cadence

### Per-PR or fast CI

Run:

- Tier 0
- selected Tier 1 smoke scenarios

Purpose:

- fast regression detection
- output/schema stability

### Nightly

Run:

- full Tier 1
- selected Tier 2
- Tier 3 freshness scenarios

Purpose:

- planner and extraction regression detection
- cost/latency trend monitoring

### Release or milestone validation

Run:

- all tiers
- local and S3 provider matrix
- bundle import/export scenarios
- optional backend-on/backend-off comparisons

Purpose:

- release confidence
- performance frontier tracking

## Public Benchmark Inputs To Reuse

The suite should selectively borrow from public work rather than trying to recreate everything from scratch.

Recommended inspirations:

- `BEIR` for heterogeneous retrieval evaluation
- `LongBench` and `InfiniteBench` for long-context downstream utility
- `RULER` for synthetic stress and regression framing
- `DocVQA` and `DocCVQA` for document-centric tasks and anchor fidelity
- `ANN-Benchmarks` for future index/backend frontier reporting

These should be treated as reference inputs or adapted workloads, not as the benchmark in full.

## Deliverables

The benchmarking workstream should produce:

1. This spec document
2. A versioned benchmark corpus format
3. A versioned query and judgment schema
4. A benchmark harness with machine-readable outputs
5. CI and nightly benchmark jobs
6. Baseline result snapshots stored with code revisions

## Immediate Next Actions

1. Add a `Benchmarking` section to the main execution plan so this workstream is visible in `P0`.
2. Define the query and judgment schema before building corpora.
3. Build a tiny golden corpus for Tier 0 with exact anchor judgments.
4. Build a mixed-format small corpus for Tier 1 across local FS and S3-compatible storage.
5. Implement a minimal harness that records latency, cost, correctness, and coverage metrics per query.
6. Add the first five golden scenarios before any major search implementation work begins.

## Definition Of Done

The benchmark plan is actionable when:

- a new engineer can build benchmark corpora and judgments without further design work
- the harness can compare two revisions of `net-rga` on latency, cost, quality, and correctness
- the suite can fail CI for clear correctness regressions
- nightly runs can reveal slower or more expensive query paths even when correctness stays flat
- benchmark outputs are stable enough to become a long-term project baseline
