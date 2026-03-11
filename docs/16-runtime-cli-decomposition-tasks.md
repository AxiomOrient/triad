# Runtime/CLI Decomposition Tasks

## Task Rows

| TASK_ID | ACTION | DONE_WHEN | EVIDENCE_REQUIRED | DEPENDS_ON |
| --- | --- | --- | --- | --- |
| DEC-01 | `triad-runtime`, `triad-cli`, `triad-config`의 `#[cfg(test)]` 블록을 별도 `tests.rs` 또는 `tests/`로 이동한다. | 세 파일의 production 영역이 테스트 없이 읽히고, helper import만으로 테스트가 재구성된다. | `cargo test -p triad-config`, `cargo test -p triad-cli`, `cargo test -p triad-runtime` 통과 | - |
| DEC-02 | `triad-cli`에서 human renderer를 `human_output.rs`로 분리한다. | `main.rs`에서 `mod human`가 제거되고 출력 문자열 테스트가 유지된다. | `cargo test -p triad-cli` 통과 | DEC-01 |
| DEC-03 | `triad-cli`에서 agent envelope/write, dispatch, exit code, parse helper를 각각 분리한다. | `main.rs`가 entrypoint/bootstrap 위주로 축소되고 command routing 테스트가 유지된다. | `cargo test -p triad-cli` 통과 | DEC-02 |
| DEC-04 | `triad-runtime`에서 config bridge와 work contract 조립 로직을 분리한다. | `run_profile/session_config/work_prompt/work_guardrails` 계열 함수가 전용 모듈에 존재하고 `LocalTriad` public API는 유지된다. | `cargo test -p triad-runtime` 통과 | DEC-01 |
| DEC-05 | `triad-runtime`에서 storage 계열을 분리한다. | evidence/run/patch persistence 함수가 별도 모듈로 이동하고 경로/IO 테스트가 유지된다. | `cargo test -p triad-runtime` 통과 | DEC-04 |
| DEC-06 | `triad-runtime`에서 verify/patch/drift/claim parsing을 단계적으로 분리한다. | `lib.rs`가 facade + module wiring 중심이 되고 verify/claim/patch 로직이 분리된다. | `cargo test -p triad-runtime` 통과 | DEC-05 |
| DEC-07 | `triad-config` production split 필요성을 재평가한다. | 변경축이 분리되지 않으면 구조 유지 결정을 문서화하고 종료한다. 분리 근거가 생기면 후속 claim으로 넘긴다. | 변경 이력 검토 + `cargo test -p triad-config` 통과 | DEC-01 |
| DEC-08 | 리팩터링 전용 claim을 추가하거나 기존 claim 체계에 매핑한다. | 실제 코드 변경이 저장소 workflow를 위반하지 않는 상태가 된다. | 선택 claim과 범위 정의 확인 | - |

## Execution Notes

- 가장 먼저 할 일은 `DEC-08`이 아니라 `DEC-01` 준비다. 다만 실제 code-edit run 시작 전에는 `DEC-08`이 선행돼야 한다.
- `DEC-07`은 기본적으로 keep 결정이 목표다. 새 책임이 보이지 않으면 쪼개지 않는다.
- runtime 분해는 한 번에 끝내지 말고 `DEC-04`부터 `DEC-06`까지 순차 적용한다.

## Decision Gates

| GATE_NAME | CHECK | PASS_CONDITION | ON_FAIL |
| --- | --- | --- | --- |
| G1-tests-first | 테스트 이동만으로 탐색 비용이 줄어드는가 | production file line count가 크게 줄고 테스트 전부 통과 | helper visibility 재정리 후 재시도 |
| G2-cli-thin-main | `main.rs`가 entrypoint 역할로 수렴하는가 | renderer/dispatch/exit policy가 별도 파일에 정착 | renderer 분리를 보류하고 dispatch만 우선 분리 |
| G3-runtime-facade | `LocalTriad` 외부 표면을 유지하는가 | `TriadApi` 구현과 public method 시그니처 변화 없음 | `claims`/`storage`만 우선 분리 |
| G4-config-keep | config split이 실질 이득을 주는가 | 아니오. production 유지 | validation 변경축이 커질 때 후속 claim 생성 |
