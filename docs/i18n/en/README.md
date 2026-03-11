# triad

[한국어 README](../../../README.md)

`triad` is a local-first CLI for keeping spec, code, and tests in sync without giving final authority to an AI-generated change.

## In Plain Words

- break work into one small requirement at a time
- let `work` change code and tests only for that requirement
- let `verify` save proof of what was checked
- let `accept` update the spec only through an explicit reviewed patch draft

Core line:

> The model proposes, the engine verifies, the human approves.

## Standard Flow

```text
next -> work -> verify -> accept
```

## Quickstart

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- next
cargo run -p triad-cli -- work REQ-auth-001
cargo run -p triad-cli -- verify REQ-auth-001
cargo run -p triad-cli -- status --claim REQ-auth-001
```

If verification creates a pending spec patch:

```bash
cargo run -p triad-cli -- accept --latest
```

## What To Read Next

- [Document map](../../00-document-map.md)
- [Workflows](../../04-workflows.md)
- [CLI contract](../../05-cli-contract.md)
- [Runtime integration](../../08-runtime-integration.md)
- [Release readiness](../../15-release-readiness.md)
