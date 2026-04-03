# Limitations And Deferred Features

This file tracks the current v0/MVP boundaries for `net-rga`.

## Current limitations

### Freshness and sync

- sync is manual; there is no live change-feed or background refresh loop
- search trusts the local manifest for planning, so provider-side drift between syncs can still affect candidate quality
- partial coverage is reported for deleted, denied, unsupported, and failed reads, but richer stale-state reasoning is still limited

### Provider coverage

- first-class providers are local filesystem and S3/S3-compatible storage only
- Google Drive, Dropbox, Azure Blob, GCS, and native document-platform connectors are not implemented yet
- provider-native search capabilities are not integrated into the execution path

### Search behavior

- search is lexical-first only; there is no semantic/vector retrieval path in v0
- regex queries do not benefit from the local lexical sidecar
- the local lexical sidecar is explicit-build only; there is no background incremental update loop yet
- ignore-file compatibility is not yet at ripgrep parity

### Document extraction

- OCR is not implemented
- audio/video transcription is not implemented
- images and videos are metadata-visible but not content-searchable
- extraction quality for complex layout-heavy PDFs and large spreadsheets is still heuristic rather than fully structure-aware

### CLI and output

- grep-like text output is line-first; richer anchors such as page, slide, and sheet matches are preserved internally and in JSON, but the plain-text renderer is still simpler than the internal anchor model
- bundle export/import is currently directory-based rather than archive-based
- provider credentials are not bundled during export/import

## Deferred features

These are intentionally postponed beyond v0:

- connector set beyond local filesystem and S3/S3-compatible
- event-driven freshness and delta ingestion
- background extraction workers
- semantic/vector backends
- hosted indexing/query service
- OCR and multimodal extraction
- richer ranking, caching policy controls, and prefetch orchestration
- deeper provider permissions and open-URL support in the CLI

## What to improve next

If work continues beyond the current MVP, the highest-value next steps are:

1. abstract the embedded lexical sidecar more cleanly behind interchangeable local and hosted backends
2. improve plain-text rendering for non-line anchors
3. add credential-aware S3 CLI ergonomics and more provider connectors
4. expand freshness handling beyond manual sync plus partial coverage reporting
5. add OCR/transcription and native document-platform adapters
