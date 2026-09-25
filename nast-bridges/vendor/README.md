# 随附的依赖源码

`qqbot-connector` 0.1.0 是 QQ 扫码连接 SDK 的本地 Rust 实现，依照 `@tencent-connect/qqbot-connector` 协议工作。2026-09-25 从原同级 `qqbot-connector` 目录导入；该目录当时尚无 Git 提交或远端，因此不标注不存在的上游提交号。

保留了原始 Cargo 清单、源码、示例、测试、README 以及 MIT / Apache-2.0 许可证。没有复制构建产物、Git 元数据、运行凭据或独立锁文件；依赖版本统一由 `nast-bridges/Cargo.lock` 固定。该源码现由本仓库跟踪，桥接构建不再依赖仓库外目录。

验证：`cargo test --manifest-path nast-bridges/Cargo.toml --workspace --locked`。真实 QQ 端点测试默认 `#[ignore]`，日常测试不会创建真实绑定任务。
