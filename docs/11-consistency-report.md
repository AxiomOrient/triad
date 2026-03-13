# Consistency Report

## Summary

- PASS count: 37
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
- PASS: document exists: docs/11-consistency-report.md
- PASS: document exists: docs/15-release-readiness.md
- PASS: schema parses as JSON: schemas/claim.schema.json
- PASS: schema parses as JSON: schemas/evidence.schema.json
- PASS: schema parses as JSON: schemas/claim_report.schema.json
- PASS: schema parses as JSON: schemas/lint_report.schema.json
- PASS: schema parses as JSON: schemas/triad_config.schema.json
- PASS: obsolete crate removed: crates/triad-config
- PASS: obsolete crate removed: crates/triad-runtime
- PASS: obsolete schema removed: schemas/agent.claim.get.schema.json
- PASS: obsolete schema removed: schemas/agent.claim.list.schema.json
- PASS: obsolete schema removed: schemas/agent.claim.next.schema.json
- PASS: obsolete schema removed: schemas/agent.drift.detect.schema.json
- PASS: obsolete schema removed: schemas/agent.patch.apply.schema.json
- PASS: obsolete schema removed: schemas/agent.patch.propose.schema.json
- PASS: obsolete schema removed: schemas/agent.run.schema.json
- PASS: obsolete schema removed: schemas/agent.status.schema.json
- PASS: obsolete schema removed: schemas/agent.verify.schema.json
- PASS: obsolete schema removed: schemas/envelope.schema.json
- PASS: workspace members match triad-core/triad-fs/triad-cli
- PASS: triad.toml matches minimal v2 config
- PASS: CLI help matches current command surface
- PASS: example claims parse via CLI lint
- PASS: CLI verify emits direct JSON and appends fresh evidence
- PASS: CLI report emits direct JSON array for all claims
- PASS: document map includes expected docs

## Verdict

READY
