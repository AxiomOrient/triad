use triad_config::{CONFIG_FILE_NAME, TriadConfig};
use triad_core::TriadError;

use crate::LocalTriad;
use crate::fs_support::{ensure_dir, ensure_file, ensure_text_file};

pub(crate) const DEFAULT_AGENTS_MD: &str = r#"# AGENTS.md

## Workflow
- Work on exactly one claim per run.
- Use the standard loop only: next -> work -> verify -> accept.
- Treat the selected claim as the only work scope for that run.
- Never edit `spec/claims/**` directly during `work`.
- Change spec only through patch draft creation and `accept`.
- Keep code and tests scoped to the selected claim.
- Prefer the smallest change that can be verified.

## Guardrails
- Do not run `git commit` or `git push`.
- Do not remove files recursively outside an explicitly approved temporary workspace.
- Do not modify unrelated claims.
- Do not write unrelated docs, schemas, or config files during `work`; stay inside selected code/test scope.
- Do not skip verification after code changes.

## Verification
- Run targeted verification first.
- Default verification layers are unit, contract, integration.
- Treat probe as opt-in.
- Record behavior changes as patch drafts, not direct spec rewrites.

## Output
- Human CLI may be concise.
- Agent CLI must emit stable JSON only on stdout.
- Agent diagnostics and errors belong on stderr, not stdout.
- If blocked, explain the blocker explicitly and stop.
- If malformed state or malformed claim is encountered, report the exact claim or file and the cause.
"#;

pub(crate) const DEFAULT_GITIGNORE: &str = r#"# triad derived state
.triad/*
!.triad/evidence.ndjson
!.triad/patches/
!.triad/patches/**
.triad/cache.sqlite
.triad/cache.sqlite-*
.triad/runs/
.triad/runs/**
.triad/tmp/
.triad/tmp/**

# Common build and OS noise
target/
.DS_Store
"#;

pub(crate) const DEFAULT_SCHEMA_FILES: &[(&str, &str)] = &[
    (
        "envelope.schema.json",
        include_str!("../../../schemas/envelope.schema.json"),
    ),
    (
        "agent.claim.list.schema.json",
        include_str!("../../../schemas/agent.claim.list.schema.json"),
    ),
    (
        "agent.claim.get.schema.json",
        include_str!("../../../schemas/agent.claim.get.schema.json"),
    ),
    (
        "agent.claim.next.schema.json",
        include_str!("../../../schemas/agent.claim.next.schema.json"),
    ),
    (
        "agent.drift.detect.schema.json",
        include_str!("../../../schemas/agent.drift.detect.schema.json"),
    ),
    (
        "agent.run.schema.json",
        include_str!("../../../schemas/agent.run.schema.json"),
    ),
    (
        "agent.verify.schema.json",
        include_str!("../../../schemas/agent.verify.schema.json"),
    ),
    (
        "agent.patch.propose.schema.json",
        include_str!("../../../schemas/agent.patch.propose.schema.json"),
    ),
    (
        "agent.patch.apply.schema.json",
        include_str!("../../../schemas/agent.patch.apply.schema.json"),
    ),
    (
        "agent.status.schema.json",
        include_str!("../../../schemas/agent.status.schema.json"),
    ),
];

pub(crate) fn init_scaffold(triad: &LocalTriad, force: bool) -> Result<(), TriadError> {
    ensure_text_file(
        triad.config.repo_root.join(CONFIG_FILE_NAME).as_std_path(),
        &TriadConfig::bootstrap_toml()?,
    )?;
    ensure_text_file(
        triad.config.repo_root.join("AGENTS.md").as_std_path(),
        DEFAULT_AGENTS_MD,
    )?;
    ensure_text_file(
        triad.config.repo_root.join(".gitignore").as_std_path(),
        DEFAULT_GITIGNORE,
    )?;
    ensure_dir(triad.config.paths.docs_dir.as_std_path())?;
    ensure_dir(triad.config.paths.schema_dir.as_std_path())?;
    for (file_name, contents) in DEFAULT_SCHEMA_FILES {
        ensure_text_file(
            triad.config.paths.schema_dir.join(file_name).as_std_path(),
            contents,
        )?;
    }
    ensure_dir(triad.config.paths.claim_dir.as_std_path())?;
    ensure_dir(triad.config.paths.state_dir.as_std_path())?;
    ensure_dir(triad.config.paths.patch_dir.as_std_path())?;
    ensure_dir(triad.config.paths.run_dir.as_std_path())?;
    ensure_file(triad.config.paths.evidence_file.as_std_path(), force)?;

    Ok(())
}
