# triad 中文入口

[返回主 README](../../../README.md)

`triad` 是一个 local-first CLI。它把工作拆成很小的需求单位，保存验证证据，并且不让 AI 生成的修改直接变成最终事实。

## 标准流程

```text
next -> work -> verify -> accept
```

## 快速开始

```bash
cargo run -p triad-cli -- init
cargo run -p triad-cli -- next
cargo run -p triad-cli -- work REQ-auth-001
cargo run -p triad-cli -- verify REQ-auth-001
cargo run -p triad-cli -- status --claim REQ-auth-001
```

如果出现待应用的 patch：

```bash
cargo run -p triad-cli -- accept --latest
```

## 继续阅读

- [文档地图](../../00-document-map.md)
- [工作流](../../04-workflows.md)
- [CLI 合同](../../05-cli-contract.md)
- [运行时集成](../../08-runtime-integration.md)
- [发布检查清单](../../15-release-readiness.md)
