# Release Readiness

릴리스 가능 조건은 아래 네 묶음이 모두 통과할 때다.

## 1. Build Gate

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

## 2. Contract Gate

- `python3 scripts/verify_artifacts.py`
- `triad --help` 가 현재 CLI contract와 일치
- `schemas/` 가 현재 data model contract와 일치

## 3. Behavior Gate

- `cargo run -p triad-cli -- lint --all --json`
- `cargo run -p triad-cli -- verify --claim REQ-auth-001 --json`
- `cargo run -p triad-cli -- report --all --json`

## 4. DoD Gate

다음이 모두 참이어야 한다.

1. `work`, `next`, `accept`, `agent`가 public surface에 없다.
2. `triad-core`가 pure verification kernel이다.
3. `Claim`이 유일한 canonical unit이다.
4. freshness가 claim digest + artifact digests로만 판정된다.
5. runtime/provider 코드는 repo tree에서 제거되었다.
6. docs와 schema가 새 모델과 일치한다.
7. 예제 claim 2개로 `lint`, `verify`, `report`가 동작한다.
