#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
from ast import literal_eval
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    class _TomllibCompat:
        @staticmethod
        def loads(text: str) -> dict[str, object]:
            return _load_toml_subset(text)

    tomllib = _TomllibCompat()

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


def _value_complete(value: str) -> bool:
    depth = 0
    in_string = False
    escaped = False
    for ch in value:
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            continue
        if ch == '"':
            in_string = True
        elif ch in "[{":
            depth += 1
        elif ch in "]}":
            depth -= 1
    return depth == 0 and not in_string


def _parse_toml_value(value: str) -> object:
    value = value.strip()
    if value.startswith('"') or value.startswith("["):
        return literal_eval(value)
    if value.isdigit():
        return int(value)
    return value


def _ensure_section(root: dict[str, object], section: str) -> dict[str, object]:
    current = root
    for part in section.split("."):
        child = current.setdefault(part, {})
        if not isinstance(child, dict):
            raise ValueError(f"section conflicts with value: {section}")
        current = child
    return current


def _load_toml_subset(text: str) -> dict[str, object]:
    root: dict[str, object] = {}
    current = root
    pending_key: str | None = None
    pending_value: list[str] = []

    for raw_line in text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue

        if pending_key is not None:
            pending_value.append(line)
            joined = "\n".join(pending_value)
            if _value_complete(joined):
                current[pending_key] = _parse_toml_value(joined)
                pending_key = None
                pending_value = []
            continue

        if line.startswith("[") and line.endswith("]"):
            current = _ensure_section(root, line[1:-1].strip())
            continue

        key, sep, value = line.partition("=")
        if sep == "":
            raise ValueError(f"unsupported TOML line: {raw_line}")
        key = key.strip()
        value = value.strip()
        if _value_complete(value):
            current[key] = _parse_toml_value(value)
        else:
            pending_key = key
            pending_value = [value]

    if pending_key is not None:
        raise ValueError(f"incomplete TOML value for key: {pending_key}")

    return root


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
            and "triad.toml" in config["snapshot"]["include"]
        ):
            lines.append(ok("triad.toml keeps only minimal v2 config sections and tracks config changes"))
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
    help_lines = help_stdout.splitlines()
    try:
        commands_start = help_lines.index("Commands:") + 1
        options_start = help_lines.index("Options:")
        command_names = {
            line.split()[0]
            for line in help_lines[commands_start:options_start]
            if line.strip()
        }
    except ValueError:
        command_names = set()
    if (
        help_code == 0
        and "--repo-root <PATH>" in help_stdout
        and {"init", "lint", "verify", "report"}.issubset(command_names)
        and {"next", "work", "accept", "agent"}.isdisjoint(command_names)
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
