# Document Map

## Scope

- 이 문서는 현재 문서 세트의 읽기 순서와 단일 책임을 고정한다.

## Reading Order

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

## Boundary Table

| File | Single Responsibility |
|---|---|
| `00-document-map.md` | 문서 집합의 책임과 읽기 순서를 정의한다. |
| `01-product-charter.md` | 제품 정의, 목표, 비목표를 고정한다. |
| `02-domain-model.md` | `Claim`, `Evidence`, `ClaimReport` 계약을 고정한다. |
| `03-spec-format.md` | strict claim markdown 형식을 고정한다. |
| `04-workflows.md` | 현재 human CLI workflow를 설명한다. |
| `05-cli-contract.md` | CLI 명령, 출력, exit code를 고정한다. |
| `06-api-contract.md` | crate 경계와 public API를 고정한다. |
| `07-storage-model.md` | `triad.toml`, `.triad/evidence.ndjson`, `spec/claims` 저장 모델을 정의한다. |
| `08-runtime-integration.md` | core와 fs adapter의 연결 방식을 설명한다. |
| `09-verification-and-ratchet.md` | revision/freshness/status 계산 규칙을 고정한다. |
| `10-implementation-blueprint.md` | 현재 파일 트리와 crate/module 배치를 고정한다. |
| `11-consistency-report.md` | 자동 정합성 점검 결과를 기록한다. |
| `15-release-readiness.md` | 최종 릴리스 게이트를 정의한다. |

## Artifact Map

- Human-facing design docs: `docs/`
- Source code: `crates/`
- Machine-readable data schemas: `schemas/`
- Example claims: `spec/claims/`
- Consistency checker: `scripts/verify_artifacts.py`
