# triad 项目入口

[English README](../../README.md)

`triad` 是一个确定性验证内核，只回答一个问题：

> 这个 Claim 现在是真的吗？

这个仓库有意保持很小的产品表面。`Claim` 是唯一的 canonical work unit，当前公开表面只包含 core、filesystem adapter 和一个很薄的 CLI。

## 包含内容

- `triad-core`
  纯验证逻辑
- `triad-fs`
  处理 claims、snapshots、config 和 evidence log 的 filesystem reference adapter
- `triad-cli`
  只提供 `init`、`lint`、`verify`、`report` 四个命令的 reference CLI

不在范围内：

- `next`
- `work`
- `accept`
- `agent`
- runtime backend
- patch draft workflow surface

## 心智模型

- `Claim`：要验证的原子契约
- `Evidence`：绑定到 claim revision 和 artifact digest 的 append-only 验证记录
- `ClaimReport`：基于当前 snapshot 与已有 evidence 计算出的当前结果

`ClaimStatus` 只有五种：

- `confirmed`
- `contradicted`
- `blocked`
- `stale`
- `unsupported`

## 快速开始

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- lint --all
cargo run -p triad-cli -- verify --claim REQ-auth-001
cargo run -p triad-cli -- report --all --json
```

## 配置摘要

`triad.toml` 是严格的 v2 contract。

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

`verify.commands` 同时支持 legacy string entry 和 structured object entry。structured object 使用 `command`、可选 `locator`、可选 `artifacts`。执行 `triad verify --claim <CLAIM_ID>` 时，`{claim_id}` 和 `{claim_path}` 会按选中的 claim 展开。

## evidence 说明

- 只有 `Hard` evidence 会改变 status。
- freshness 只针对 evidence 行里记录的 artifact 子集计算。
- reference shell adapter 只会生成 `0 => pass` 和 `non-zero => fail`。
- shell 路径不会自动生成 `unknown`，所以 `blocked` report 需要 seeded、manual 或 non-shell evidence 路径。

## 深入阅读

- [Domain model](../../docs/02-domain-model.md)
- [Claim format](../../docs/03-spec-format.md)
- [CLI contract](../../docs/05-cli-contract.md)
