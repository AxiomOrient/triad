# Workflows

현재 public workflow는 아래 네 명령뿐이다.

## 1. `triad init`

- minimal scaffold를 만든다.
- `triad.toml`
- `spec/claims/`
- `.triad/evidence.ndjson`

## 2. `triad lint`

- claim markdown과 config가 현재 contract를 만족하는지 확인한다.
- `--json`이면 lint report object를 직접 출력한다.

## 3. `triad verify`

- configured verify commands를 실행한다.
- evidence를 append한다.
- current snapshot 기준 claim report를 다시 계산한다.

## 4. `triad report`

- command 실행 없이 저장된 evidence와 current snapshot으로 report만 계산한다.
- `--claim` 또는 `--all`을 지원한다.

## Out Of Scope

- `next`
- `work`
- `accept`
- `agent ...`
- runtime backend orchestration
