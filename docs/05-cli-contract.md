# CLI Contract

## Commands

| Command | Purpose |
|---|---|
| `triad init` | minimal scaffold 생성 |
| `triad lint [--claim <CLAIM_ID> \| --all] [--json]` | claim/config contract 점검 |
| `triad verify --claim <CLAIM_ID> [--json]` | verify command 실행, evidence append, report 출력 |
| `triad report [--claim <CLAIM_ID> \| --all] [--json]` | evidence + snapshot 기준 report 계산 |

## Output And Exit Codes

- human output과 machine output은 같은 subcommand를 쓴다.
- `--json`이면 direct JSON object/array를 출력한다.
- envelope는 없다.

| Code | Meaning |
|---|---|
| `0` | 성공 또는 actionable failure 없음 |
| `2` | `contradicted` 또는 `blocked` report 발생 |
| `5` | invalid input / invalid state / parse or config error |
| `7` | internal error |

## Repository Contract

```text
repo/
├─ triad.toml
├─ spec/claims/
├─ schemas/
└─ .triad/evidence.ndjson
```

`triad.toml`의 최소 shape는 아래다.

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

## Verification Gate

최종 게이트는 아래 네 개다.

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `python3 scripts/verify_artifacts.py`

`scripts/verify_artifacts.py` 는 docs/schema/crate surface, CLI contract, semantic fixture를 같이 확인한다.
