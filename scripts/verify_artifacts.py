#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / "docs"
SCHEMAS = ROOT / "schemas"
CRATES = ROOT / "crates"

EXPECTED_DOCS = [
    "02-domain-model.md",
    "03-spec-format.md",
    "05-cli-contract.md",
]

EXPECTED_SCHEMAS = [
    "claim.schema.json",
    "claim_report.schema.json",
    "evidence.schema.json",
    "lint_report.schema.json",
    "triad_config.schema.json",
]

EXPECTED_CRATES = [
    "triad-cli",
    "triad-core",
    "triad-fs",
]


def ok(message: str) -> str:
    return f"- PASS: {message}"


def fail(message: str) -> str:
    return f"- FAIL: {message}"


def run_command(args: list[str], *, cwd: Path = ROOT) -> tuple[int, str, str]:
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=True)
    return result.returncode, result.stdout, result.stderr


def parse_json_output(raw: str, context: str) -> object:
    try:
        return json.loads(raw)
    except Exception as exc:
        raise ValueError(f"{context} JSON parse failed: {exc}") from exc


def sha256_prefixed_hex(contents: bytes) -> str:
    return f"sha256:{hashlib.sha256(contents).hexdigest()}"


def write_claim(repo_root: Path, claim_id: str, title: str, statement: str) -> Path:
    claim_dir = repo_root / "spec" / "claims"
    claim_dir.mkdir(parents=True, exist_ok=True)
    path = claim_dir / f"{claim_id}.md"
    path.write_text(
        (
            f"# {claim_id} {title}\n\n"
            "## Claim\n"
            f"{statement}\n\n"
            "## Examples\n"
            "- valid -> 200\n\n"
            "## Invariants\n"
            "- invariant holds\n"
        ),
        encoding="utf-8",
    )
    return path


def write_config(repo_root: Path, commands: list[str]) -> None:
    (repo_root / ".triad").mkdir(parents=True, exist_ok=True)
    (repo_root / "triad.toml").write_text(
        (
            "version = 2\n\n"
            "[paths]\n"
            'claim_dir = "spec/claims"\n'
            'evidence_file = ".triad/evidence.ndjson"\n\n'
            "[snapshot]\n"
            'include = ["spec/claims/**"]\n\n'
            "[verify]\n"
            f"commands = {json.dumps(commands)}\n"
        ),
        encoding="utf-8",
    )
    (repo_root / ".triad" / "evidence.ndjson").write_text("", encoding="utf-8")


def write_structured_config(repo_root: Path, commands_toml: str, snapshot_include: str) -> None:
    (repo_root / ".triad").mkdir(parents=True, exist_ok=True)
    (repo_root / "triad.toml").write_text(
        (
            "version = 2\n\n"
            "[paths]\n"
            'claim_dir = "spec/claims"\n'
            'evidence_file = ".triad/evidence.ndjson"\n\n'
            "[snapshot]\n"
            f"include = {snapshot_include}\n\n"
            "[verify]\n"
            f"commands = {commands_toml}\n"
        ),
        encoding="utf-8",
    )
    (repo_root / ".triad" / "evidence.ndjson").write_text("", encoding="utf-8")


def append_seed_evidence(repo_root: Path, row: dict[str, object]) -> None:
    evidence_file = repo_root / ".triad" / "evidence.ndjson"
    with evidence_file.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps(row))
        handle.write("\n")


def build_binary() -> Path:
    code, stdout, stderr = run_command(["cargo", "build", "-p", "triad-cli", "--quiet"])
    if code != 0:
        raise RuntimeError(f"cargo build failed: stdout={stdout!r} stderr={stderr!r}")

    binary = ROOT / "target" / "debug" / "triad"
    if not binary.is_file():
        raise RuntimeError(f"triad binary missing after build: {binary}")
    return binary


def run_triad(binary: Path, args: list[str], *, cwd: Path) -> tuple[int, str, str]:
    return run_command([str(binary), *args], cwd=cwd)


def restore_text_file(path: Path, original: str | None) -> None:
    if original is None:
        if path.exists():
            path.unlink()
    else:
        path.write_text(original, encoding="utf-8")


def report_object(binary: Path, repo_root: Path, claim_id: str, context: str) -> tuple[int, dict[str, object]]:
    code, stdout, stderr = run_triad(
        binary,
        ["report", "--claim", claim_id, "--json"],
        cwd=repo_root,
    )
    if stderr:
        raise ValueError(f"{context} wrote stderr: {stderr!r}")
    payload = parse_json_output(stdout, context)
    if not isinstance(payload, list) or len(payload) != 1 or not isinstance(payload[0], dict):
        raise ValueError(f"{context} expected one-item JSON array, got {stdout!r}")
    return code, payload[0]


def revision_digest(binary: Path, repo_root: Path, claim_id: str) -> str:
    code, stdout, stderr = run_triad(
        binary,
        ["lint", "--claim", claim_id, "--json"],
        cwd=repo_root,
    )
    if code != 0:
        raise ValueError(f"lint failed for fixture {claim_id}: stdout={stdout!r} stderr={stderr!r}")
    payload = parse_json_output(stdout, f"lint {claim_id}")
    claims = payload.get("claims") if isinstance(payload, dict) else None
    if not isinstance(claims, list) or len(claims) != 1:
        raise ValueError(f"lint payload missing single claim for {claim_id}: {stdout!r}")
    digest = claims[0].get("revision_digest")
    if not isinstance(digest, str):
        raise ValueError(f"lint payload missing revision digest for {claim_id}: {stdout!r}")
    return digest


def seed_unknown_evidence(binary: Path, repo_root: Path, claim_id: str, claim_path: Path) -> None:
    digest = revision_digest(binary, repo_root, claim_id)
    artifact_key = claim_path.relative_to(repo_root).as_posix()
    artifact_digests = {
        artifact_key: sha256_prefixed_hex(claim_path.read_bytes()),
    }
    append_seed_evidence(
        repo_root,
        {
            "id": "EVID-000001",
            "claim_id": claim_id,
            "class": "hard",
            "kind": "analysis",
            "verdict": "unknown",
            "verifier": "fixture",
            "claim_revision_digest": digest,
            "artifact_digests": artifact_digests,
            "command": None,
            "locator": None,
            "summary": "fixture unknown evidence",
            "provenance": {
                "actor": "fixture",
                "runtime": None,
                "session_id": None,
                "task_id": None,
                "workflow_id": None,
                "commit": None,
                "environment_digest": None,
            },
            "created_at": "2026-03-13T00:00:00Z",
        },
    )


def verify_semantic_fixtures(binary: Path) -> list[str]:
    lines: list[str] = []

    with tempfile.TemporaryDirectory(prefix="triad-verify-artifacts-") as temp_root:
        fixtures_root = Path(temp_root)

        confirmed_root = fixtures_root / "confirmed"
        confirmed_root.mkdir()
        write_config(confirmed_root, ["true"])
        write_claim(
            confirmed_root,
            "REQ-auth-001",
            "Login success",
            "System grants access with valid credentials.",
        )
        code, stdout, stderr = run_triad(
            binary,
            ["verify", "--claim", "REQ-auth-001", "--json"],
            cwd=confirmed_root,
        )
        if code == 0 and not stderr:
            payload = parse_json_output(stdout, "fixture confirmed verify")
            if (
                isinstance(payload, dict)
                and payload.get("report", {}).get("status") == "confirmed"
                and payload.get("evidence_ids") == ["EVID-000001"]
            ):
                lines.append(ok("semantic fixture: confirmed"))
            else:
                lines.append(fail(f"semantic fixture confirmed mismatch: {stdout!r}"))
        else:
            lines.append(
                fail(f"semantic fixture confirmed command failed: stdout={stdout!r} stderr={stderr!r}")
            )

        contradicted_root = fixtures_root / "contradicted"
        contradicted_root.mkdir()
        write_config(contradicted_root, ["false"])
        write_claim(
            contradicted_root,
            "REQ-auth-001",
            "Login fails",
            "System rejects invalid credentials.",
        )
        code, stdout, stderr = run_triad(
            binary,
            ["verify", "--claim", "REQ-auth-001", "--json"],
            cwd=contradicted_root,
        )
        if code == 2 and not stderr:
            payload = parse_json_output(stdout, "fixture contradicted verify")
            if (
                isinstance(payload, dict)
                and payload.get("report", {}).get("status") == "contradicted"
                and payload.get("evidence_ids") == ["EVID-000001"]
            ):
                lines.append(ok("semantic fixture: contradicted"))
            else:
                lines.append(fail(f"semantic fixture contradicted mismatch: {stdout!r}"))
        else:
            lines.append(
                fail(
                    f"semantic fixture contradicted command failed: stdout={stdout!r} stderr={stderr!r}"
                )
            )

        unsupported_root = fixtures_root / "unsupported"
        unsupported_root.mkdir()
        write_config(unsupported_root, ["true"])
        write_claim(
            unsupported_root,
            "REQ-auth-001",
            "No evidence yet",
            "System behavior is not verified yet.",
        )
        code, payload = report_object(binary, unsupported_root, "REQ-auth-001", "fixture unsupported report")
        if code == 0 and payload.get("status") == "unsupported":
            lines.append(ok("semantic fixture: unsupported"))
        else:
            lines.append(fail(f"semantic fixture unsupported mismatch: {payload!r}"))

        stale_root = fixtures_root / "stale"
        stale_root.mkdir()
        write_config(stale_root, ["true"])
        stale_claim = write_claim(
            stale_root,
            "REQ-auth-001",
            "Session refresh",
            "System refreshes session tokens on valid renewal.",
        )
        code, stdout, stderr = run_triad(
            binary,
            ["verify", "--claim", "REQ-auth-001", "--json"],
            cwd=stale_root,
        )
        if code != 0 or stderr:
            lines.append(fail(f"semantic fixture stale setup failed: stdout={stdout!r} stderr={stderr!r}"))
        else:
            stale_claim.write_text(
                stale_claim.read_text(encoding="utf-8").replace(
                    "valid renewal.",
                    "valid renewal and rotates the token.",
                ),
                encoding="utf-8",
            )
            code, payload = report_object(binary, stale_root, "REQ-auth-001", "fixture stale report")
            if code == 0 and payload.get("status") == "stale":
                lines.append(ok("semantic fixture: stale"))
            else:
                lines.append(fail(f"semantic fixture stale mismatch: {payload!r}"))

        blocked_root = fixtures_root / "blocked"
        blocked_root.mkdir()
        write_config(blocked_root, ["true"])
        blocked_claim = write_claim(
            blocked_root,
            "REQ-auth-001",
            "Manual checkpoint",
            "System needs human confirmation before release.",
        )
        seed_unknown_evidence(binary, blocked_root, "REQ-auth-001", blocked_claim)
        code, payload = report_object(binary, blocked_root, "REQ-auth-001", "fixture blocked report")
        if code == 2 and payload.get("status") == "blocked":
            lines.append(ok("semantic fixture: blocked"))
        else:
            lines.append(fail(f"semantic fixture blocked mismatch: {payload!r}"))

        subset_root = fixtures_root / "subset-freshness"
        subset_root.mkdir()
        (subset_root / "src").mkdir()
        (subset_root / "src" / "unrelated.rs").write_text("before", encoding="utf-8")
        write_structured_config(
            subset_root,
            """[
  { command = "true", locator = "claim:{claim_id}", artifacts = ["spec/claims/**"] }
]""",
            '["spec/claims/**", "src/**"]',
        )
        write_claim(
            subset_root,
            "REQ-auth-001",
            "Subset freshness",
            "System keeps claim freshness scoped to recorded artifacts.",
        )
        code, stdout, stderr = run_triad(
            binary,
            ["verify", "--claim", "REQ-auth-001", "--json"],
            cwd=subset_root,
        )
        if code != 0 or stderr:
            lines.append(
                fail(f"semantic fixture subset setup failed: stdout={stdout!r} stderr={stderr!r}")
            )
        else:
            (subset_root / "src" / "unrelated.rs").write_text("after", encoding="utf-8")
            code, payload = report_object(
                binary,
                subset_root,
                "REQ-auth-001",
                "fixture subset freshness report",
            )
            if code == 0 and payload.get("status") == "confirmed":
                lines.append(ok("semantic fixture: subset freshness"))
            else:
                lines.append(fail(f"semantic fixture subset freshness mismatch: {payload!r}"))

    return lines


def build_report() -> tuple[list[str], int, int]:
    lines: list[str] = []
    ok_count = 0
    fail_count = 0

    try:
        binary = build_binary()
        lines.append(ok("reference CLI binary builds"))
        ok_count += 1
    except Exception as exc:
        lines.append(fail(str(exc)))
        return lines, ok_count, fail_count + 1

    docs_on_disk = sorted(
        path.name for path in DOCS.iterdir() if path.is_file() and path.suffix == ".md"
    )
    if docs_on_disk == EXPECTED_DOCS:
        lines.append(ok("top-level docs set matches current public contract"))
        ok_count += 1
    else:
        lines.append(fail(f"unexpected docs set: {docs_on_disk}"))
        fail_count += 1

    schema_files = sorted(
        path.name for path in SCHEMAS.iterdir() if path.is_file() and path.suffix == ".json"
    )
    if schema_files == EXPECTED_SCHEMAS:
        lines.append(ok("schema set matches current public contract"))
        ok_count += 1
    else:
        lines.append(fail(f"unexpected schema set: {schema_files}"))
        fail_count += 1

    crate_dirs = sorted(path.name for path in CRATES.iterdir() if path.is_dir())
    if crate_dirs == EXPECTED_CRATES:
        lines.append(ok("crate set matches triad-core/triad-fs/triad-cli"))
        ok_count += 1
    else:
        lines.append(fail(f"unexpected crate set: {crate_dirs}"))
        fail_count += 1

    for name in EXPECTED_SCHEMAS:
        path = SCHEMAS / name
        try:
            json.loads(path.read_text(encoding="utf-8"))
            lines.append(ok(f"schema parses as JSON: schemas/{name}"))
            ok_count += 1
        except Exception as exc:
            lines.append(fail(f"schema parse error in schemas/{name}: {exc}"))
            fail_count += 1

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
        top_level_keys = sorted(config.keys())
        paths = sorted(config["paths"].keys())
        snapshot = sorted(config["snapshot"].keys())
        verify = sorted(config["verify"].keys())
        if (
            config.get("version") == 2
            and top_level_keys == ["paths", "snapshot", "verify", "version"]
            and paths == ["claim_dir", "evidence_file"]
            and snapshot == ["include"]
            and verify == ["commands"]
        ):
            lines.append(ok("triad.toml keeps only minimal v2 config sections"))
            ok_count += 1
        else:
            lines.append(fail("triad.toml does not match minimal v2 config shape"))
            fail_count += 1
    except Exception as exc:
        lines.append(fail(f"triad.toml parse error: {exc}"))
        fail_count += 1

    try:
        schema = json.loads((SCHEMAS / "triad_config.schema.json").read_text(encoding="utf-8"))
        command_items = schema["properties"]["verify"]["properties"]["commands"]["items"]
        variants = command_items.get("anyOf", [])
        has_string = any(variant.get("type") == "string" for variant in variants)
        has_object = any(variant.get("type") == "object" for variant in variants)
        if has_string and has_object:
            lines.append(ok("triad config schema supports legacy and structured verify commands"))
            ok_count += 1
        else:
            lines.append(fail("triad config schema missing legacy/structured verify command union"))
            fail_count += 1
    except Exception as exc:
        lines.append(fail(f"triad config schema verification failed: {exc}"))
        fail_count += 1

    help_code, help_stdout, help_stderr = run_triad(binary, ["--help"], cwd=ROOT)
    if (
        help_code == 0
        and all(token in help_stdout for token in ["init", "lint", "verify", "report"])
        and all(token not in help_stdout for token in ["next", "work", "accept", "agent"])
    ):
        lines.append(ok("CLI help matches current command surface"))
        ok_count += 1
    else:
        lines.append(fail(f"CLI help mismatch: stdout={help_stdout!r} stderr={help_stderr!r}"))
        fail_count += 1

    lint_code, lint_stdout, lint_stderr = run_triad(
        binary,
        ["lint", "--all", "--json"],
        cwd=ROOT,
    )
    if lint_code == 0:
        try:
            lint_json = parse_json_output(lint_stdout, "lint")
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

    repo_evidence_file = ROOT / ".triad" / "evidence.ndjson"
    original_evidence = (
        repo_evidence_file.read_text(encoding="utf-8")
        if repo_evidence_file.exists()
        else None
    )
    try:
        verify_code, verify_stdout, verify_stderr = run_triad(
            binary,
            ["verify", "--claim", "REQ-auth-001", "--json"],
            cwd=ROOT,
        )
        if verify_code == 0:
            try:
                verify_json = parse_json_output(verify_stdout, "verify")
                verify_claim_id = verify_json["claim_id"]
                report = verify_json["report"]
                evidence_ids = verify_json["evidence_ids"]
                if (
                    verify_claim_id == "REQ-auth-001"
                    and report["claim_id"] == "REQ-auth-001"
                    and report["status"] == "confirmed"
                    and isinstance(evidence_ids, list)
                    and len(evidence_ids) == 2
                ):
                    lines.append(ok("CLI verify emits direct JSON and appends fresh evidence"))
                    ok_count += 1
                else:
                    lines.append(
                        fail(f"verify output did not match expected contract: {verify_stdout!r}")
                    )
                    fail_count += 1
            except Exception as exc:
                lines.append(fail(str(exc)))
                fail_count += 1
        else:
            lines.append(
                fail(f"CLI verify failed: stdout={verify_stdout!r} stderr={verify_stderr!r}")
            )
            fail_count += 1

        report_code, report_stdout, report_stderr = run_triad(
            binary,
            ["report", "--all", "--json"],
            cwd=ROOT,
        )
        if report_code == 0:
            try:
                report_json = parse_json_output(report_stdout, "report")
                if not isinstance(report_json, list):
                    raise ValueError(f"report output was not a JSON array: {report_stdout!r}")
                reports_by_id = {report["claim_id"]: report for report in report_json}
                if (
                    "REQ-auth-001" in reports_by_id
                    and "REQ-auth-002" in reports_by_id
                    and reports_by_id["REQ-auth-001"]["status"] == "confirmed"
                    and len(reports_by_id["REQ-auth-001"]["fresh_evidence_ids"]) >= 2
                ):
                    lines.append(ok("CLI report emits direct JSON array for all claims"))
                    ok_count += 1
                else:
                    lines.append(
                        fail(f"report output did not match expected contract: {report_stdout!r}")
                    )
                    fail_count += 1
            except Exception as exc:
                lines.append(fail(str(exc)))
                fail_count += 1
        else:
            lines.append(
                fail(f"CLI report failed: stdout={report_stdout!r} stderr={report_stderr!r}")
            )
            fail_count += 1
    finally:
        restore_text_file(repo_evidence_file, original_evidence)

    for line in verify_semantic_fixtures(binary):
        lines.append(line)
        if line.startswith("- PASS:"):
            ok_count += 1
        else:
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

    print("\n".join(report))
    return 0 if fail_count == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
