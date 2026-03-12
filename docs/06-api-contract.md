# API Contract

## Workspace Crates

| Crate | Responsibility |
|---|---|
| `triad-core` | pure verification kernel |
| `triad-fs` | filesystem reference adapter |
| `triad-cli` | reference binary |

## `triad-core`

public surface:

- domain types in `model.rs`
- ids in `ids.rs`
- `compute_claim_revision_digest`
- `classify_evidence_freshness`
- `verify_claim`
- `verify_many`
- `short_revision`

`triad-core`는 filesystem, process spawn, config parsing을 모른다.

## `triad-fs`

public surface:

- `TriadConfig`, `CanonicalTriadConfig`
- `ClaimMarkdownAdapter`
- `EvidenceNdjsonStore`
- `SnapshotAdapter`
- `CommandCapture`
- `init_scaffold`

`triad-fs`는 reference adapter일 뿐이고, host는 `triad-core`만 직접 써도 된다.

## `triad-cli`

- binary only
- `triad-fs` + `triad-core`를 wiring 하는 thin layer
