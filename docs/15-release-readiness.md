# Release Readiness Checklist

## Scope

- 이 문서는 제품 출시 전 확인해야 할 최종 점검 항목을 정의한다.
- 구현 계획이 아니라 **출시 판정 체크리스트** 다.

## Release Gate

아래 네 묶음이 모두 통과해야 릴리스 가능하다.

1. build gate
2. contract gate
3. workflow gate
4. docs gate

## Release Contract Policy

- deterministic local evidence로 닫히는 계약은 automated gate로 판정한다.
- 사람 손으로만 확인할 수 있는 외부 현실은 최소 manual check만 유지한다.
- 현재 release-blocking manual check는 clean install path 하나다.

## 1. Build Gate

### 1.1 Workspace commands
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `python3 scripts/verify_artifacts.py`

판정:
- 네 명령 모두 성공
- warning을 예외로 남기지 않음

검증 기록:
- 2026-03-11: `cargo fmt --all --check` 성공
- 2026-03-11: `cargo clippy --workspace --all-targets --all-features -- -D warnings` 성공
- 2026-03-11: `cargo test --workspace` 성공
- 2026-03-11: `python3 scripts/verify_artifacts.py` 재실행 결과 `PASS count: 95`, `FAIL count: 0`, verdict `READY`

### 1.2 Clean install path
- clean checkout
- `cargo install --path crates/triad-cli`
- `triad --help`
- `triad agent claim next --help`

판정:
- 바이너리 설치와 기본 help 노출 성공
- README와 release checklist의 install 절차가 같은 명령을 가리킴

수동 검증 기록:
- 2026-03-11: 임시 `CARGO_HOME` + 별도 install root에서 `cargo install --path crates/triad-cli --root "$tmpdir/install"` 실행 성공
- 2026-03-11: `"$tmpdir/install/bin/triad" --help` 성공
- 2026-03-11: `"$tmpdir/install/bin/triad" agent claim next --help` 성공

## 2. Contract Gate

### 2.1 CLI contract
- human CLI 명령: `init`, `next`, `work`, `verify`, `accept`, `status`
- agent CLI 명령: `claim`, `drift`, `run`, `verify`, `patch`, `status`

판정:
- help text와 docs/05, docs/10이 일치

### 2.2 JSON schema contract
- `schemas/envelope.schema.json`
- 모든 `agent.*.schema.json`

판정:
- schema parse 성공
- CLI 출력이 schema contract test를 통과

### 2.3 Schema stability
- schema field name, required 여부, enum token이 이전에 문서화한 contract와 충돌하지 않음
- `docs/05-cli-contract.md`
- `docs/10-implementation-blueprint.md`
- `schemas/*.json`

판정:
- agent envelope의 `schema_version`, `ok`, `command`, `data`, `diagnostics` 구조가 유지된다.
- claim/drift/run/verify/patch/status schema가 문서와 같은 field 이름을 유지한다.
- breaking rename 또는 silent field removal 없이 schema contract test가 통과한다.

### 2.4 Public Rust API
- `triad-core` public type names
- `TriadApi` trait methods
- `TriadError` categories

판정:
- docs/06과 실제 코드가 일치

## 3. Workflow Gate

참고:
- 이 섹션의 핵심 동작은 automated regression evidence로 release-blocking 하게 검증한다.
- 동일 계약이 deterministic test로 이미 닫히면 별도 수동 transcript는 요구하지 않는다.

### 3.1 Happy path
순서:
1. `triad init`
2. strict claim 추가
3. `triad next`
4. `triad work <CLAIM_ID>`
5. `triad verify <CLAIM_ID>`
6. `triad accept --latest`

판정:
- claim이 healthy 상태로 닫힌다.

### 3.2 Quickstart path
- README quickstart의 `init -> next -> work -> verify -> accept` 순서를 그대로 따라간다.
- README 예시와 실제 CLI 출력 구조를 비교한다.

판정:
- 새 사용자가 README만으로 첫 루프를 재현할 수 있다.
- quickstart 명령 이름과 순서가 `docs/04`, `docs/05`, `docs/10` 과 충돌하지 않는다.

### 3.3 Failure paths
반드시 확인할 실패 경로:
- malformed claim
- stale evidence
- patch conflict
- blocked runtime
- contradicted evidence

판정:
- status와 exit code가 문서와 일치
- silent failure 없음

### 3.4 Guardrail paths
- spec direct write attempt
- `git commit` attempt
- `git push` attempt
- destructive rm attempt
- unrelated file write attempt

판정:
- runtime blocked 또는 명시적 거부
- run report에 blocker가 기록됨

### 3.5 Automated regression evidence
- 아래 automated tests는 workflow gate의 핵심 경로를 수동 체크와 별도로 계속 잠근다.
- happy path: `tests::e2e_happy_path_fixture_ratchets_single_claim_to_healthy`
- contradicted path: `tests::e2e_contradicted_fixture_marks_single_claim_as_contradicted`
- blocked runtime / guardrail path: `tests::e2e_blocked_fixture_reports_live_guardrail_violation_as_runtime_blocked`
- stale evidence path: `tests::e2e_stale_evidence_fixture_demotes_verified_claim_to_needs_test`
- patch conflict path: `tests::patch_golden_conflict_reports_exact_message_for_repo_fixture`
- malformed claim path: `tests::parser_golden_reports_exact_error_for_invalid_fixture`
- agent stdout/stderr discipline: `tests::stdout_stderr_discipline_keeps_agent_json_on_stdout_only`, `tests::stdout_stderr_discipline_routes_agent_errors_to_stderr_only`
- 위 automated coverage가 3.1~3.4의 release-blocking evidence다.
- manual replay는 debugging 또는 demo 목적일 때만 선택적으로 수행한다.

## 4. Docs Gate

### 4.1 Document map
- `docs/00-document-map.md` 가 최신 문서 집합을 가리킨다.

### 4.2 Current contract docs parity
- `docs/04-workflows.md`
- `docs/05-cli-contract.md`
- `docs/06-api-contract.md`
- `docs/08-runtime-integration.md`
- `docs/10-implementation-blueprint.md`
- `README.md`

판정:
- 현재 구현 상태와 충돌하는 명칭/경로/명령이 없다.
- release checklist가 참조하는 install, quickstart, failure-path 용어가 다른 문서와 같은 이름을 쓴다.

### 4.3 README quickstart
- 설치
- init
- next
- work
- verify
- accept
- agent JSON example

판정:
- 새 사용자가 README만 보고 기본 루프를 수행할 수 있다.
- release checklist의 quickstart path와 README가 같은 순서와 명령을 가리킨다.

## Release Decision

릴리스 가능 판정은 아래 두 문장이 동시에 참일 때만 내린다.

1. `triad` 는 local-first claim/evidence ratchet loop를 끝까지 닫는다.
2. 문서, 코드, schema, CLI, 테스트가 동일한 계약을 말한다.

둘 중 하나라도 거짓이면 릴리스를 미룬다.

## Post-Tag Audit

태그 직후에는 release 준비 단계에서 사용한 같은 consistency gate를 한 번 더 실행한다.

- command: `python3 scripts/verify_artifacts.py`
- source of truth: [`docs/11-consistency-report.md`](./11-consistency-report.md)

판정:
- report summary가 `FAIL count: 0` 이어야 한다.
- report verdict가 `READY` 를 유지해야 한다.
- README release snapshot, release checklist, consistency report 사이에 충돌하는 명령/경로/이름이 없어야 한다.

수동 검증 기록:
- 2026-03-11: post-tag audit 기준 `python3 scripts/verify_artifacts.py` 재실행 결과 `PASS count: 95`, `FAIL count: 0`, verdict `READY`
