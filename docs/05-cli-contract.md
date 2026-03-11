# CLI Contract

## Scope

- human CLI와 agent CLI의 명령, flags, stdout/stderr 정책, exit code를 정의한다.

## Out of Scope

- 내부 repository 구현과 storage layout의 세부 파일 형식은 다루지 않는다.

## Human CLI

| Command | Purpose | Flags | Behavioral Contract |
|---|---|---|---|
| `triad init` | 프로젝트 초기화 및 기본 파일 생성 | --force | root config와 .triad 디렉터리를 만든다. |
| `triad next` | 다음으로 다룰 claim 1개 선택 | - | severity_then_id 규칙으로 단 하나의 claim만 출력하고 마지막 줄에 권장 명령 1개를 제시한다. |
| `triad work [CLAIM_ID]` | 선택된 claim 범위에서 코드/테스트 변경 | --dry-run, --model <MODEL>, --effort <low|medium|high> | configured backend(`codex`, `claude`, `gemini`)를 사용해 temp workspace에서 one-shot 실행을 수행하고, spec 직접 수정 없이 code/tests만 바꾸며 `Summary`, `Blockers`, `Next` 를 구분해 출력한다. |
| `triad verify [CLAIM_ID]` | 검증 실행 및 evidence 기록 | --with-probe, --full-workspace | 기본은 unit, contract, integration; probe는 opt-in. human 출력은 verdict/status와 `Blockers`, `Next` 를 분리한다. |
| `triad accept [PATCH_ID \| --latest]` | 대기 중 patch draft 적용 | --latest | explicit patch id를 적용하거나, `--latest` 이면 repo 전체 pending patch 중 가장 큰 `PATCH-######` id를 deterministic 하게 선택한다. human 출력은 적용 결과, blocker, follow-up command를 구분한다. |
| `triad status` | 현재 drift / pending patch / failing evidence 요약 | --claim <CLAIM_ID>, --verbose | 다음 행동 한 개를 마지막 줄에 제시한다. |

## Agent CLI

### Output Policy

- stdout: JSON only
- stderr: diagnostics/logs only
- default: compact JSON
- optional pretty print flag는 제공하지 않는다
- agent command가 실패하면 stdout은 비워 두고, 오류/진단 문자열만 stderr로 보낸다

| Command | Purpose |
|---|---|
| `triad agent claim list` | 모든 claim 요약 반환 |
| `triad agent claim get <CLAIM_ID>` | 단일 claim 상세 반환 |
| `triad agent claim next` | 다음 claim 1개 반환 |
| `triad agent drift detect --claim <CLAIM_ID>` | claim에 대한 drift 계산 |
| `triad agent run --claim <CLAIM_ID>` | configured backend를 사용한 claim-scoped one-shot 작업 실행 |
| `triad agent verify --claim <CLAIM_ID>` | 검증 실행 및 evidence 기록 |
| `triad agent patch propose --claim <CLAIM_ID>` | pending spec patch draft 생성 |
| `triad agent patch apply --patch <PATCH_ID>` | patch draft 적용 |
| `triad agent status [--claim <CLAIM_ID>]` | 프로젝트 또는 claim 상태 요약 |

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | 성공 |
| `2` | drift detected; 후속 작업 필요 |
| `3` | verification failed |
| `4` | patch approval required |
| `5` | invalid input, invalid state, runtime blocked, or patch conflict |
| `7` | internal error |

오류 분류 토큰은 `TriadError` 의 stable kind를 사용한다:
`config`, `parse`, `io`, `invalid-state`, `runtime-blocked`, `verification-failed`, `patch-conflict`, `serialization`.

성공 응답의 종료 코드는 결과 상태에도 의존한다. `healthy` 는 `0`, non-healthy drift는 기본적으로 `2`, pending patch 또는 `needs-spec` 는 `4`, verify verdict `fail` 은 `3` 으로 승격된다.

## Human Output Example

```text
$ triad next

REQ-auth-001  needs-test
Reason: code/tests changed for this claim but no fresh passing evidence exists
Suggested: triad verify REQ-auth-001

Next: triad verify REQ-auth-001
```

`triad status` 의 human 출력은 project summary line들을 먼저 보여주고, 기본 모드에서는 추천 claim 1개만 자세히 보여준다. `--verbose` 또는 `--claim` 을 쓰면 해당 범위의 claim line들을 모두 보여준다. 마지막 줄은 항상 `Next: <command>` 형태의 단일 follow-up command다.

`triad work`, `triad verify`, `triad accept` 의 human 출력도 같은 원칙을 따른다. 각 명령은 JSON envelope 대신 짧은 텍스트를 출력하고, `Summary:` 와 `Blockers:` 를 별도 line으로 구분한 뒤 마지막 줄에 항상 `Next: <command>` 형태의 단일 follow-up command를 둔다.

## Agent Output Envelope

모든 agent 응답은 공통 envelope를 따른다.

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "claim.next",
  "data": {},
  "diagnostics": []
}
```

## Agent Output Example

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "claim.next",
  "data": {
    "claim_id": "REQ-auth-001",
    "status": "needs-test",
    "reason": "code/tests changed for this claim but no fresh passing evidence exists",
    "next_action": "verify"
  },
  "diagnostics": []
}
```

## Stability Policy

- human CLI text는 UX 개선을 위해 바뀔 수 있다.
- `triad agent` namespace는 schema version이 올라가기 전까지 backward compatible contract를 유지한다.
- 새로운 field는 agent JSON에 추가 가능하지만, 기존 field의 의미 변경은 schema version bump 없이는 금지한다.
