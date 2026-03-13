# triad 소개

[English README](../../README.md)

`triad`는 한 가지 질문에 답하는 deterministic verification kernel이다.

> 이 Claim은 지금 참인가?

이 저장소는 표면을 의도적으로 작게 유지한다. `Claim`이 유일한 canonical work unit이고, 현재 공개 surface는 core, filesystem adapter, thin CLI뿐이다.

## 포함 범위

- `triad-core`
  순수 검증 로직
- `triad-fs`
  claims, snapshots, config, evidence log를 다루는 filesystem reference adapter
- `triad-cli`
  `init`, `lint`, `verify`, `report` 네 개만 제공하는 reference CLI

현재 범위 밖:

- `next`
- `work`
- `accept`
- `agent`
- runtime backend
- patch draft workflow surface

## 핵심 개념

- `Claim`: 검증하려는 원자적 계약
- `Evidence`: claim revision과 artifact digest에 묶인 append-only 검증 기록
- `ClaimReport`: 현재 snapshot과 evidence를 합쳐 계산한 현재 판정

`ClaimStatus`는 다섯 개만 사용한다.

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

## 빠른 시작

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

## 설정 요약

`triad.toml`은 strict v2 contract다.

```toml
version = 2

[paths]
claim_dir = "spec/claims"
evidence_file = ".triad/evidence.ndjson"

[snapshot]
include = ["src/**", "tests/**", "crates/**", "Cargo.toml", "Cargo.lock"]

[verify]
commands = ["cargo test --lib", "cargo test --tests"]
```

`verify.commands`는 string entry와 structured object entry를 모두 허용한다. structured object는 `command`, optional `locator`, optional `artifacts`를 사용한다. `triad verify --claim <CLAIM_ID>` 실행 시 `{claim_id}`와 `{claim_path}`가 선택된 claim 기준으로 확장된다.

## evidence 관련 주의점

- `Hard` evidence만 status를 바꾼다.
- freshness는 evidence row에 기록된 artifact subset에 대해서만 계산한다.
- reference shell adapter는 `0 => pass`, non-zero `=> fail`만 자동 생성한다.
- 따라서 `unknown` verdict는 shell 경로에서 자동 생성되지 않고, `blocked` report는 seeded/manual/non-shell evidence 경로가 필요하다.

## 더 읽기

- [Domain model](../../docs/02-domain-model.md)
- [Claim format](../../docs/03-spec-format.md)
- [CLI contract](../../docs/05-cli-contract.md)
