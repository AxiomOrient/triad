# triad

`triad`는 스펙, 코드, 테스트가 서로 어긋나는 문제를 줄이기 위한 로컬 CLI입니다.

AI가 코드를 바꾸는 일은 쉽습니다. 어려운 것은 "무엇이 바뀌었는지", "그 변경이 검증되었는지", "이제 스펙도 바꿔야 하는지"를 끝까지 추적하는 일입니다.  
`triad`는 그 추적 과정을 작고 단순한 루프로 고정합니다.

> 모델은 제안하고, 엔진은 검증하고, 인간은 승인한다.

## 언어

- 한국어: 현재 문서
- [English](./docs/i18n/en/README.md)
- [Español](./docs/i18n/es/README.md)
- [中文](./docs/i18n/zh/README.md)

## 이 프로젝트는 무엇인가

`triad`는 "프롬프트에서 바로 코드를 만든다"는 도구가 아닙니다.  
대신 아래 질문에 끝까지 답하게 만드는 도구입니다.

- 지금 무엇을 구현하려는가
- 그 변경이 실제로 검증되었는가
- 검증 결과가 현재 스펙과 맞는가
- 스펙을 바꿔야 한다면 누가 승인하는가

그래서 `triad`의 중심은 코드 생성이 아니라 **작은 요구사항 단위로 작업하고, 검증 결과를 남기고, 스펙 변경을 통제하는 것**입니다.

## 왜 필요한가

실제 개발에서는 자주 이런 일이 생깁니다.

- 문서는 먼저 쓰였지만 코드가 다르게 구현된다.
- 테스트는 통과하지만 문서가 예전 상태로 남는다.
- AI가 수정한 코드가 많아질수록 "정말 맞는 변경인가?"를 다시 확인하기 어려워진다.

`triad`는 이 문제를 크게 풀지 않습니다. 대신 아주 좁게 풉니다.

- 한 번에 하나의 작은 요구사항만 다룬다.
- 검증 결과를 기록으로 남긴다.
- 스펙은 바로 덮어쓰지 않고 승인 가능한 초안으로만 바꾼다.

## 어떻게 동작하나

기본 루프는 하나뿐입니다.

```text
next -> work -> verify -> accept
```

각 단계는 아래 의미를 가집니다.

1. `next`
   - 지금 가장 먼저 다뤄야 할 요구사항 하나를 고릅니다.
2. `work`
   - 그 요구사항 범위 안에서만 코드와 테스트를 수정합니다.
3. `verify`
   - 검증을 실행하고 결과를 기록합니다.
4. `accept`
   - 검증 결과 때문에 스펙을 바꿔야 하면, 그 변경 초안을 사람이 승인해 반영합니다.

이 구조 덕분에 `triad`는 "AI가 뭘 했는지 모르겠다"는 상태를 피하려고 합니다.

## 핵심 개념

전문 용어는 최소로 쓰지만, 이 네 단어만 알면 전체 흐름을 이해할 수 있습니다.

- `claim`
  - 아주 작은 요구사항 1개입니다.
- `evidence`
  - 검증 결과 기록입니다.
- `drift`
  - 스펙과 실제 상태가 얼마나 어긋났는지에 대한 현재 판단입니다.
- `patch draft`
  - 스펙을 바로 고치지 않고 먼저 보여주는 변경 초안입니다.

## 무엇을 보장하나

- 한 번의 `work`는 한 claim만 다룹니다.
- `work` 중에는 `spec/claims/**`를 직접 수정하지 않습니다.
- 검증 결과는 `.triad/evidence.ndjson`에 append-only로 쌓입니다.
- agent CLI는 stdout에 JSON만 내보내고, 진단 메시지는 stderr로 분리합니다.
- `work` backend는 `codex`, `claude`, `gemini` 중 하나를 사용하지만, 결과 계약은 같은 형태로 맞춥니다.

## 빠르게 써보기

이 저장소에는 예시 claim 두 개가 이미 들어 있습니다.

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- next
cargo run -p triad-cli -- work REQ-auth-001
cargo run -p triad-cli -- verify REQ-auth-001
cargo run -p triad-cli -- status --claim REQ-auth-001
```

검증 결과 때문에 스펙 변경 초안이 생겼다면 아래처럼 명시적으로 적용합니다.

```bash
cargo run -p triad-cli -- accept --latest
```

## 프로젝트 구조 한눈에 보기

```text
triad/
├─ Cargo.toml
├─ triad.toml
├─ AGENTS.md
├─ docs/
├─ schemas/
├─ scripts/
├─ spec/
│  └─ claims/
└─ crates/
   ├─ triad-core/
   ├─ triad-config/
   ├─ triad-runtime/
   └─ triad-cli/
```

이 구조에서 중요한 포인트는 간단합니다.

- `docs/`
  - 사람이 읽는 설명과 계약 문서
- `schemas/`
  - 기계가 읽는 agent JSON 출력 규칙
- `scripts/`
  - 문서와 계약이 깨지지 않았는지 확인하는 최소 스크립트
- `spec/claims/`
  - 작업 단위를 정의하는 작은 요구사항 파일
- `crates/`
  - 실제 Rust 구현

## 사람용 명령과 agent용 명령

사람이 주로 쓰는 명령:

- `triad init`
- `triad next`
- `triad work [CLAIM_ID]`
- `triad verify [CLAIM_ID]`
- `triad accept [PATCH_ID | --latest]`
- `triad status`

자동 호출자가 쓰는 명령:

- `triad agent claim list|get|next`
- `triad agent drift detect --claim <CLAIM_ID>`
- `triad agent run --claim <CLAIM_ID>`
- `triad agent verify --claim <CLAIM_ID>`
- `triad agent patch propose --claim <CLAIM_ID>`
- `triad agent patch apply --patch <PATCH_ID>`
- `triad agent status [--claim <CLAIM_ID>]`

## 릴리스 기준

현재 프로젝트는 automated-first release contract를 사용합니다.

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace`
- `python3 scripts/verify_artifacts.py`

사람이 꼭 다시 확인해야 하는 것은 clean install path처럼 **자동 테스트만으로는 닫히지 않는 외부 경계**뿐입니다. 자세한 기준은 [docs/15-release-readiness.md](./docs/15-release-readiness.md)에 있습니다.

## 더 읽을 문서

- [docs/00-document-map.md](./docs/00-document-map.md): 전체 문서 지도
- [docs/04-workflows.md](./docs/04-workflows.md): 표준 작업 흐름
- [docs/05-cli-contract.md](./docs/05-cli-contract.md): CLI 계약과 출력 규칙
- [docs/08-runtime-integration.md](./docs/08-runtime-integration.md): runtime과 guardrail
- [docs/10-implementation-blueprint.md](./docs/10-implementation-blueprint.md): 고정된 구현 구조
- [docs/11-consistency-report.md](./docs/11-consistency-report.md): 현재 정합성 결과
- [docs/15-release-readiness.md](./docs/15-release-readiness.md): 출시 체크리스트
