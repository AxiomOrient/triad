# triad 한국어 포털

[루트 README](../../../README.md)

루트 README가 한국어 source of truth다. 이 문서는 동일한 command surface를 짧게 다시 보여주는 entrypoint다.

현재 public surface:

- `triad-core`
- `triad-fs`
- `triad-cli`

현재 명령:

- `triad init`
- `triad lint [--claim <CLAIM_ID> | --all] [--json]`
- `triad verify --claim <CLAIM_ID> [--json]`
- `triad report [--claim <CLAIM_ID> | --all] [--json]`

빠른 시작:

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```
