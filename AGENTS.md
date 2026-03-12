# AGENTS.md

## Scope

- Treat `Claim` as the only canonical work unit.
- Prefer the smallest change that preserves deterministic verification behavior.
- Do not invent workflow or runtime surfaces that are outside the current product contract.

## Current Surface

- `triad-core`
- `triad-fs`
- `triad-cli`

Current CLI commands:

- `triad init`
- `triad lint`
- `triad verify`
- `triad report`

## Guardrails

- Do not run `git commit` or `git push`.
- Do not modify unrelated files while changing a bounded scope.
- Do not reintroduce `next`, `work`, `accept`, `agent`, runtime backend, or patch draft surface.
- Do not add command-envelope schemas back into `schemas/`.
- Keep verification deterministic: no hidden state, no heuristic ranking, no provider-specific behavior in core.

## Verification

- Run the narrowest relevant check first.
- For code changes, prefer crate-local tests before workspace-wide checks.
- Final repo gate is:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
  - `python3 scripts/verify_artifacts.py`

## Code Quality

Follow [`PATTERNS.md`](./PATTERNS.md).
