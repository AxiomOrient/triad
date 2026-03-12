# CLI Contract

## Commands

| Command | Purpose |
|---|---|
| `triad init` | minimal scaffold 생성 |
| `triad lint [--claim <CLAIM_ID> \| --all] [--json]` | claim/config contract 점검 |
| `triad verify --claim <CLAIM_ID> [--json]` | verify command 실행, evidence append, report 출력 |
| `triad report [--claim <CLAIM_ID> \| --all] [--json]` | evidence + snapshot 기준 report 계산 |

## Output

- human output과 machine output은 같은 subcommand를 쓴다.
- `--json`이면 direct JSON object/array를 출력한다.
- envelope는 없다.

## Exit Codes

| Code | Meaning |
|---|---|
| `0` | 성공 또는 actionable failure 없음 |
| `2` | `contradicted` 또는 `blocked` report 발생 |
| `5` | invalid input / invalid state / parse or config error |
| `7` | internal error |

## Deliberately Removed

- `next`
- `work`
- `accept`
- `status`
- `agent` namespace
