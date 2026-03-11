# Runtime Integration

## Scope

- `triad-runtime` 이 표준 one-shot backend(`codex`, `claude`, `gemini`)를 어떻게 실행하는지 정의한다.

## Out of Scope

- domain 자체의 옳고 그름이나 claim grammar는 다시 정의하지 않는다.

## Chosen Integration Surface

제품의 표준 `work` backend는 아래 세 개다.

- `codex`
- `claude`
- `gemini`

선택한 기본 경로는 **temp workspace에서 실행되는 one-shot process path** 다.

adapter가 담당하는 일:
- command shape 결정
- stdout 또는 output file normalization

host가 담당하는 일:
- prompt envelope 생성
- temp workspace staging
- diff 계산과 copy-back
- `blocked_actions`, `changed_paths`, staged diff에 대한 fail-closed guardrail
- run record persistence

## Why This Shape

`triad` 가 `work` 단계에서 실제로 필요한 것은 "한 claim을 한 번 안전하게 실행하고, 결과를 같은 계약으로 회수하는 것" 이다.

그래서 public surface는 provider routing이 아니라 runtime 선택만 노출한다.

- `backend` 는 `codex`, `claude`, `gemini` 중 하나다.
- provider abstraction이나 long-lived session lifecycle은 표준 경로에 넣지 않는다.
- backend마다 CLI와 출력 형식이 달라도, host가 같은 run contract와 guardrail을 유지한다.

이 구조의 핵심 효과는 backend를 바꿔도 `next -> work -> verify -> accept` 루프와 evidence ratchet 규칙은 바뀌지 않는다는 점이다.

## Config Mapping

`triad-runtime` 은 validated `CanonicalTriadConfig` 에서 local runtime defaults를 직접 만든다.

- `agent.model` -> selected backend model
- `agent.effort` -> `ReasoningEffort`
- `agent.approval_policy` -> `ApprovalPolicy`
- `agent.timeout_seconds` -> `RunProfile.timeout`
- `agent.sandbox_policy = "read-only"` -> `SandboxPreset::ReadOnly`
- `agent.sandbox_policy = "workspace-write"` -> `SandboxPreset::WorkspaceWrite { writable_roots: [repo_root], network_access: false }`
- `agent.sandbox_policy = "danger-full-access"` -> `SandboxPreset::DangerFullAccess`

unknown enum-like string이나 `timeout_seconds = 0` 은 adapter 실행 전 `config error` 로 거부한다.
표준 one-shot backend는 `approval_policy = "never"` 만 지원하고, `danger-full-access` 는 adapter 단계에서 `config error` 로 막는다.

## One-Shot Lifecycle

1. selected claim 존재 확인
2. temp workspace snapshot 생성
3. selected backend command 준비
4. backend one-shot 실행
5. normalized JSON result 수집
6. staged diff 계산
7. guardrail 통과 시 allowed change만 real repo로 copy-back
8. run record 저장

한 `triad work` 명령은 **하나의 claim, 하나의 backend invocation** 을 사용한다.

## Prompt Envelope

`triad-runtime` 은 free-form prompt를 허용하지 않는다. prompt는 항상 다음 블록으로 구성된다.

1. system rules
2. project `AGENTS.md`
3. selected claim
4. explicit forbidden actions
5. required output contract

현재 구현에서는 attachment 범위를 의도적으로 좁힌다.

- attachment 1: repo root의 `AGENTS.md`
- attachment 2: 선택된 claim 한 개의 `spec/claims/<CLAIM_ID>.md`
- output schema source: `schemas/agent.run.schema.json`

다른 claim, 다른 docs, 전체 spec tree는 prompt envelope에 자동 첨부되지 않는다.

## Required Guardrails

backend 자체를 신뢰하지 않고, live `run_claim` 경로에서 아래를 fail-closed 로 금지한다.

- spec file write (`spec/claims/**`)
- `git commit`
- `git push`
- destructive recursive remove outside temporary workspace
- claim 범위를 벗어난 unrelated file write

`triad-runtime` 은 별도의 fail-closed guardrail policy를 만들고, backend 실행 전 selected claim 존재를 확인한 뒤 reported `blocked_actions`, `changed_paths`, staged diff를 모두 `runtime blocked` 로 재검사한다.

guardrail policy의 현재 판정 규칙:
- `git commit` -> `runtime blocked`
- `git push` -> `runtime blocked`
- `spec/claims/**` write -> `runtime blocked`
- temporary workspace 밖 recursive remove -> `runtime blocked`
- live allow roots (`src/`, `tests/`, `crates/`, `.triad/tmp`) 밖 write/remove -> `runtime blocked`

derived artifact policy:
- `target/**` 는 staged diff와 copy-back에서 무시한다.
- root에 원래 없던 새 `Cargo.lock` 은 derived artifact로 간주해 무시한다.
- 기존 `Cargo.lock` 변경은 여전히 reported diff와 guardrail 검사를 통과해야 한다.

## Backend Mapping

### `codex`
- command: `codex exec`
- input: stdin prompt
- output capture: `--output-last-message`
- native schema: `--output-schema`

### `claude`
- command: `claude -p`
- input: prompt arg
- output capture: stdout JSON
- native schema: `--json-schema`
- host behavior: `structured_output`, prose+fenced JSON, inner payload를 모두 strict JSON object로 normalize 한 뒤 triad run envelope로 맞춘다.

### `gemini`
- command: `gemini -p`
- input: prompt arg
- output capture: stdout JSON wrapper
- native schema: 없음
- host behavior: wrapper의 `response` string을 추출한 뒤 fenced/raw JSON 또는 partial inner payload를 strict parse하고 triad run envelope로 normalize 한다.

## Structured Output

final normalized payload는 반드시 `schemas/agent.run.schema.json` 과 같은 의미의 JSON object여야 한다.

필수 필드:
- `claim_id`
- `summary`
- `changed_paths`
- `suggested_test_selectors`
- `blocked_actions`
- `needs_patch`

## Omitted On Purpose

- long-lived session lifecycle
- provider abstraction
- background daemon
- multi-agent orchestration
- historical planning detail
