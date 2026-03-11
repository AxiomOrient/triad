# Verification And Ratchet

## Scope

- evidence 생성, verification layer, stale 판단, patch ratchet 규칙을 정의한다.

## Out of Scope

- CLI help text와 runtime prompt wording은 다루지 않는다.

## Verification Layers

### unit
- 순수 함수/변환/직렬화 규칙
- 외부 프로세스와 네트워크 없음

### contract
- 경계 형태, JSON shape, ownership / isolation invariant
- public boundary semantics 확인

### integration
- cross-module wiring, end-to-end 흐름
- mock runtime 또는 실제 로컬 runtime wiring

### probe
- 외부 세계와의 접촉
- OS, filesystem permission, network, live dependency 등
- 기본 파이프라인에서는 opt-in

## Standard Verify Order

1. targeted tests for the claim
2. default layers: `unit, contract, integration`
3. optional probe if `--with-probe`
4. evidence append
5. drift recalculate
6. patch draft refresh if needed

## Current Command Mapping

- targeted selector가 있으면 claim-scoped command를 먼저 계획한다.
- selector가 없거나 `--full-workspace` 이면 workspace command로 fallback 한다.
- probe는 `VerifyLayer::Probe` 가 request에 있을 때만 계획된다.
- runtime의 default verify request는 `verify.default_layers` 에서 시작하고, `--with-probe` 가 있을 때만 `probe` 를 뒤에 추가한다.
- `verify.default_layers` 자체에 `probe` 를 넣는 구성은 허용하지 않는다.

targeted command mapping:
- `unit` -> `cargo test --lib <selector>`
- `contract` -> `cargo test <selector>`
- `integration` -> `cargo test --tests <selector>`
- `probe` -> `cargo test --tests <selector> -- --ignored`

workspace fallback mapping:
- `unit` -> `cargo test --workspace --lib`
- `contract` -> `cargo test --workspace`
- `integration` -> `cargo test --workspace --tests`
- `probe` -> `cargo test --workspace --tests -- --ignored`

## Evidence Creation Rule

evidence는 verify 단계에서만 생성된다.  
`work` 는 run record를 남길 수 있지만 evidence를 직접 쓰지 않는다.

이 규칙의 목적:
- 코드 변경과 검증 결과를 분리
- evidence log의 의미를 단순화
- "코드가 바뀌었음" 과 "검증되었음" 을 혼동하지 않기

## Freshness Rule

evidence는 `covered_paths` 와 해당 file digest를 가진다.  
최신 working tree에서 같은 path의 digest를 다시 계산해 비교한다.

digest 계산 규칙:
- 각 `covered_path` 는 repo root 기준 relative path여야 한다.
- `..` 로 repo root 밖으로 나가는 path는 거부한다.
- digest는 file raw bytes의 SHA-256을 계산하고 `sha256:<lowercase-hex>` 로 기록한다.

- 모두 동일 -> fresh
- 하나라도 다름 -> stale
- path가 삭제됨 -> stale

freshness는 global git revision 대신 **claim-scoped covered path digest** 를 기준으로 사용한다.

## Drift Mapping

### fresh passing evidence exists
- no pending patch -> `healthy`
- pending patch -> `needs-spec`

### latest relevant evidence is failing
- current runtime의 latest fresh `fail` -> `contradicted`
- current runtime의 latest fresh `unknown` -> `blocked`

### no fresh evidence
- 과거 evidence에 non-empty `covered_paths` 가 관측됨 -> `needs-test`
- spec only and no implementation path observed -> `needs-code`

## Patch Proposal Rule

patch draft는 아래 조건이 모두 참일 때만 생성한다.

1. fresh evidence가 존재한다.
2. behavior change가 claim text/examples/invariants와 다르다.
3. mismatch가 deterministic하게 설명 가능하다.
4. spec direct write는 아직 일어나지 않았다.

현재 runtime의 deterministic mismatch detector는 아래 신호가 동시에 있을 때만 patch 후보로 승격한다.

- latest fresh relevant evidence가 `pass` 이다.
- latest run record가 같은 claim에 대해 `needs_patch = true` 를 기록했다.
- run summary가 비어 있지 않다.
- run `changed_paths` 와 fresh pass evidence `covered_paths` 가 겹친다.

위 조건을 만족하지 않으면 patch 후보를 만들지 않는다.

patch draft는 현재 claim file을 완전히 재작성하지 않고, **최소 unified diff** 를 생성한다.

## Accept Rule

`accept` 는 아래를 순서대로 수행한다.

1. patch draft 표시
2. conflict 검사
3. diff apply
4. spec revision 갱신
5. optional full workspace verify
6. patch state를 applied로 기록

## Anti-Patterns Explicitly Rejected

- test 없이 spec patch만 먼저 적용
- failing evidence를 지우거나 덮어쓰기
- 전체 spec section rewrite를 기본 전략으로 사용
- unrelated claim을 한 run에서 함께 수정
