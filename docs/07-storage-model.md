# Storage Model

## Scope

- repo layout, `.triad/` 내부 파일, evidence/patch/run 보존 규칙을 정의한다.

## Out of Scope

- claim markdown parser 규칙과 CLI 플래그 의미는 다루지 않는다.

## Repository Layout

```text
repo/
├─ AGENTS.md
├─ Cargo.toml
├─ triad.toml
├─ docs/
├─ schemas/
├─ spec/
│  └─ claims/
├─ crates/
└─ .triad/
   ├─ evidence.ndjson
   ├─ patches/
   ├─ runs/
   └─ cache.sqlite
```

## Ownership Rules

- `spec/claims/` 는 정본이다.
- `src/` 와 `tests/` 는 구현 정본이다.
- `.triad/evidence.ndjson` 는 append-only audit log다.
- `.triad/patches/` 는 적용 전 patch draft 보관소다.
- `.triad/runs/` 는 work session 결과 보관소다.
- `.triad/cache.sqlite` 는 gitignore 대상 cache다.

## Evidence Log Format

파일: `.triad/evidence.ndjson`

- 한 줄 = evidence 하나
- UTF-8 JSON object
- immutable append-only
- append 시 compact JSON 1개를 직렬화하고 terminal newline `\n` 을 붙인다.
- 기존 파일이 non-empty인데 마지막 줄바꿈이 없으면 append를 거부한다.
- reader는 non-empty line만 top-to-bottom으로 읽고 각 line을 독립 JSON object로 parse한다.
- newest evidence is the last matching line for a given `(claim_id, kind, selector)` interpretation

예시:

```json
{"id":"EVID-000001","claim_id":"REQ-auth-001","kind":"integration","verdict":"pass","test_selector":"auth::login_success","command":"cargo test auth::login_success -- --nocapture","covered_paths":["src/auth.rs","tests/auth_login.rs"],"covered_digests":{"src/auth.rs":"sha256:...","tests/auth_login.rs":"sha256:..."},"spec_revision":3,"created_at":"2026-03-09T10:00:00+09:00"}
```

## Patch Storage

파일쌍:
- `.triad/patches/PATCH-000001.json`
- `.triad/patches/PATCH-000001.diff`

JSON meta는 patch id, claim id, evidence refs, rationale을 담고, diff 파일은 사람이 바로 review 가능한 unified diff를 담는다.
meta JSON은 `created_at` 과 `diff_path` 도 포함한다.
`diff_path` 는 repo root 기준 relative path로 기록되고, 같은 `PATCH-######` stem의 `.diff` 파일을 가리켜야 한다.
새 patch draft id는 existing `.json` / `.diff` stem의 최대 `PATCH-######` 다음 번호를 사용한다.
`patch propose` 는 claim당 pending patch가 이미 있으면 새 patch를 만들지 않는다.

## Run Storage

파일: `.triad/runs/RUN-000001.json`

포함 항목:
- run id
- claim id
- summary
- prompt fingerprint
- changed paths
- blocked actions
- suggested selectors
- needs_patch
- runtime metadata

run file은 reproducibility 보조 정보다. 정본이 아니다.

## Cleanup Policy

- accepted patch는 기본적으로 archive하지 않고 남긴다. audit trail을 우선한다.
- superseded patch는 latest가 아닌 것으로 표시하지만 삭제하지 않는다.
- cache.sqlite만 자유롭게 지워도 된다.

## Git Policy

권장:
- commit 대상: `spec/`, `src/`, `tests/`, `docs/`, `schemas/`, `.triad/evidence.ndjson`
- optional commit 대상: accepted patch meta/diff
- ignore 대상: `.triad/cache.sqlite`, `.triad/runs/`

기본 scaffold의 `.gitignore` 기본 정책:

- `.triad/evidence.ndjson` 는 남긴다.
- `.triad/patches/` 는 남긴다.
- `.triad/cache.sqlite` 는 ignore 한다.
- `.triad/runs/` 는 ignore 한다.

그 외 OS/editor/cache 잡파일 ignore는 추가할 수 있지만, 위 triad 파생 상태 정책과 충돌하면 안 된다.
