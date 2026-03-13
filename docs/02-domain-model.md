# Core Contract

`triad`는 Claim 하나를 현재 evidence와 artifact snapshot으로 판정하는 verification kernel이다.

## Surface

| Crate | Responsibility |
|---|---|
| `triad-core` | pure verification kernel |
| `triad-fs` | filesystem reference adapter |
| `triad-cli` | `init / lint / verify / report` thin CLI |

`next`, `work`, `accept`, `agent`, runtime backend, patch draft surface는 현재 계약에 없다.

## Claim

```rust
pub struct Claim {
    pub id: ClaimId,
    pub title: String,
    pub statement: String,
    pub examples: Vec<String>,
    pub invariants: Vec<String>,
    pub notes: Option<String>,
    pub revision_digest: String,
}
```

- `Claim`은 유일한 canonical unit이다.
- `revision_digest`는 canonical claim markdown bytes의 `sha256`이다.

## Evidence

```rust
pub struct Evidence {
    pub id: EvidenceId,
    pub claim_id: ClaimId,
    pub class: EvidenceClass,
    pub kind: EvidenceKind,
    pub verdict: Verdict,
    pub verifier: String,
    pub claim_revision_digest: String,
    pub artifact_digests: BTreeMap<String, String>,
    pub command: Option<String>,
    pub locator: Option<String>,
    pub summary: Option<String>,
    pub provenance: Provenance,
    pub created_at: String,
}
```

- `Hard` evidence만 status를 바꾼다.
- `Advisory` evidence는 `reasons`에는 들어가지만 status는 바꾸지 않는다.
- evidence log는 `.triad/evidence.ndjson` append-only NDJSON이다.

## Freshness And Status

freshness는 아래 둘이 모두 같을 때만 `Fresh`다.

1. `evidence.claim_revision_digest == claim.revision_digest`
2. `evidence.artifact_digests == current_artifact_snapshot`

`ClaimStatus`는 다섯 개만 쓴다.

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

판정 순서는 고정이다.

1. fresh hard fail 존재 -> `contradicted`
2. fresh hard pass 존재 -> `confirmed`
3. fresh hard unknown 존재 -> `blocked`
4. stale hard evidence만 존재 -> `stale`
5. hard evidence 없음 -> `unsupported`

## ClaimReport

```rust
pub struct ClaimReport {
    pub claim_id: ClaimId,
    pub status: ClaimStatus,
    pub reasons: Vec<String>,
    pub fresh_evidence_ids: Vec<EvidenceId>,
    pub stale_evidence_ids: Vec<EvidenceId>,
    pub advisory_evidence_ids: Vec<EvidenceId>,
    pub strongest_verdict: Option<Verdict>,
}
```

- `strongest_verdict`는 `fail > pass > unknown` 순서다.
- `triad-core`는 filesystem, config parsing, process spawn을 모른다.
