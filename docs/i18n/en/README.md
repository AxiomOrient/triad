# triad

[한국어 README](../../../README.md)

`triad` is a headless deterministic verification kernel that answers one question:

> Is this Claim true right now?

Current public surface:

- `triad-core`
- `triad-fs`
- `triad-cli`

Current commands:

- `triad init`
- `triad lint [--claim <CLAIM_ID> | --all] [--json]`
- `triad verify --claim <CLAIM_ID> [--json]`
- `triad report [--claim <CLAIM_ID> | --all] [--json]`

Quickstart:

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

Read next:

- [Document map](../../00-document-map.md)
- [Domain model](../../02-domain-model.md)
- [CLI contract](../../05-cli-contract.md)
