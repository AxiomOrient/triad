# Implementation Blueprint

## Scope

- 실제 구현 스켈레톤을 고정한다: Cargo workspace, triad.toml, clap tree, Rust modules, JSON schema catalog.

## Out of Scope

- 철학적 배경 설명은 최소화하고 구현 결정만 적는다.

## Frozen Workspace Layout

```text
triad/
├─ Cargo.toml
├─ triad.toml
├─ AGENTS.md
├─ docs/
├─ schemas/
├─ scripts/
├─ spec/
│  └─ claims/
└─ crates/
   ├─ triad-core/
   ├─ triad-config/
   ├─ triad-runtime/
   └─ triad-cli/
```

## Cargo Workspace

- virtual workspace
- `resolver = "3"`
- edition = `2024`
- crates: `triad-core`, `triad-config`, `triad-runtime`, `triad-cli`

## Root Configuration

### `triad.toml`
- path settings
- agent runtime defaults
- backend-specific runtime defaults
- verify defaults
- guardrails
- schema dir

설정은 의도적으로 작다.  
우선순위 가중치, custom workflow, provider matrix는 두지 않는다.
표준 backend 이름은 `codex`, `claude`, `gemini` 세 개로 제한한다.

## clap Command Tree

```text
triad
├─ init [--force]
├─ next
├─ work [CLAIM_ID] [--dry-run] [--model <MODEL>] [--effort <LEVEL>]
├─ verify [CLAIM_ID] [--with-probe] [--full-workspace]
├─ accept [PATCH_ID | --latest]
├─ status [--claim <CLAIM_ID>] [--verbose]
└─ agent
   ├─ claim
   │  ├─ list
   │  ├─ get <CLAIM_ID>
   │  └─ next
   ├─ drift detect --claim <CLAIM_ID>
   ├─ run --claim <CLAIM_ID>
   ├─ verify --claim <CLAIM_ID> [--with-probe] [--full-workspace]
   ├─ patch propose --claim <CLAIM_ID>
   ├─ patch apply --patch <PATCH_ID>
   └─ status [--claim <CLAIM_ID>]
```

## JSON Schema Catalog

- `envelope.schema.json`
- `agent.claim.list.schema.json`
- `agent.claim.get.schema.json`
- `agent.claim.next.schema.json`
- `agent.drift.detect.schema.json`
- `agent.run.schema.json`
- `agent.verify.schema.json`
- `agent.patch.propose.schema.json`
- `agent.patch.apply.schema.json`
- `agent.status.schema.json`

## Rust Module Allocation

### `triad-core`
- `lib.rs` (re-export hub for public surface)
- `ids.rs`
- `model.rs`
- `api.rs`
- `error.rs`

### `triad-config`
- `lib.rs`

### `triad-runtime`
- `lib.rs` (public surface re-export, `LocalTriad` impl, parser/store/drift/verify/patch internals)
- `agent_runtime/mod.rs`
- `agent_runtime/adapter.rs`
- `agent_runtime/process_runner.rs`
- `agent_runtime/backend_probe.rs`
- `agent_runtime/session.rs`
- `agent_runtime/workspace_stage.rs`
- `agent_runtime/codex.rs`
- `agent_runtime/claude.rs`
- `agent_runtime/gemini.rs`

### `triad-cli`
- `cli.rs`
- `main.rs`

## Execution Sequence

### Sequence 0 — Architecture and Scaffold
- workspace manifest and crate skeleton
- `triad init` scaffold behavior
- `.triad/`, `spec/claims/`, `schemas/` 초기 구조
- core domain types, IDs, `TriadApi`, `TriadError`
- config loader and repo root discovery

### Sequence 1 — Parsing and State
- strict claim parser
- evidence log reader/writer
- drift calculator
- `next`, `status`

### Sequence 2 — Verify
- targeted runner
- default layers
- evidence append
- stale detection

### Sequence 3 — Work
- one-shot agent runtime adapter integration
- backend capability probe
- structured output contract
- temp workspace guardrails and guarded copy-back

### Sequence 4 — Patch
- diff proposal
- apply with conflict detection
- accept flow

### Sequence 5 — Contract Freeze
- agent JSON stability
- golden tests for schemas
- docs/implementation parity checks

### Sequence 6 — Human CLI Shell
- clap command wiring to runtime
- human output formatter
- `next`, `status`, `work`, `verify`, `accept` text outputs
- exit code mapping

### Sequence 7 — End-to-End and Hardening
- happy/contradicted/blocked/stale/needs-spec fixture tests
- malformed state and malformed claim handling
- consistent diagnostics and exit codes

### Sequence 8 — Packaging and Release
- README quickstart
- CI gate (fmt, clippy, test, consistency script)
- install smoke test
- release checklist


## Pinned Files

| Artifact | Path |
|---|---|
| Root workspace manifest | [`../Cargo.toml`](../Cargo.toml) |
| Project config | [`../triad.toml`](../triad.toml) |
| Agent rules | [`../AGENTS.md`](../AGENTS.md) |
| Core domain crate | [`../crates/triad-core/src/lib.rs`](../crates/triad-core/src/lib.rs) |
| Public API trait | [`../crates/triad-core/src/api.rs`](../crates/triad-core/src/api.rs) |
| Domain model | [`../crates/triad-core/src/model.rs`](../crates/triad-core/src/model.rs) |
| Config model | [`../crates/triad-config/src/lib.rs`](../crates/triad-config/src/lib.rs) |
| Runtime facade | [`../crates/triad-runtime/src/lib.rs`](../crates/triad-runtime/src/lib.rs) |
| clap tree | [`../crates/triad-cli/src/cli.rs`](../crates/triad-cli/src/cli.rs) |
| CLI entrypoint | [`../crates/triad-cli/src/main.rs`](../crates/triad-cli/src/main.rs) |
| JSON schemas | [`../schemas/`](../schemas/) |
| Verification script | [`../scripts/verify_artifacts.py`](../scripts/verify_artifacts.py) |
