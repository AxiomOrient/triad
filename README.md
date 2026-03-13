# triad

`triad`는 Claim 하나를 현재 증거로 판정하는 headless deterministic verification kernel이다.

핵심 질문은 하나다.

> 이 Claim은 지금 참인가?

이 저장소의 현재 public surface는 그 질문에 답하기 위한 최소 구성만 남긴다.

- `triad-core`: 순수 검증 커널
- `triad-fs`: filesystem reference adapter
- `triad-cli`: `init / lint / verify / report` reference CLI

`next`, `work`, `accept`, `agent` 같은 workflow/orchestration surface는 v1 범위 밖이다.

## 빠른 시작

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

## 핵심 개념

- `Claim`
  - 원자적 계약 단위이자 원자적 검증 단위
- `Evidence`
  - append-only 검증 기록
- `ClaimReport`
  - 현재 snapshot과 evidence를 기준으로 계산한 판정 결과

`ClaimStatus` 는 아래 다섯 개만 쓴다.

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

## 현재 명령

- `triad init`
- `triad lint [--claim <CLAIM_ID> | --all] [--json]`
- `triad verify --claim <CLAIM_ID> [--json]`
- `triad report [--claim <CLAIM_ID> | --all] [--json]`

`--json` 출력은 direct JSON object/array다. envelope는 사용하지 않는다.

## 저장소 구조

```text
triad/
├─ crates/
│  ├─ triad-core/
│  ├─ triad-fs/
│  └─ triad-cli/
├─ docs/
├─ schemas/
├─ scripts/
├─ spec/
│  └─ claims/
├─ AGENTS.md
├─ Cargo.toml
├─ README.md
└─ triad.toml
```

## 검증 기준

현재 최종 게이트는 아래 명령으로 닫는다.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_artifacts.py
cargo run -p triad-cli -- lint --all --json
cargo run -p triad-cli -- verify --claim REQ-auth-001 --json
cargo run -p triad-cli -- report --all --json
```

## 더 읽을 문서

- [docs/02-domain-model.md](./docs/02-domain-model.md)
- [docs/03-spec-format.md](./docs/03-spec-format.md)
- [docs/05-cli-contract.md](./docs/05-cli-contract.md)
