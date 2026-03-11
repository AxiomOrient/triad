# triad en español

[Volver al README principal](../../../README.md)

`triad` es una CLI local-first para trabajar una pequeña necesidad a la vez, guardar prueba de verificación y evitar que un cambio generado por IA se vuelva verdad final sin revisión humana.

## Flujo estándar

```text
next -> work -> verify -> accept
```

## Inicio rápido

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- next
cargo run -p triad-cli -- work REQ-auth-001
cargo run -p triad-cli -- verify REQ-auth-001
cargo run -p triad-cli -- status --claim REQ-auth-001
```

Si aparece un patch pendiente:

```bash
cargo run -p triad-cli -- accept --latest
```

## Leer después

- [Mapa de documentos](../../00-document-map.md)
- [Workflows](../../04-workflows.md)
- [Contrato de CLI](../../05-cli-contract.md)
- [Integración de runtime](../../08-runtime-integration.md)
- [Checklist de release](../../15-release-readiness.md)
