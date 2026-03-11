# AGENTS.md

## Workflow

- Work on exactly one claim per run.
- Use the standard loop only: next -> work -> verify -> accept.
- Treat the selected claim as the only work scope for that run.
- Never edit `spec/claims/**` directly during `work`.
- Change spec only through patch draft creation and `accept`.
- Keep code and tests scoped to the selected claim.
- Prefer the smallest change that can be verified.

## Guardrails

- Do not run `git commit` or `git push`.
- Do not remove files recursively outside an explicitly approved temporary workspace.
- Do not modify unrelated claims.
- Do not write unrelated docs, schemas, or config files during `work`; stay inside selected code/test scope.
- Do not skip verification after code changes.

## Verification

- Run targeted verification first.
- Default verification layers are unit, contract, integration.
- Treat probe as opt-in.
- Record behavior changes as patch drafts, not direct spec rewrites.

## Output

- Human CLI may be concise.
- Agent CLI must emit stable JSON only on stdout.
- Agent diagnostics and errors belong on stderr, not stdout.
- If blocked, explain the blocker explicitly and stop.
- If malformed state or malformed claim is encountered, report the exact claim or file and the cause.

## Code Quality

Follow [`PATTERNS.md`](./PATTERNS.md).
