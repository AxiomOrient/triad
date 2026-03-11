# Domain Model

## Scope

- `triad`의 핵심 엔터티와 파생 상태를 정의한다.
- 어떤 값이 정본이고 어떤 값이 파생물인지 구분한다.

## Out of Scope

- CLI 표시 형식과 JSON schema의 직렬화 레이아웃은 다루지 않는다.

## Canonical Entities

### Claim
하나의 원자적 요구사항. `spec/claims/*.md` 에 존재한다.

필드:
- `id`: immutable identifier, 예: `REQ-auth-001`
- `title`: 짧은 제목
- `statement`: 본문 claim
- `examples`: 구체적 입출력 또는 시나리오 목록
- `invariants`: 항상 유지되어야 하는 규칙
- `notes`: 선택적 메모
- `revision`: accepted spec content에서 계산된 정수 revision

### Evidence
claim에 대한 검증 결과. `.triad/evidence.ndjson` 에 append-only로 저장된다.

필드:
- `id`
- `claim_id`
- `kind`: `unit | contract | integration | probe`
- `verdict`: `pass | fail | unknown`
- `test_selector`: 선택적 테스트 셀렉터
- `command`: 실행된 검증 명령
- `covered_paths`: freshness 판단에 쓰이는 파일 경로 목록
- `covered_digests`: `covered_paths` 의 실행 시점 digest
- `spec_revision`
- `created_at`

### Patch Draft
spec direct write 대신 생성되는 제안물.

필드:
- `id`
- `claim_id`
- `based_on_evidence`
- `unified_diff`
- `rationale`
- `created_at`
- `state`: `pending | applied | superseded`

### Drift Report
claim의 현재 건강 상태를 계산한 파생 상태.

`status` 값:
- `healthy`
- `needs-code`
- `needs-test`
- `needs-spec`
- `contradicted`
- `blocked`

## Derived State Rules

### healthy
- pass evidence가 존재하고
- 해당 evidence가 stale이 아니며
- pending patch가 없고
- failing evidence가 최신 결과를 모순시키지 않는다.

### needs-code
- spec은 존재하지만 pass evidence를 만드는 구현이 아직 없다고 판단되는 상태.
- 예: `work` 이전의 새 claim, 또는 known todo marker 상태.

### needs-test
- fresh evidence는 없지만 과거 evidence에 non-empty `covered_paths` 가 관측된 상태.
- verify가 선행되어야 한다.

### needs-spec
- 검증된 behavior change가 spec에 반영되지 않은 상태.
- patch draft 생성 대상이다.

### contradicted
- current runtime에서는 latest fresh `fail` evidence를 `contradicted` 로 취급한다.

### blocked
- current runtime에서는 latest fresh `unknown` evidence를 `blocked` 로 취급한다.
- `triad work` 가 `runtime blocked` 로 실패해도 drift가 자동으로 `blocked` 로 바뀌지는 않는다. work failure surface와 drift status는 분리된다.

## Invariants

1. claim id는 생성 후 바뀌지 않는다.
2. evidence는 append-only다. 수정/삭제 대신 새 evidence를 추가한다.
3. patch draft는 evidence를 참조할 뿐 evidence를 변경하지 않는다.
4. `work` 는 code/tests 만 수정할 수 있고 spec direct write는 금지된다.
5. 한 번의 `work` 세션은 정확히 하나의 claim만 대상으로 한다.
6. `accept` 없이는 spec revision이 증가하지 않는다.
7. `next` 는 항상 정확히 하나의 claim만 반환한다.

## Claim State Machine

```text
spec only
  -> work
code/tests changed
  -> verify
evidence appended
  -> (healthy | contradicted | needs-spec)
needs-spec
  -> accept
accepted spec revision
  -> healthy
```

## Selection Priority

`triad next` 는 아래 순서로 우선순위를 고정한다.

1. `contradicted`
2. `needs-test`
3. `needs-code`
4. `needs-spec`
5. `blocked`
6. `healthy` 는 다른 actionable claim이 있을 때 기본 선택 대상에서 제외된다. 모든 claim이 healthy이면 lexical first healthy를 fallback으로 반환한다.

동일 status 내부에서는 `claim_id` 오름차순으로 고른다.  
이 규칙은 사용자 설정 대상이 아니다.

## Rust Types To Freeze

```rust
pub enum EvidenceKind { Unit, Contract, Integration, Probe }
pub enum Verdict { Pass, Fail, Unknown }
pub enum DriftStatus { Healthy, NeedsCode, NeedsTest, NeedsSpec, Contradicted, Blocked }
pub enum PatchState { Pending, Applied, Superseded }
pub enum VerifyLayer { Unit, Contract, Integration, Probe }
pub enum NextAction { Work, Verify, Accept, Status }
```
