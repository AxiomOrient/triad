# Workflows

## Scope

- human loop와 agent loop, 상태 전이, 실패 처리, next 선택 규칙을 정의한다.

## Out of Scope

- JSON schema field-level details는 다루지 않는다.

## Standard Human Workflow

### 1. `triad next`
- 다음 claim 한 개를 결정한다.
- 출력은 claim id, status, 이유, 권장 다음 명령으로 제한한다.

### 2. `triad work [CLAIM_ID]`
- configured backend(`codex`, `claude`, `gemini`) 중 하나를 선택해 temp workspace에서 one-shot 실행을 시작한다.
- code/tests 변경만 허용한다.
- spec file direct edit, git commit, git push, destructive rm을 차단한다.
- backend 응답의 `blocked_actions` 와 `changed_paths`, staged diff를 모두 같은 guardrail로 다시 검사해 fail-closed 로 차단한다.
- 결과는 changed paths, suggested selectors, blocked actions를 남긴다.

### 3. `triad verify [CLAIM_ID]`
- targeted verification을 먼저 실행한다.
- 기본 layer는 `unit, contract, integration` 이다.
- 성공/실패 여부와 covered path digest를 evidence로 append한다.
- behavior change가 관측되면 pending patch draft를 갱신한다.

### 4. `triad accept [PATCH_ID | --latest]`
- explicit `PATCH_ID` 또는 repo 전체에서 sequence가 가장 큰 pending patch id를 선택하는 `--latest` 로 patch draft를 적용한다.
- 적용 뒤에는 spec revision을 갱신한다.
- 기본값으로 전체 workspace verify를 다시 수행한다.

## Standard Agent Workflow

agent는 workflow를 새로 정의하지 않는다. 아래 primitive를 호출해 같은 루프를 따른다.

1. `triad agent claim next`
2. `triad agent run --claim <CLAIM_ID>`
3. `triad agent verify --claim <CLAIM_ID>`
4. `triad agent patch apply --patch <PATCH_ID>` 또는 인간 승인 대기

## Deterministic Next Selection

`triad next` / `triad agent claim next` 는 아래 알고리즘을 사용한다.

1. 모든 claim에 대해 drift를 계산한다.
2. status priority를 적용한다.
3. 동일 priority에서는 `claim_id` lexical ascending을 적용한다.
4. actionable claim이 있으면 첫 번째 claim만 반환한다.
5. 모든 claim이 healthy이면 lexical first healthy를 fallback으로 반환한다.

pseudocode:

```text
claims = load_claims()
reports = claims.map(detect_drift)
ordered = sort_by(status_priority, claim_id)
return ordered.first()
```

## Failure Handling

### runtime blocked
- forbidden command 또는 금지된 tool intent 보고
- 금지된 changed path 보고
- staged workspace diff와 보고된 changed path 불일치

처리:
- command는 non-zero 종료
- successful run record는 남기지 않는다
- claim drift는 자동으로 `blocked` 로 재분류되지 않는다
- human은 오류 메시지로 blocker를 보고, `status` 에서는 기존 drift 상태를 본다

### work failed
- runtime approval 필요
- missing local dependency
- selected backend CLI failure 또는 malformed JSON output

처리:
- command는 non-zero 종료
- claim drift는 fresh evidence 없이 그대로 유지된다
- human은 stderr 또는 진단 메시지로 이유를 본다

### verify failed
- failing evidence 기록
- drift는 `contradicted` 또는 `needs-code`
- 다음 행동은 `work` 로 돌아간다

### patch pending
- verify는 성공했지만 spec mismatch가 발견되면 `needs-spec`
- 다음 행동은 `accept`

## Deliberate Omissions

제품은 아래를 지원하지 않는다.

- parallel claim execution
- auto-merge of multiple patch drafts
- hidden background watcher
- commit-time hard blocking as the main loop

## UX Rule

인간용 출력의 마지막 줄은 항상 다음 행동 하나만 남긴다. 예:

```text
Next: triad verify REQ-auth-001
```
