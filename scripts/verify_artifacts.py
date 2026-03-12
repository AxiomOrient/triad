#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
SCHEMAS = ROOT / "schemas"

EXPECTED_DOCS = [
    "00-document-map.md",
    "01-product-charter.md",
    "02-domain-model.md",
    "03-spec-format.md",
    "04-workflows.md",
    "05-cli-contract.md",
    "06-api-contract.md",
    "07-storage-model.md",
    "08-runtime-integration.md",
    "09-verification-and-ratchet.md",
    "10-implementation-blueprint.md",
    "11-consistency-report.md",
    "15-release-readiness.md",
]

EXPECTED_SCHEMAS = [
    "claim.schema.json",
    "evidence.schema.json",
    "claim_report.schema.json",
    "lint_report.schema.json",
    "triad_config.schema.json",
]

OBSOLETE_CRATES = [
    ROOT / "crates" / "triad-config",
    ROOT / "crates" / "triad-runtime",
]

OBSOLETE_SCHEMAS = [
    "agent.claim.get.schema.json",
    "agent.claim.list.schema.json",
    "agent.claim.next.schema.json",
    "agent.drift.detect.schema.json",
    "agent.patch.apply.schema.json",
    "agent.patch.propose.schema.json",
    "agent.run.schema.json",
    "agent.status.schema.json",
    "agent.verify.schema.json",
    "envelope.schema.json",
]


def ok(msg: str) -> str:
    return f"- PASS: {msg}"


def fail(msg: str) -> str:
    return f"- FAIL: {msg}"


def run_command(args: list[str]) -> tuple[int, str, str]:
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True)
    return result.returncode, result.stdout, result.stderr


def build_report() -> tuple[list[str], int, int]:
    lines: list[str] = []
    ok_count = 0
    fail_count = 0

    for name in EXPECTED_DOCS:
        path = DOCS / name
        if path.is_file():
            lines.append(ok(f"document exists: docs/{name}"))
            ok_count += 1
        else:
            lines.append(fail(f"missing document: docs/{name}"))
            fail_count += 1

    for name in EXPECTED_SCHEMAS:
        path = SCHEMAS / name
        if not path.is_file():
            lines.append(fail(f"missing schema: schemas/{name}"))
            fail_count += 1
            continue
        try:
            json.loads(path.read_text(encoding="utf-8"))
            lines.append(ok(f"schema parses as JSON: schemas/{name}"))
            ok_count += 1
        except Exception as exc:
            lines.append(fail(f"schema parse error in schemas/{name}: {exc}"))
            fail_count += 1

    for path in OBSOLETE_CRATES:
        if path.exists():
            lines.append(fail(f"obsolete crate still exists: {path.relative_to(ROOT)}"))
            fail_count += 1
        else:
            lines.append(ok(f"obsolete crate removed: {path.relative_to(ROOT)}"))
            ok_count += 1

    for name in OBSOLETE_SCHEMAS:
        path = SCHEMAS / name
        if path.exists():
            lines.append(fail(f"obsolete schema still exists: schemas/{name}"))
            fail_count += 1
        else:
            lines.append(ok(f"obsolete schema removed: schemas/{name}"))
            ok_count += 1

    try:
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        members = cargo["workspace"]["members"]
        expected_members = [
            "crates/triad-core",
            "crates/triad-fs",
            "crates/triad-cli",
        ]
        if members == expected_members:
            lines.append(ok("workspace members match triad-core/triad-fs/triad-cli"))
            ok_count += 1
        else:
            lines.append(fail(f"unexpected workspace members: {members}"))
            fail_count += 1
    except Exception as exc:
        lines.append(fail(f"root Cargo.toml parse error: {exc}"))
        fail_count += 1

    try:
        config = tomllib.loads((ROOT / "triad.toml").read_text(encoding="utf-8"))
        paths = sorted(config["paths"].keys())
        snapshot = sorted(config["snapshot"].keys())
        verify = sorted(config["verify"].keys())
        if config.get("version") == 2 and paths == ["claim_dir", "evidence_file"] and snapshot == ["include"] and verify == ["commands"]:
            lines.append(ok("triad.toml matches minimal v2 config"))
            ok_count += 1
        else:
            lines.append(fail("triad.toml does not match minimal v2 config"))
            fail_count += 1
    except Exception as exc:
        lines.append(fail(f"triad.toml parse error: {exc}"))
        fail_count += 1

    help_code, help_stdout, help_stderr = run_command(["cargo", "run", "-p", "triad-cli", "--", "--help"])
    if help_code == 0 and all(token in help_stdout for token in ["init", "lint", "verify", "report"]) and all(token not in help_stdout for token in ["work", "accept", "agent"]):
        lines.append(ok("CLI help matches current command surface"))
        ok_count += 1
    else:
        lines.append(fail(f"CLI help mismatch: stdout={help_stdout!r} stderr={help_stderr!r}"))
        fail_count += 1

    lint_code, lint_stdout, lint_stderr = run_command(
        ["cargo", "run", "-p", "triad-cli", "--", "lint", "--all", "--json"]
    )
    if lint_code == 0:
        try:
            lint_json = json.loads(lint_stdout)
            claim_ids = [claim["claim_id"] for claim in lint_json["claims"]]
            if lint_json["ok"] is True and "REQ-auth-001" in claim_ids and "REQ-auth-002" in claim_ids:
                lines.append(ok("example claims parse via CLI lint"))
                ok_count += 1
            else:
                lines.append(fail(f"lint output missing expected claims: {lint_stdout!r}"))
                fail_count += 1
        except Exception as exc:
            lines.append(fail(f"lint JSON parse failed: {exc}"))
            fail_count += 1
    else:
        lines.append(fail(f"CLI lint failed: stdout={lint_stdout!r} stderr={lint_stderr!r}"))
        fail_count += 1

    doc_map = (DOCS / "00-document-map.md").read_text(encoding="utf-8")
    missing = [name for name in EXPECTED_DOCS if name not in doc_map]
    if not missing:
        lines.append(ok("document map includes expected docs"))
        ok_count += 1
    else:
        lines.append(fail(f"document map missing docs: {missing}"))
        fail_count += 1

    return lines, ok_count, fail_count


def main() -> int:
    lines, ok_count, fail_count = build_report()
    verdict = "READY" if fail_count == 0 else "NOT READY"
    report = [
        "# Consistency Report",
        "",
        "## Summary",
        "",
        f"- PASS count: {ok_count}",
        f"- FAIL count: {fail_count}",
        "",
        "## Checks",
        "",
        *lines,
        "",
        "## Verdict",
        "",
        verdict,
        "",
    ]

    (DOCS / "11-consistency-report.md").write_text("\n".join(report), encoding="utf-8")
    print("\n".join(report))
    return 0 if fail_count == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
