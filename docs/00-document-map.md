# Document Map

## Scope

- 이 문서는 전체 문서 세트의 역할 분해와 읽기 순서를 정의한다.
- 각 문서의 단일 책임을 명시해 문서 간 중복과 누락을 막는다.

## Out of Scope

- 구현 세부 타입 정의의 본문은 다루지 않는다.
- 제품 철학 자체의 옳고 그름을 다시 논증하지 않는다.

## Reading Order

### Core Contracts

1. `01-product-charter.md`
2. `02-domain-model.md`
3. `03-spec-format.md`
4. `04-workflows.md`
5. `05-cli-contract.md`
6. `06-api-contract.md`
7. `07-storage-model.md`
8. `08-runtime-integration.md`
9. `09-verification-and-ratchet.md`
10. `10-implementation-blueprint.md`
11. `15-release-readiness.md`
12. `11-consistency-report.md`

## MECE Boundary Table

| File | Single Responsibility | Includes | Excludes |
|---|---|---|---|
| `00-document-map.md` | 문서 집합의 지도와 경계. 각 문서의 단일 책임, 포함 범위, 제외 범위를 정의한다. | 문서 책임과 경계, 읽기 순서 | 도메인 세부 규칙 |
| `01-product-charter.md` | 제품 목적, 설계 원칙, 목표/비목표, 성공 기준을 정의한다. | 목표, 비목표, 원칙, 성공 기준 | 파일 형식, 명령 플래그 |
| `02-domain-model.md` | claim/evidence/patch/drift 도메인 모델과 불변조건을 정의한다. | claim/evidence/patch/drift 정의, 불변조건 | CLI 문법, 파일 경로 |
| `03-spec-format.md` | machine-readable claim markdown 형식과 파싱 규칙을 정의한다. | claim markdown grammar, 파싱 규칙 | runtime 실행 방식 |
| `04-workflows.md` | human loop와 agent loop, 상태 전이, 다음 claim 선택 규칙을 정의한다. | 명령 순서, 상태 전이, 선택 규칙 | Rust crate API |
| `05-cli-contract.md` | 인간용 CLI와 agent용 CLI의 계약, exit code, 출력 정책을 정의한다. | 명령, flags, exit code, stdout policy | 내부 저장 형식 |
| `06-api-contract.md` | Rust crate 경계, 공개 API, 오류 모델, 안정성 정책을 정의한다. | public Rust API, crate boundary, errors | prompt wording |
| `07-storage-model.md` | repo layout, .triad 상태 파일, patch/run/evidence 저장 형식을 정의한다. | repo layout, .triad file semantics | claim grammar |
| `08-runtime-integration.md` | 현재 구현의 adapter-first runtime integration과 그 이유를 기록한다. | backend mapping, temp workspace, guardrail, output normalization, integration rationale | 상세 task ledger |
| `09-verification-and-ratchet.md` | 검증 레이어, 증거 생성, stale 판단, patch ratchet 규칙을 정의한다. | test layers, stale 판단, patch 생성 규칙 | CLI UI wording |
| `10-implementation-blueprint.md` | Cargo workspace, triad.toml, clap tree, Rust skeleton, schema 카탈로그를 고정한다. | workspace tree, clap tree, schema catalog, fixed file map | 상세 실행 순서와 태스크 분해 |
| `15-release-readiness.md` | 출시 가능 여부를 판정하는 최종 체크리스트를 정의한다. | build/contract/workflow/docs gate | 구현 작업 분해 |
| `11-consistency-report.md` | 자동 검증 결과를 기록한다. | 자동 점검 결과, 검증 범위, 실패/경고 여부 | 설계 변경 제안 |

## Artifact Map

- Human-facing design reference lives under `docs/`.
- Current runtime implementation contract lives at [`08-runtime-integration.md`](./08-runtime-integration.md).
- Adapter rationale and guardrail decisions are folded into [`08-runtime-integration.md`](./08-runtime-integration.md).
- Machine-readable contracts live under `schemas/`.
- Executable implementation skeleton lives under `crates/`.
- Repository defaults live at root: `Cargo.toml`, `triad.toml`, `AGENTS.md`.
- Example strict claims live under `spec/claims/`.
- Localized entry docs live under `docs/i18n/<lang>/`.

## Rule For Future Additions

새 문서를 추가할 때는 아래 세 가지를 동시에 만족해야 한다.

1. 기존 문서의 단일 책임을 침범하지 않아야 한다.
2. 새 문서가 없으면 독자가 중요한 결정을 잃어야 한다.
3. `docs/00-document-map.md` 와 `scripts/verify_artifacts.py` 의 document map coverage 또는 explicit artifact check로 orphan 상태가 감지되어야 한다.
