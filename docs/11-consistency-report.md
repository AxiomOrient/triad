# Consistency Report

## Summary

- PASS count: 95
- FAIL count: 0

## Checks

- PASS: document exists: docs/00-document-map.md
- PASS: document exists: docs/01-product-charter.md
- PASS: document exists: docs/02-domain-model.md
- PASS: document exists: docs/03-spec-format.md
- PASS: document exists: docs/04-workflows.md
- PASS: document exists: docs/05-cli-contract.md
- PASS: document exists: docs/06-api-contract.md
- PASS: document exists: docs/07-storage-model.md
- PASS: document exists: docs/08-runtime-integration.md
- PASS: document exists: docs/09-verification-and-ratchet.md
- PASS: document exists: docs/10-implementation-blueprint.md
- PASS: document exists: docs/15-release-readiness.md
- PASS: schema parses as JSON: schemas/envelope.schema.json
- PASS: schema parses as JSON: schemas/agent.claim.list.schema.json
- PASS: schema parses as JSON: schemas/agent.claim.get.schema.json
- PASS: schema parses as JSON: schemas/agent.claim.next.schema.json
- PASS: schema parses as JSON: schemas/agent.drift.detect.schema.json
- PASS: schema parses as JSON: schemas/agent.run.schema.json
- PASS: schema parses as JSON: schemas/agent.verify.schema.json
- PASS: schema parses as JSON: schemas/agent.patch.propose.schema.json
- PASS: schema parses as JSON: schemas/agent.patch.apply.schema.json
- PASS: schema parses as JSON: schemas/agent.status.schema.json
- PASS: root Cargo.toml parses and all workspace members exist
- PASS: triad.toml parses
- PASS: document map includes all expected docs
- PASS: scaffold exists: .triad/evidence.ndjson
- PASS: scaffold exists: .triad/patches
- PASS: scaffold exists: .triad/runs
- PASS: scaffold exists: .gitignore
- PASS: scaffold exists: AGENTS.md
- PASS: scaffold exists: triad.toml
- PASS: AGENTS.md contains required rule: Work on exactly one claim per run.
- PASS: AGENTS.md contains required rule: Never edit `spec/claims/**` directly during `work`.
- PASS: AGENTS.md contains required rule: Do not run `git commit` or `git push`.
- PASS: CLI skeleton includes token: Init
- PASS: CLI skeleton includes token: Next
- PASS: CLI skeleton includes token: Work
- PASS: CLI skeleton includes token: Verify
- PASS: CLI skeleton includes token: Accept
- PASS: CLI skeleton includes token: Status
- PASS: CLI skeleton includes token: AgentCommand
- PASS: CLI skeleton includes token: AgentClaimCommand
- PASS: CLI skeleton includes token: AgentDriftCommand
- PASS: CLI skeleton includes token: AgentPatchCommand

## Verdict

READY
