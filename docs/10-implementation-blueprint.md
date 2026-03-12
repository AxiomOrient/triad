# Implementation Blueprint

## Workspace

```text
triad/
├─ Cargo.toml
├─ triad.toml
├─ schemas/
├─ scripts/
├─ spec/
│  └─ claims/
└─ crates/
   ├─ triad-core/
   ├─ triad-fs/
   └─ triad-cli/
```

## `triad-core`

```text
src/
├─ error.rs
├─ ids.rs
├─ model.rs
├─ revision.rs
├─ freshness.rs
├─ verify.rs
├─ report.rs
└─ lib.rs
```

## `triad-fs`

```text
src/
├─ claim_markdown.rs
├─ evidence_ndjson.rs
├─ snapshot.rs
├─ config.rs
├─ command_capture.rs
├─ init.rs
└─ lib.rs
```

## `triad-cli`

```text
src/
├─ cli.rs
├─ dispatch.rs
├─ output.rs
├─ parsing.rs
├─ exit_codes.rs
├─ main.rs
└─ tests.rs
```

## Frozen CLI Surface

```text
triad init
triad lint [--claim <CLAIM_ID> | --all] [--json]
triad verify --claim <CLAIM_ID> [--json]
triad report [--claim <CLAIM_ID> | --all] [--json]
```

## Frozen Schema Set

- `claim.schema.json`
- `evidence.schema.json`
- `claim_report.schema.json`
- `lint_report.schema.json`
- `triad_config.schema.json`
