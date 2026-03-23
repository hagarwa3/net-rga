# net-rga

`net-rga` is a provider-agnostic document search CLI with grep-like affordances.

The current v0/MVP focuses on:

- local filesystem corpora
- S3 and S3-compatible object storage corpora
- manifest-first sync into local state
- exact lexical search with final provider-side verification
- extraction for plain text, gzip text, PDF, DOCX, PPTX, and XLSX
- optional local lexical indexing as a planning accelerator
- portable bundle export/import for synced corpora

## Why it exists

`rg` works because local files are cheap to traverse and read on demand. Remote object stores and document-heavy corpora break those assumptions:

- listing is remote and latent
- fetching content is expensive
- PDFs and Office docs need extraction before they become searchable
- freshness drift and permission changes matter

`net-rga` keeps the CLI grep-like, but uses a local manifest, optional local index, and on-demand remote verification to make remote and mixed-format search practical.

## Current architecture

At a high level:

1. `corpus add` stores a corpus definition in local config.
2. `sync` materializes remote or local metadata into a local SQLite manifest.
3. `search` filters and ranks manifest candidates locally, optionally consults the local lexical sidecar, then fetches and verifies provider content.
4. extracted content is normalized into canonical chunks with anchors
5. results are rendered in grep-like text output or JSON

Local state lives under `~/.net-rga` by default, or under `NET_RGA_STATE_ROOT` if set.

## Build

```bash
cargo build
```

Useful validation commands:

```bash
cargo xcheck
cargo xtest
cargo xclippy
```

## Quickstart

Add a local filesystem corpus:

```bash
net-rga corpus add local --provider local-fs --root /path/to/corpus
net-rga sync local
net-rga search riverglass local
net-rga inspect local
```

Add an S3 corpus:

```bash
net-rga corpus add logs \
  --provider s3 \
  --bucket my-bucket \
  --prefix docs/ \
  --region us-east-1

net-rga sync logs
net-rga search riverglass logs --fixed-strings
```

Export and import a synced corpus bundle:

```bash
net-rga export local /tmp/local.bundle
net-rga import /tmp/local.bundle
```

Bundles are currently directory-based and include:

- `bundle.json`
- `corpus.toml`
- `manifest.db`
- optional `index/`
- optional `cache/`

## Commands

### `corpus`

Manage configured corpora:

```bash
net-rga corpus add ...
net-rga corpus list
net-rga corpus remove <name>
```

### `sync`

Refresh the manifest for a corpus:

```bash
net-rga sync <corpus>
```

Sync records:

- listed document count
- inserted and updated documents
- tombstoned deletions
- denied objects
- provider failures
- sync checkpoints

### `search`

Search a corpus with grep-like flags:

```bash
net-rga search <pattern> <corpus> \
  --glob 'docs/**' \
  --type pdf \
  --content-type application/pdf \
  --size-min 1024 \
  --size-max 5000000 \
  --modified-after 1000 \
  --modified-before 2000 \
  --max-count 10 \
  --fixed-strings \
  --json
```

Supported search behavior in v0:

- literal and regex matching
- manifest-side path, type, size, and time filters
- canonical extraction for supported document formats
- grep-like text rendering
- JSON output for agent-friendly consumption
- coverage reporting for deleted, denied, unsupported, stale, and failed candidates

Exit codes:

- `0`: complete search with matches
- `1`: complete search with no matches
- `3`: partial-coverage search
- `2`: command or runtime error

### `inspect`

Show local state for a corpus:

- provider and source
- manifest path
- index and cache paths
- document count
- tombstones
- failure count
- last sync timestamps

### `export` / `import`

Move corpus state between environments:

- `export` writes a bundle directory from local state
- `import` restores config and manifest state into a clean or existing state root

## Supported document handling

Current extractor coverage:

- plain text
- CSV, JSON, Markdown, YAML, log-like text
- gzip-compressed text
- PDF with page anchors
- DOCX with paragraph/chunk anchors
- PPTX with slide anchors
- XLSX with sheet and cell anchors

Media handling in v0:

- images and videos are visible through metadata and filtering
- content search is metadata-only for media

## Local state layout

For a corpus id like `local`, state is stored under:

```text
~/.net-rga/
  config.toml
  corpora/
    local/
      manifest.db
      index/
        index.db
      cache/
```

The manifest is the source of truth for synced namespace state.

The lexical index is optional and currently updated opportunistically from verified reads.

## Benchmarks

The repo includes a benchmark scaffold under [`benchmarks/`](benchmarks/):

- Tier 0 deterministic local-fs golden cases
- tracked Tier 1 mixed-format and provider-matrix definitions
- tracked Tier 3 freshness-chaos definitions

Benchmark corpora and result artifacts are intentionally kept out of git.

## Repository roadmap documents

- [`PLAN.md`](PLAN.md): implementation plan and phase breakdown
- [`BENCHMARK_PLAN.md`](BENCHMARK_PLAN.md): benchmark strategy and suite design
