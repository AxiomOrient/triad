# Domain Model

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
- `revision_digest`는 canonical markdown bytes의 `sha256`이다.

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
- evidence는 `.triad/evidence.ndjson`에 append-only로 쌓인다.

## ClaimStatus

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

판정 순서:

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

- report는 explanation 가능한 output이어야 한다.
- strongest verdict는 fail > pass > unknown 순으로 계산한다.
