# API Contract

## Scope

- Rust crate 경계와 public API 표면을 고정한다.
- 무엇이 public contract이고 무엇이 internal detail인지 정한다.

## Out of Scope

- clap parsing과 CLI help text는 다루지 않는다.

## Workspace Crates

| Crate | Responsibility | Public? |
|---|---|---|
| `triad-core` | 순수 도메인 모델, ID, 상태 계산, 공개 trait | Yes |
| `triad-config` | triad.toml 로딩과 정적 설정 모델 | Yes |
| `triad-runtime` | local engine orchestration, one-shot runtime adapter coordination, verification coordination | Yes |
| `triad-cli` | clap 기반 사용자/agent CLI | Binary only |

## Public Rust Surface

`triad-core` 는 도메인과 contract를 담는 유일한 public foundation crate다.

### Re-export Policy
- `triad-core` 는 domain types와 `TriadApi` trait를 export한다.
- `triad-config` 는 `TriadConfig`, `CanonicalTriadConfig`, repo discovery helpers를 export한다.
- `triad-runtime` 는 `LocalTriad` 와 canonicalized config 기반 runtime builder를 export한다.
- `triad-cli` 는 binary only이다.

## Runtime Builder Input

- `triad-runtime` 는 raw TOML 문자열을 파싱하지 않는다.
- `LocalTriad` 는 validation을 통과한 `CanonicalTriadConfig` 만 입력으로 받는다.
- repo root discovery, `triad.toml` load, canonicalization, config validation은 `triad-config` 와 CLI entry layer의 책임이다.

## Frozen Trait

```rust
pub trait TriadApi {
    fn ingest_spec(&self) -> Result<IngestReport, TriadError>;
    fn list_claims(&self) -> Result<Vec<ClaimSummary>, TriadError>;
    fn get_claim(&self, id: &ClaimId) -> Result<ClaimBundle, TriadError>;
    fn next_claim(&self) -> Result<NextClaim, TriadError>;
    fn detect_drift(&self, id: &ClaimId) -> Result<DriftReport, TriadError>;
    fn run_claim(&self, req: RunClaimRequest) -> Result<RunClaimReport, TriadError>;
    fn verify_claim(&self, req: VerifyRequest) -> Result<VerifyReport, TriadError>;
    fn propose_patch(&self, id: &ClaimId) -> Result<ProposePatchReport, TriadError>;
    fn apply_patch(&self, id: &PatchId) -> Result<ApplyPatchReport, TriadError>;
    fn status(&self, claim: Option<&ClaimId>) -> Result<StatusReport, TriadError>;
}
```

## Frozen Requests And Reports

- `RunClaimRequest`: `claim_id`, `dry_run`, `model`, `effort`
- `RunClaimReport`: `run_id`, `claim_id`, `summary`, `changed_paths`, `suggested_test_selectors`, `blocked_actions`, `needs_patch`
- `VerifyRequest`: `claim_id`, `layers`, `full_workspace`
- `VerifyReport`: `claim_id`, `verdict`, `layers`, `full_workspace`, `evidence_ids`, `status_after_verify`, `pending_patch_id`
- `ProposePatchReport`: `patch_id`, `claim_id`, `based_on_evidence`, `path`, `reason`
- `ApplyPatchReport`: `patch_id`, `claim_id`, `applied`, `new_revision`, `followup_action`
- `StatusReport`: `summary`, `claims`

`VerifyReport.evidence_ids` 와 `ProposePatchReport.based_on_evidence` 는 raw string이 아니라 `EvidenceId` 목록이다.
`VerifyReport.full_workspace` 는 verify가 targeted selector 대신 workspace-wide command를 사용했는지 나타낸다.
`ApplyPatchReport.followup_action` 은 kebab-case `NextAction` 이다. `full_workspace_after_accept = false` 이면 `verify` 를 반환하고, 그 외에는 accept 뒤 drift 계산 결과를 그대로 따른다.

## Frozen Enums

- `NextAction`: `work`, `verify`, `accept`, `status`
- `VerifyLayer`: `unit`, `contract`, `integration`, `probe`

## Error Model

`TriadError` 는 아래 machine-stable kind를 가진다.

- `config`
- `parse`
- `io`
- `invalid-state`
- `runtime-blocked`
- `verification-failed`
- `patch-conflict`
- `serialization`

오류 분류는 기계적으로 매핑 가능해야 하며, CLI exit code 결정에 사용된다.

## Compatibility Rules

1. public type name rename은 major change다.
2. enum variant remove/rename은 breaking change다.
3. optional field addition은 non-breaking이다.
4. internal helper trait는 public contract가 아니다.
5. backend CLI의 low-level flag surface는 `triad` public Rust API에 재노출하지 않는다.

## Why Runtime Adapter Selection Is Public But Provider Abstraction Is Not

제품은 public backend set을 `codex`, `claude`, `gemini` 세 개로 고정한다.  
이 결정은 두 가지 이유로 유지한다.

- workflow contract를 흔들지 않고 core domain에 집중하기 위해
- provider matrix 테스트와 config entropy를 의도적으로 피하기 위해

즉 public surface는 runtime adapter 선택까지만 노출하고, provider routing이나 backend별 상세 CLI flag는 `triad-runtime` 내부 seam에 남긴다.
