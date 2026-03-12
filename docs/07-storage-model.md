# Storage Model

## Repository Layout

```text
repo/
├─ triad.toml
├─ spec/
│  └─ claims/
├─ schemas/
└─ .triad/
   └─ evidence.ndjson
```

## Ownership

- `spec/claims/` 는 claim 정본이다.
- `triad.toml` 은 adapter config 정본이다.
- `.triad/evidence.ndjson` 는 append-only evidence log다.

## `triad.toml`

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

## Evidence Log

- NDJSON
- 한 줄당 `Evidence` 하나
- 줄 끝 newline 유지
- 기존 row 수정/삭제 금지
