# triad en español

[README principal en coreano](../../../README.md)

`triad` es un verification kernel determinista y sin interfaz interactiva. Responde una sola pregunta:

> ¿Este Claim es verdadero ahora mismo?

Superficie pública actual:

- `triad-core`
- `triad-fs`
- `triad-cli`

Comandos actuales:

- `triad init`
- `triad lint [--claim <CLAIM_ID> | --all] [--json]`
- `triad verify --claim <CLAIM_ID> [--json]`
- `triad report [--claim <CLAIM_ID> | --all] [--json]`

Inicio rápido:

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

Seguir leyendo:

- [Mapa de documentos](../../00-document-map.md)
- [Modelo de dominio](../../02-domain-model.md)
- [Contrato de CLI](../../05-cli-contract.md)
