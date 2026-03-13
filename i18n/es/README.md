# Introducción a triad

[English README](../../README.md)

`triad` es un kernel de verificación determinista para una sola pregunta:

> ¿Esta claim es verdadera ahora mismo?

El repositorio mantiene una superficie pequeña a propósito. `Claim` es la única unidad canónica y la superficie pública actual se limita al core, al adaptador de filesystem y a un CLI delgado.

## Qué incluye

- `triad-core`
  Lógica de verificación pura
- `triad-fs`
  Adaptador de referencia para claims, snapshots, config y evidence logs
- `triad-cli`
  CLI de referencia con solo `init`, `lint`, `verify` y `report`

Fuera de alcance:

- `next`
- `work`
- `accept`
- `agent`
- runtime backends
- superficies de workflow para patch drafts

## Modelo mental

- `Claim`: un contrato atómico que quieres verificar
- `Evidence`: registros append-only ligados a una revisión de claim y a digests de artefactos
- `ClaimReport`: el resultado actual calculado a partir del snapshot presente y la evidencia guardada

`ClaimStatus` tiene solo cinco valores:

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

## Inicio rápido

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

## Resumen de configuración

`triad.toml` es un contrato estricto v2.

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

`verify.commands` acepta strings heredados y objetos estructurados. El objeto estructurado usa `command`, `locator` opcional y `artifacts` opcional. Durante `triad verify --claim <CLAIM_ID>`, `{claim_id}` y `{claim_path}` se expanden contra la claim seleccionada.

## Notas sobre evidence

- Solo la evidencia `Hard` cambia el status.
- La freshness se calcula solo sobre el subconjunto de artefactos grabado en la fila de evidence.
- El adaptador shell de referencia genera solo `0 => pass` y `non-zero => fail`.
- `unknown` no se genera automáticamente por shell, así que un reporte `blocked` requiere evidence seeded, manual o non-shell.

## Lecturas adicionales

- [Domain model](../../docs/02-domain-model.md)
- [Claim format](../../docs/03-spec-format.md)
- [CLI contract](../../docs/05-cli-contract.md)
