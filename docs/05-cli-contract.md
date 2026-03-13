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
include = ["src/**", "tests/**", "crates/**", "triad.toml", "Cargo.toml", "Cargo.lock"]

[verify]
commands = ["cargo test --lib", "cargo test --tests"]
```

`verify.commands`는 하위호환으로 string 또는 object entry를 허용한다.

```toml
[verify]
commands = [
  "cargo test --lib",
  { command = "cargo test -- {claim_id}", locator = "cargo-test:{claim_id}", artifacts = ["crates/triad-core/**"] }
]
```

- string entry는 legacy repo-wide snapshot capture를 유지한다.
- object entry는 claim template expansion과 evidence-local artifact subset capture를 허용한다.
- 현재 reference shell adapter는 exit code `0 => pass`, non-zero `=> fail`만 생성한다. `unknown` verdict와 그에 따른 `blocked` report는 seeded/manual/non-shell evidence 경로가 필요하다.

## Verification Gate

최종 게이트는 아래 네 개다.

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `python3 scripts/verify_artifacts.py`

`scripts/verify_artifacts.py` 는 docs/schema/crate surface와 root config contract만 확인한다.
CLI behavior와 semantic status coverage는 Rust 테스트가 담당한다.
