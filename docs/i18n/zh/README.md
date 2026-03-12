# triad 中文入口

[韩文主 README](../../../README.md)

`triad` 是一个无交互、可确定重放的 verification kernel。它只回答一个问题：

> 这个 Claim 现在是否为真？

当前 public surface:

- `triad-core`
- `triad-fs`
- `triad-cli`

当前命令：

- `triad init`
- `triad lint [--claim <CLAIM_ID> | --all] [--json]`
- `triad verify --claim <CLAIM_ID> [--json]`
- `triad report [--claim <CLAIM_ID> | --all] [--json]`

快速开始：

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

继续阅读：

- [文档地图](../../00-document-map.md)
- [领域模型](../../02-domain-model.md)
- [CLI 合同](../../05-cli-contract.md)
