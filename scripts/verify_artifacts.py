#!/usr/bin/env python3
from __future__ import annotations

import json
import re
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
    "15-release-readiness.md",
]

EXPECTED_SCHEMAS = [
    "envelope.schema.json",
    "agent.claim.list.schema.json",
    "agent.claim.get.schema.json",
    "agent.claim.next.schema.json",
    "agent.drift.detect.schema.json",
    "agent.run.schema.json",
    "agent.verify.schema.json",
    "agent.patch.propose.schema.json",
    "agent.patch.apply.schema.json",
    "agent.status.schema.json",
]

REQUIRED_AGENT_RULES = [
    "Work on exactly one claim per run.",
    "Never edit `spec/claims/**` directly during `work`.",
    "Do not run `git commit` or `git push`.",
]

BROKEN_VERSION_RE = re.compile(r"\b[Vv](?:1|2)\b")


def fail(msg: str) -> str:
    return f"- FAIL: {msg}"


def ok(msg: str) -> str:
    return f"- PASS: {msg}"


def parse_markdown_links(text: str):
    for match in re.finditer(r"\[[^\]]+\]\(([^)]+)\)", text):
        yield match.group(1)


def validate_doc_map(text: str):
    expected = sorted(path.name for path in DOCS.glob("*.md"))
    missing = [name for name in expected if name not in text]
    return missing


def main() -> int:
    lines = []
    ok_count = 0
    fail_count = 0

    # docs exist
    for name in EXPECTED_DOCS:
        path = DOCS / name
        if path.exists():
            lines.append(ok(f"document exists: docs/{name}"))
            ok_count += 1
        else:
            lines.append(fail(f"missing document: docs/{name}"))
            fail_count += 1

    # schema files parse
    for name in EXPECTED_SCHEMAS:
        path = SCHEMAS / name
        if not path.exists():
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

    # cargo workspace
    cargo_path = ROOT / "Cargo.toml"
    try:
        cargo = tomllib.loads(cargo_path.read_text(encoding="utf-8"))
        members = cargo["workspace"]["members"]
        missing_members = [m for m in members if not (ROOT / m / "Cargo.toml").exists()]
        if missing_members:
            lines.append(fail(f"workspace members missing Cargo.toml: {missing_members}"))
            fail_count += 1
        else:
            lines.append(ok("root Cargo.toml parses and all workspace members exist"))
            ok_count += 1
    except Exception as exc:
        lines.append(fail(f"root Cargo.toml parse error: {exc}"))
        fail_count += 1

    # triad.toml
    try:
        tomllib.loads((ROOT / "triad.toml").read_text(encoding="utf-8"))
        lines.append(ok("triad.toml parses"))
        ok_count += 1
    except Exception as exc:
        lines.append(fail(f"triad.toml parse error: {exc}"))
        fail_count += 1

    # markdown links
    for md in ROOT.rglob("*.md"):
        text = md.read_text(encoding="utf-8")
        for link in parse_markdown_links(text):
            if link.startswith(("http://", "https://", "mailto:")):
                continue
            target = (md.parent / link).resolve()
            if not target.exists():
                lines.append(fail(f"broken relative link in {md.relative_to(ROOT)} -> {link}"))
                fail_count += 1
            else:
                ok_count += 1

        if BROKEN_VERSION_RE.search(text):
            lines.append(fail(f"versioned roadmap language present in {md.relative_to(ROOT)}"))
            fail_count += 1

    # doc map coverage
    doc_map_text = (DOCS / "00-document-map.md").read_text(encoding="utf-8")
    missing = validate_doc_map(doc_map_text)
    if missing:
        lines.append(fail(f"document map missing entries: {missing}"))
        fail_count += 1
    else:
        lines.append(ok("document map includes all expected docs"))
        ok_count += 1

    # root scaffold
    required_paths = [
        ROOT / ".triad" / "evidence.ndjson",
        ROOT / ".triad" / "patches",
        ROOT / ".triad" / "runs",
        ROOT / ".gitignore",
        ROOT / "AGENTS.md",
        ROOT / "triad.toml",
    ]
    for path in required_paths:
        if path.exists():
            lines.append(ok(f"scaffold exists: {path.relative_to(ROOT)}"))
            ok_count += 1
        else:
            lines.append(fail(f"missing scaffold path: {path.relative_to(ROOT)}"))
            fail_count += 1

    # AGENTS.md rules
    agents = (ROOT / "AGENTS.md").read_text(encoding="utf-8")
    for rule in REQUIRED_AGENT_RULES:
        if rule in agents:
            lines.append(ok(f"AGENTS.md contains required rule: {rule}"))
            ok_count += 1
        else:
            lines.append(fail(f"AGENTS.md missing required rule: {rule}"))
            fail_count += 1

    # CLI command cross-check
    cli_rs = (ROOT / "crates" / "triad-cli" / "src" / "cli.rs").read_text(encoding="utf-8")
    expected_tokens = [
        "Init",
        "Next",
        "Work",
        "Verify",
        "Accept",
        "Status",
        "AgentCommand",
        "AgentClaimCommand",
        "AgentDriftCommand",
        "AgentPatchCommand",
    ]
    for token in expected_tokens:
        if token in cli_rs:
            lines.append(ok(f"CLI skeleton includes token: {token}"))
            ok_count += 1
        else:
            lines.append(fail(f"CLI skeleton missing token: {token}"))
            fail_count += 1

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
        "READY" if fail_count == 0 else "NOT READY",
        "",
    ]
    out = DOCS / "11-consistency-report.md"
    out.write_text("\n".join(report), encoding="utf-8")
    print("\n".join(report))
    return 0 if fail_count == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
