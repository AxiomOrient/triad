# Runtime/CLI Decomposition Plan

## Goal

`crates/triad-runtime/src/lib.rs`, `crates/triad-cli/src/main.rs`, and `crates/triad-config/src/lib.rs`의 거대 파일 문제를 읽기 비용과 변경축 기준으로 줄인다. 동작 변경 없이 구조를 분리하고, `triad-config`는 설계 응집도가 유지되는 한 현재 프로덕션 구조를 보존한다.

## Scope

- In scope
  - `crates/triad-runtime/src/lib.rs`
  - `crates/triad-cli/src/main.rs`
  - `crates/triad-config/src/lib.rs`
  - 위 파일들의 동작 보존형 모듈 분리
  - 테스트 모듈 이동
- Out of scope
  - claim/spec 의미 변경
  - CLI contract 변경
  - runtime API surface 변경
  - backend behavior 변경

## Evidence Summary

- `crates/triad-runtime/src/lib.rs`는 9,760줄이며 테스트가 6,150줄로 63.0%를 차지한다.
- `crates/triad-cli/src/main.rs`는 2,761줄이며 테스트가 1,861줄로 67.4%를 차지한다.
- `crates/triad-config/src/lib.rs`는 918줄이며 테스트가 513줄로 55.9%를 차지한다.
- runtime은 이미 `agent_runtime/` 하위 모듈로 backend/session/workspace staging을 분리해 두었지만, 그 외 영역은 여전히 `lib.rs` 하나에 남아 있다.
- `triad-cli`는 clap 정의가 이미 `cli.rs`로 분리되어 있는데, `main.rs`에는 human renderer, agent JSON envelope, runtime bootstrap, command dispatch, exit code policy, tests가 함께 들어 있다.
- `triad-config`의 프로덕션 영역은 약 405줄이며 데이터 구조, canonicalization, validation이 한 도메인 안에 묶여 있다. 큰 파일이지만 현재는 “거대 오브젝트”보다 “테스트 동거” 성격이 더 강하다.

## Complexity Inventory

### Essential

- `LocalTriad` facade가 외부 API 진입점 역할을 수행하는 것
- CLI human/agent 출력 정책과 exit code 정책의 존재
- config canonicalization과 validation이 repo-root/path invariant를 강제하는 것

### Accidental

- 테스트가 프로덕션 파일을 지배해서 탐색 비용을 키우는 것
- runtime `lib.rs`가 storage, claim parsing, verify orchestration, patch apply, config bridge까지 동시에 포함하는 것
- CLI `main.rs`가 entrypoint 역할을 넘어서 렌더링 정책과 dispatch 세부 구현까지 품는 것

## Recommendation

1. `triad-runtime`와 `triad-cli`는 분해가 필요하다.
2. 첫 단계는 테스트 분리다. 가장 낮은 리스크로 파일 크기와 읽기 비용을 줄인다.
3. 두 번째 단계에서 production code를 변경축 기준으로 나눈다.
4. `triad-config`는 지금 당장 production 분해를 하지 않는다.
5. `triad-config`는 테스트만 분리하고, production split은 새 책임이 생길 때 재평가한다.

## Target Module Shape

### `triad-runtime`

- `lib.rs`
  - `LocalTriad` 공개 facade
  - 외부 공개 re-export만 유지
- `runtime_config.rs`
  - `run_profile_from_triad`
  - `session_config_from_triad`
  - `sandbox_policy_from_triad`
- `work_contract.rs`
  - prompt/schema/guardrail/session attachment 구성
- `storage.rs`
  - evidence/run/patch read-write
- `verify.rs`
  - verify planning/execution/evidence append
- `patching.rs`
  - deterministic mismatch, claim proposal, patch apply
- `claims.rs`
  - claim discovery, parse, canonical lines/diff
- `drift.rs`
  - drift compute, summaries, status helpers
- `tests/`
  - helper/fake runner/test fixture 분리

### `triad-cli`

- `main.rs`
  - `main`
  - top-level `execute_cli`
- `human_output.rs`
  - 현재 `mod human`
- `agent_output.rs`
  - `AgentEnvelope`, `AgentDiagnostic`, `write_agent_envelope`
- `dispatch.rs`
  - human/agent command dispatch
- `exit_codes.rs`
  - `CliExit` 및 status-to-exit mapping
- `parse.rs`
  - claim/patch id parsing, verify request resolution
- `tests/`
  - dispatch/renderer/agent envelope 테스트

### `triad-config`

- 유지
  - `TriadConfig`, canonicalization, validation은 같은 파일에 유지
- 선택
  - `tests.rs` 또는 `tests/`로 테스트만 이동

## Critical Path

1. 테스트 분리로 대형 파일 압축
2. CLI production 분리
3. runtime production 분리
4. config 재평가

## Decision Gates

### Gate 1: Tests-First Split

- Check: 테스트만 별도 파일로 뺐을 때 public API와 import 경계가 자연스러운가
- Pass condition: 코드 이동만으로 테스트가 전부 통과하고 public visibility 추가가 최소화된다
- On fail: production split 전에 helper visibility 전략을 다시 설계한다

### Gate 2: CLI Thin Entrypoint

- Check: `main.rs`가 entrypoint + bootstrap 수준으로 줄어드는가
- Pass condition: renderer, dispatch, exit policy가 독립 파일로 이동해도 테스트와 호출 경로가 단순해진다
- On fail: dispatch와 parse를 먼저 나누고 renderer는 유지한다

### Gate 3: Runtime Stable Facade

- Check: `LocalTriad` 외부 API를 보존한 채 내부 module split이 가능한가
- Pass condition: `TriadApi for LocalTriad` 구현 시그니처와 crate public API가 그대로 유지된다
- On fail: `claims.rs`와 `storage.rs`만 먼저 분리하고 orchestration은 다음 단계로 미룬다

### Gate 4: Config Keep-or-Split

- Check: `triad-config` production code가 실제로 서로 다른 변경축을 가지는가
- Pass condition: 아니다. 현 구조 유지
- On fail: path canonicalization/validation이 별도 책임으로 자주 독립 변경될 때만 분리한다

## Constraints

- 동작 변경 금지
- claim/spec 파일 직접 수정 금지
- 문서화된 CLI/agent contract 유지
- 검증은 crate 단위 targeted test를 우선 사용
- 현재 저장소 workflow상 실제 코드 변경 실행 전에 이 리팩터링을 담당하는 claim이 필요하다

## Done Condition

- runtime와 cli에 대해 모듈 분리 순서와 경계가 명확하다
- config에 대해 “유지” 판단 기준이 명시돼 있다
- 각 단계마다 검증 게이트가 있다
- claim 부재가 실행 전 blocker임을 문서에 남겼다
