# 开发指南

## 常用命令

```bash
cargo run                        # 起服务端（需先构建 web/dist）
cd web && npm run dev            # 前端 HMR（3000 端口，代理到 8000）
cargo test --workspace           # 全量测试
mdbook serve --open              # 本文档预览（http://localhost:3000）
```

## 对齐 ST 的工作流

1. 行为疑问先查 `refrence/SillyTavern`（v1.18.0 检出）拿准语义；
2. 实现时代码注释标注 ST 源码锚点（如 `openai.js:801 populationInjectionPrompts`）；
3. 在 `docs/ST_PARITY_AUDIT.md` 登记差异，完成验证后更新状态；
4. 加端到端冒烟覆盖（见[测试](./testing.md)）。

## 前端约定

- 组件只用 shadcn/ui 生态（含补齐：command/popover/checkbox/radio-group/collapsible/
  toggle-group），不手写 UI 部件；复杂交互用成熟库（dnd-kit 排序）
- 状态单库 zustand（`web/src/store.ts`）；WS 客户端 `web/src/rpc.ts`
  （指数退避重连 + `generate.status` 恢复流式气泡）
- 命令层 `web/src/commands.ts`：内置 → 自定义 → 插件 → 未知拦截

## 加一个 RPC

1. `src/rpc.rs` dispatch 表加方法名 → handler；
2. 语义尽量落在 engine/storage crate（可测试），handler 只做参数解析；
3. 前端 `web/src/rpc.ts` 直接 `rpc.call('方法名', {...})`，无需封装；
4. 若属 Bridge API（见[桥接](./bridges.md)），在同一提交中同步 `nast-bridges/`，并运行 `cargo test --manifest-path nast-bridges/Cargo.toml --workspace --locked`。

## 文档（本 mdBook）

```bash
mdbook build          # 产出 book/（已 gitignore）
mdbook serve          # 本地预览
```

- 源在 `docs/src/`，目录 `SUMMARY.md`；
- CI 推送 main 自动发布 GitHub Pages（`.github/workflows/doc-release.yml`）；
- 首次发布需在仓库 Settings → Pages 将 Source 设为 **GitHub Actions**；
- 若仓库名不是 `nast`，改 `book.toml` 的 `site-url = "/<repo>/"`。
