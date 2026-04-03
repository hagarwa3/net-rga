#!/usr/bin/env python3

from __future__ import annotations

import argparse
import fnmatch
import json
import math
import os
import re
import shutil
import stat
import sys
import tempfile
import time
import xml.etree.ElementTree as ET
import zipfile
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
BENCHMARK_ROOT = ROOT / "benchmarks"
CORPUS_ROOTS = {
    "tier0.local_fs": BENCHMARK_ROOT / "data" / "tier0" / "local_fs",
    "tier1.local_fs.mixed_small": BENCHMARK_ROOT / "data" / "tier1" / "local_fs" / "mixed_small",
}
UNSUPPORTED_EXTENSIONS = {
    "avi",
    "gif",
    "jpeg",
    "jpg",
    "mov",
    "mp4",
    "png",
}


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def dump_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=True)
        handle.write("\n")


def iter_case_paths(case_dir: Path) -> list[Path]:
    return sorted(case_dir.glob("*.json"))


def smart_case_sensitive(text: str, mode: str) -> bool:
    if mode == "sensitive":
        return True
    if mode == "insensitive":
        return False
    return any(character.isupper() for character in text)


def compile_pattern(query: dict) -> re.Pattern[str]:
    flags = 0 if smart_case_sensitive(query["text"], query["case_sensitivity"]) else re.IGNORECASE
    if query["syntax"] == "literal":
        return re.compile(re.escape(query["text"]), flags)
    return re.compile(query["text"], flags)


def relative_path(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def file_type(path: Path) -> str:
    suffix = path.suffix.lower()
    return suffix[1:] if suffix.startswith(".") else suffix


def is_unsupported(path: Path) -> bool:
    return file_type(path) in UNSUPPORTED_EXTENSIONS


def matches_globs(relpath: str, query: dict) -> bool:
    includes = query.get("path_globs_include", [])
    excludes = query.get("path_globs_exclude", [])
    if includes and not any(fnmatch.fnmatch(relpath, pattern) for pattern in includes):
        return False
    if excludes and any(fnmatch.fnmatch(relpath, pattern) for pattern in excludes):
        return False
    file_types = query.get("file_types", [])
    if file_types and file_type(Path(relpath)) not in set(file_types):
        return False
    return True


def list_candidate_files(corpus_root: Path, query: dict) -> list[Path]:
    candidates = []
    for path in sorted(corpus_root.rglob("*")):
        if not path.is_file():
            continue
        relpath = relative_path(path, corpus_root)
        if matches_globs(relpath, query):
            candidates.append(path)
    return candidates


def apply_mutation(corpus_root: Path, mutation_set_id: str | None) -> None:
    if not mutation_set_id:
        return
    if mutation_set_id == "revoke_private_secret":
        target = corpus_root / "docs" / "private" / "secret.txt"
        target.chmod(0)
        return
    raise ValueError(f"unknown mutation set: {mutation_set_id}")


def cleanup_mutation(corpus_root: Path, mutation_set_id: str | None) -> None:
    if mutation_set_id == "revoke_private_secret":
        target = corpus_root / "docs" / "private" / "secret.txt"
        target.chmod(stat.S_IRUSR | stat.S_IWUSR)


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def search_text_document(path: Path, corpus_root: Path, pattern: re.Pattern[str]) -> tuple[list[dict], int, int]:
    content = path.read_text(encoding="utf-8")
    snippets = []
    for line_number, line in enumerate(content.splitlines(), start=1):
        if not pattern.search(line):
            continue
        snippets.append(
            {
                "document_id": relative_path(path, corpus_root),
                "anchor_kind": "line_span",
                "path": relative_path(path, corpus_root),
                "line_start": line_number,
                "line_end": line_number,
                "snippet": line,
            }
        )
    raw_bytes = path.stat().st_size
    return snippets, raw_bytes, len(content.encode("utf-8"))


def extract_docx_paragraphs(path: Path) -> list[str]:
    with zipfile.ZipFile(path) as archive:
        xml = archive.read("word/document.xml")
    root = ET.fromstring(xml)
    paragraphs = []
    for paragraph in root.iter():
        if local_name(paragraph.tag) != "p":
            continue
        text = "".join(node.text or "" for node in paragraph.iter() if local_name(node.tag) == "t").strip()
        if text:
            paragraphs.append(text)
    return paragraphs


def search_docx_document(path: Path, corpus_root: Path, pattern: re.Pattern[str]) -> tuple[list[dict], int, int]:
    paragraphs = extract_docx_paragraphs(path)
    matches = []
    extracted_text = []
    for index, paragraph in enumerate(paragraphs, start=1):
        extracted_text.append(paragraph)
        if not pattern.search(paragraph):
            continue
        matches.append(
            {
                "document_id": relative_path(path, corpus_root),
                "anchor_kind": "chunk_span",
                "path": relative_path(path, corpus_root),
                "chunk_id": f"paragraph-{index}",
                "snippet": paragraph,
            }
        )
    extracted_bytes = len("\n".join(extracted_text).encode("utf-8"))
    return matches, path.stat().st_size, extracted_bytes


def decode_pdf_literal(raw: bytes) -> str:
    output = bytearray()
    index = 0
    while index < len(raw):
        byte = raw[index]
        if byte == 0x5C and index + 1 < len(raw):
            next_byte = raw[index + 1]
            if next_byte in (0x5C, 0x28, 0x29):
                output.append(next_byte)
                index += 2
                continue
        output.append(byte)
        index += 1
    return output.decode("utf-8")


def extract_pdf_pages(path: Path) -> list[str]:
    payload = path.read_bytes()
    streams = re.findall(rb"stream\n(.*?)\nendstream", payload, re.DOTALL)
    pages = []
    for stream in streams:
        fragments = []
        for literal in re.findall(rb"\(((?:\\.|[^\\)])*)\)\s*Tj", stream):
            fragments.append(decode_pdf_literal(literal))
        page_text = " ".join(fragment.strip() for fragment in fragments if fragment.strip()).strip()
        if page_text:
            pages.append(page_text)
    return pages


def search_pdf_document(path: Path, corpus_root: Path, pattern: re.Pattern[str]) -> tuple[list[dict], int, int]:
    pages = extract_pdf_pages(path)
    matches = []
    extracted_text = []
    for index, page_text in enumerate(pages, start=1):
        extracted_text.append(page_text)
        if not pattern.search(page_text):
            continue
        matches.append(
            {
                "document_id": relative_path(path, corpus_root),
                "anchor_kind": "page_span",
                "path": relative_path(path, corpus_root),
                "page": index,
                "snippet": page_text,
            }
        )
    extracted_bytes = len("\n".join(extracted_text).encode("utf-8"))
    return matches, path.stat().st_size, extracted_bytes


def search_document(path: Path, corpus_root: Path, pattern: re.Pattern[str]) -> tuple[list[dict], int, int]:
    suffix = path.suffix.lower()
    if suffix == ".docx":
        return search_docx_document(path, corpus_root, pattern)
    if suffix == ".pdf":
        return search_pdf_document(path, corpus_root, pattern)
    return search_text_document(path, corpus_root, pattern)


def evaluate_anchor(actual_result: dict, anchor: dict) -> str:
    tolerance = anchor.get("tolerance", "exact")
    anchor_kind = anchor["anchor_kind"]
    locator = anchor["locator"]

    if anchor_kind == "line_span":
        actual_start = actual_result["line_start"]
        actual_end = actual_result["line_end"]
        expected_start = locator.get("line_start", actual_start)
        expected_end = locator.get("line_end", actual_end)
        if tolerance == "exact":
            return "pass" if actual_start == expected_start and actual_end == expected_end else "fail"
        if tolerance == "same_container":
            return "pass" if expected_start <= actual_start <= expected_end else "fail"
        return "pass" if abs(actual_start - expected_start) <= 1 else "fail"

    if anchor_kind == "page_span":
        actual_page = actual_result.get("page")
        expected_page = locator.get("page")
        return "pass" if actual_page == expected_page else "fail"

    if anchor_kind == "chunk_span":
        actual_chunk_id = actual_result.get("chunk_id")
        expected_chunk_id = locator.get("chunk_id")
        if tolerance in {"exact", "same_container"}:
            return "pass" if actual_chunk_id == expected_chunk_id else "fail"
        return "nearby" if actual_chunk_id and expected_chunk_id else "fail"

    return "nearby"


def evaluate_case(case: dict, judgment: dict, actual_results: list[dict], coverage: dict) -> dict:
    relevant = judgment["relevant_results"]
    actual_by_doc = {}
    for result in actual_results:
        actual_by_doc.setdefault(result["document_id"], []).append(result)

    disallowed = set(judgment.get("disallowed_documents", []))
    actual_docs = set(actual_by_doc.keys())

    exact_match = "pass"
    if disallowed.intersection(actual_docs):
        exact_match = "fail"
    elif len(actual_results) < judgment.get("minimum_expected_matches", 0):
        exact_match = "fail"
    else:
        for relevant_result in relevant:
            if relevant_result["document_id"] not in actual_docs:
                exact_match = "fail"
                break

    anchor_status = "not_applicable"
    snippet_status = "not_applicable"
    if relevant:
        anchor_status = "pass"
        snippet_status = "pass"
        for relevant_result in relevant:
            matches = actual_by_doc.get(relevant_result["document_id"], [])
            if not matches:
                anchor_status = "fail"
                snippet_status = "fail"
                continue
            anchors = relevant_result.get("acceptable_anchors", [])
            if anchors:
                case_anchor_pass = False
                case_anchor_nearby = False
                for match in matches:
                    for anchor in anchors:
                        outcome = evaluate_anchor(match, anchor)
                        if outcome == "pass":
                            case_anchor_pass = True
                            break
                        if outcome == "nearby":
                            case_anchor_nearby = True
                    if case_anchor_pass:
                        break
                if not case_anchor_pass:
                    anchor_status = "nearby" if case_anchor_nearby and anchor_status != "fail" else "fail"
            for expectation in relevant_result.get("snippet_expectations", []):
                snippet_pass = False
                for match in matches:
                    snippet = match["snippet"]
                    lower_snippet = snippet.lower()
                    terms = expectation.get("must_include_terms", [])
                    phrase = expectation.get("preferred_phrase")
                    if terms and not all(term.lower() in lower_snippet for term in terms):
                        continue
                    if phrase and phrase.lower() not in lower_snippet:
                        continue
                    snippet_pass = True
                    break
                if not snippet_pass:
                    snippet_status = "fail"

    actual_reasons = []
    if coverage["deleted_count"] > 0:
        actual_reasons.append("deleted")
    if coverage["denied_count"] > 0:
        actual_reasons.append("denied")
    if coverage["stale_count"] > 0:
        actual_reasons.append("stale")
    if coverage["unsupported_count"] > 0:
        actual_reasons.append("unsupported")
    if coverage["failure_count"] > 0:
        actual_reasons.append("transient_failure")

    coverage_expectation = judgment["coverage_expectation"]
    coverage_correct = False
    if coverage_expectation["status"] == "complete_required":
        coverage_correct = coverage["reported_status"] == "complete"
    elif coverage_expectation["status"] == "partial_required":
        allowed = set(coverage_expectation.get("allowed_partial_reasons", []))
        coverage_correct = coverage["reported_status"] == "partial" and set(actual_reasons).issubset(allowed)
    else:
        allowed = set(coverage_expectation.get("allowed_partial_reasons", []))
        coverage_correct = coverage["reported_status"] == "complete" or (
            coverage["reported_status"] == "partial" and set(actual_reasons).issubset(allowed)
        )

    return {
        "exact_match": exact_match,
        "anchor": anchor_status,
        "snippet": snippet_status,
        "coverage_summary": "pass" if coverage_correct else "fail",
        "coverage_correct": coverage_correct,
    }


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = max(0, math.ceil(fraction * len(ordered)) - 1)
    return ordered[index]


def corpus_copy(corpus_id: str) -> Path:
    source = CORPUS_ROOTS[corpus_id]
    if not source.exists():
        raise FileNotFoundError(
            f"missing corpus data at {source}; run 'python3 benchmarks/materialize_tier0_corpus.py' first"
        )
    tempdir = Path(tempfile.mkdtemp(prefix="net-rga-bench-"))
    target = tempdir / corpus_id.replace(".", "_")
    shutil.copytree(source, target)
    return target


def run_case(case: dict, judgment_lookup: dict[str, dict]) -> dict:
    start = time.perf_counter()
    corpus_root = corpus_copy(case["corpus_id"])
    mutation_set_id = case["run_mode"].get("mutation_set_id")
    judgment = judgment_lookup[case["expected_results"]["judgment_case_id"]]
    list_calls = 1
    stat_calls = 0
    read_calls = 0
    bytes_fetched = 0
    extracted_bytes = 0
    actual_results = []
    deleted_count = 0
    denied_count = 0
    stale_count = 0
    unsupported_count = 0
    failure_count = 0
    errors = []
    first_candidate_ms = 0.0
    first_verified_ms = 0.0

    try:
        apply_mutation(corpus_root, mutation_set_id)
        pattern = compile_pattern(case["query"])
        candidates = list_candidate_files(corpus_root, case["query"])
        stat_calls = len(candidates)
        first_candidate_ms = (time.perf_counter() - start) * 1000.0
        stop_after_first = case["query"].get("stop_after_first_verified_match", False)
        for path in candidates:
            if is_unsupported(path):
                unsupported_count += 1
                continue
            read_calls += 1
            try:
                matches, fetched_bytes, extracted = search_document(path, corpus_root, pattern)
            except PermissionError:
                denied_count += 1
                continue
            except FileNotFoundError:
                deleted_count += 1
                continue
            except OSError as exc:
                failure_count += 1
                errors.append(str(exc))
                continue
            bytes_fetched += fetched_bytes
            extracted_bytes += extracted
            if matches and first_verified_ms == 0.0:
                first_verified_ms = (time.perf_counter() - start) * 1000.0
            actual_results.extend(matches)
            if stop_after_first and matches:
                break
    finally:
        cleanup_mutation(corpus_root, mutation_set_id)
        shutil.rmtree(corpus_root.parent)

    reported_status = "partial" if any(
        count > 0 for count in [deleted_count, denied_count, stale_count, unsupported_count, failure_count]
    ) else "complete"

    coverage = {
        "reported_status": reported_status,
        "coverage_correct": False,
        "deleted_count": deleted_count,
        "denied_count": denied_count,
        "stale_count": stale_count,
        "unsupported_count": unsupported_count,
        "failure_count": failure_count,
    }
    correctness = evaluate_case(case, judgment, actual_results, coverage)
    coverage["coverage_correct"] = correctness["coverage_correct"]

    status = "passed"
    if errors:
        status = "error"
    if correctness["exact_match"] == "fail" or correctness["coverage_summary"] == "fail":
        status = "failed"
    if correctness["anchor"] == "fail" or correctness["snippet"] == "fail":
        status = "failed"

    total_ms = (time.perf_counter() - start) * 1000.0
    return {
        "case_id": case["case_id"],
        "status": status,
        "latency_ms": {
          "total": round(total_ms, 3),
          "first_candidate": round(first_candidate_ms, 3),
          "first_verified_match": round(first_verified_ms, 3),
        },
        "cost_counters": {
            "list_calls": list_calls,
            "stat_calls": stat_calls,
            "read_calls": read_calls,
            "bytes_fetched": bytes_fetched,
            "extracted_bytes": extracted_bytes,
        },
        "correctness": {
            "exact_match": correctness["exact_match"],
            "anchor": correctness["anchor"],
            "snippet": correctness["snippet"],
            "coverage_summary": correctness["coverage_summary"],
        },
        "coverage": coverage,
        "errors": errors,
    }


def load_judgment_lookup(cases: list[dict]) -> dict[str, dict]:
    lookup = {}
    loaded_files = {}
    for case in cases:
        judgment_file = ROOT / case["expected_results"]["judgment_file"]
        if judgment_file not in loaded_files:
            payload = load_json(judgment_file)
            loaded_files[judgment_file] = {entry["case_id"]: entry for entry in payload["cases"]}
        lookup.update(loaded_files[judgment_file])
    return lookup


def filter_cases(
    cases: list[dict],
    backend_mode: str | None,
    provider_mode: str | None,
) -> list[dict]:
    filtered = []
    for case in cases:
        run_mode = case["run_mode"]
        if backend_mode and run_mode["backend_mode"] != backend_mode:
            continue
        if provider_mode and run_mode["provider_mode"] != provider_mode:
            continue
        filtered.append(case)
    return filtered


def require_single_value(cases: list[dict], label: str, selector) -> str:
    values = {selector(case) for case in cases}
    if len(values) != 1:
        raise ValueError(f"benchmark run must be homogeneous for {label}, found: {sorted(values)}")
    return values.pop()


def build_run_mode(cases: list[dict]) -> dict:
    network_profiles = {
        case["run_mode"].get("network_profile", "not_applicable") for case in cases
    }
    if len(network_profiles) != 1:
        raise ValueError(
            f"benchmark run must be homogeneous for network_profile, found: {sorted(network_profiles)}"
        )
    return {
        "provider_mode": require_single_value(cases, "provider_mode", lambda case: case["run_mode"]["provider_mode"]),
        "manifest_state": require_single_value(cases, "manifest_state", lambda case: case["run_mode"]["manifest_state"]),
        "content_cache_state": require_single_value(
            cases,
            "content_cache_state",
            lambda case: case["run_mode"]["content_cache_state"],
        ),
        "backend_mode": require_single_value(cases, "backend_mode", lambda case: case["run_mode"]["backend_mode"]),
        "network_profile": network_profiles.pop(),
    }


def run_suite(case_dir: Path, output_path: Path, backend_mode: str | None, provider_mode: str | None) -> int:
    case_paths = iter_case_paths(case_dir)
    cases = filter_cases([load_json(path) for path in case_paths], backend_mode, provider_mode)
    if not cases:
        raise ValueError("no benchmark cases matched the selected filters")
    judgment_lookup = load_judgment_lookup(cases)
    git_commit = "unknown"
    dirty = False
    try:
        import subprocess

        git_commit = subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=str(ROOT),
            text=True,
        ).strip()
        dirty = bool(
            subprocess.check_output(["git", "status", "--porcelain"], cwd=str(ROOT), text=True).strip()
        )
    except Exception:
        pass

    query_results = [run_case(case, judgment_lookup) for case in cases]
    totals = [entry["latency_ms"]["total"] for entry in query_results]
    first_verified = [
        entry["latency_ms"]["first_verified_match"]
        for entry in query_results
        if entry["latency_ms"]["first_verified_match"] > 0
    ]
    suite_id = require_single_value(cases, "suite_id", lambda case: case["suite_id"])
    corpus_id = require_single_value(cases, "corpus_id", lambda case: case["corpus_id"])
    run_mode_payload = build_run_mode(cases)
    aggregate = {
        "query_count": len(query_results),
        "pass_count": sum(1 for entry in query_results if entry["status"] == "passed"),
        "fail_count": sum(1 for entry in query_results if entry["status"] == "failed"),
        "error_count": sum(1 for entry in query_results if entry["status"] == "error"),
        "latency_ms": {
            "p50_total": round(percentile(totals, 0.50), 3),
            "p95_total": round(percentile(totals, 0.95), 3),
            "p50_first_verified_match": round(percentile(first_verified, 0.50), 3) if first_verified else 0.0,
            "p95_first_verified_match": round(percentile(first_verified, 0.95), 3) if first_verified else 0.0,
        },
        "cost_counters": {
            "list_calls": sum(entry["cost_counters"]["list_calls"] for entry in query_results),
            "stat_calls": sum(entry["cost_counters"]["stat_calls"] for entry in query_results),
            "read_calls": sum(entry["cost_counters"]["read_calls"] for entry in query_results),
            "bytes_fetched": sum(entry["cost_counters"]["bytes_fetched"] for entry in query_results),
            "extracted_bytes": sum(entry["cost_counters"].get("extracted_bytes", 0) for entry in query_results),
        },
    }
    report = {
        "schema_version": "1.0",
        "run_id": f"{suite_id}-{run_mode_payload['backend_mode']}-{int(time.time())}",
        "suite_id": suite_id,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "corpus_id": corpus_id,
        "tool_revision": {
            "git_commit": git_commit,
            "dirty": dirty,
        },
        "run_mode": run_mode_payload,
        "query_results": query_results,
        "aggregate_metrics": aggregate,
    }
    dump_json(output_path, report)
    print(f"wrote benchmark report to {output_path}")
    return 0 if aggregate["fail_count"] == 0 and aggregate["error_count"] == 0 else 1


def compare_reports(before_path: Path, after_path: Path) -> int:
    before = load_json(before_path)
    after = load_json(after_path)
    before_cases = {entry["case_id"]: entry for entry in before["query_results"]}
    after_cases = {entry["case_id"]: entry for entry in after["query_results"]}
    regressions = []
    for case_id, before_entry in before_cases.items():
        after_entry = after_cases.get(case_id)
        if after_entry is None:
            regressions.append(f"{case_id}: missing from new report")
            continue
        before_rank = {"passed": 0, "failed": 1, "error": 2}[before_entry["status"]]
        after_rank = {"passed": 0, "failed": 1, "error": 2}[after_entry["status"]]
        if after_rank > before_rank:
            regressions.append(
                f"{case_id}: status regressed from {before_entry['status']} to {after_entry['status']}"
            )
    if regressions:
        for regression in regressions:
            print(regression)
        return 1
    print("no status regressions detected")
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Net-RGA benchmark harness")
    subparsers = parser.add_subparsers(dest="command", required=True)

    run_parser = subparsers.add_parser("run", help="run benchmark cases")
    run_parser.add_argument(
        "--cases",
        default=str(BENCHMARK_ROOT / "cases" / "tier0"),
        help="directory containing benchmark case JSON files",
    )
    run_parser.add_argument(
        "--output",
        default=str(BENCHMARK_ROOT / "results" / "tier0_local_fs.json"),
        help="output report path",
    )
    run_parser.add_argument(
        "--backend-mode",
        choices=["disabled", "enabled"],
        default="disabled",
        help="run only cases for the selected backend mode",
    )
    run_parser.add_argument(
        "--provider-mode",
        choices=["local_fs", "s3_compatible", "s3", "corpus_default"],
        default=None,
        help="run only cases for the selected provider mode",
    )

    compare_parser = subparsers.add_parser("compare", help="compare two benchmark reports")
    compare_parser.add_argument("before")
    compare_parser.add_argument("after")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.command == "run":
        return run_suite(
            Path(args.cases),
            Path(args.output),
            args.backend_mode,
            args.provider_mode,
        )
    return compare_reports(Path(args.before), Path(args.after))


if __name__ == "__main__":
    sys.exit(main())
