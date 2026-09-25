# 测试

## Rust 单元/集成测试

```bash
cargo test --workspace     # 宏/拼装/世界书/正则/角色导入/存储/插件/桥接核心…
```

各 crate 测试对照 ST 源码语义逐条验证（如世界书预算累计制、`{{pick}}`
的 getStringHash/ARC4 复刻、fixMarkdown 引号补齐向量）。

## 端到端冒烟（Node，起真实服务端 + mock OpenAI）

前置：`cargo build`；`ws` 模块路径可用 `NAST_WS_MODULE` 覆盖（默认
`D:/temp/nast-smoke/node_modules/ws`）。

```bash
node tests/m0_connection_smoke.js     # 连接闭环（secrets/models.list/custom_url）10/10
node tests/m1_pipeline_smoke.js       # 聊天链路 ST 一致性（WI/AN/persona/reasoning/停止串/编辑）20/20
node tests/m3_group_smoke.js          # 群聊（激活策略/触发单成员/静音）12/12
node tests/m4_preset_regex_smoke.js   # 预设 round-trip / 全局正则 / Prompt Manager 6/6
node tests/m5_plugin_smoke.js         # 插件（命令/KV/toast/prompt_built/死循环超时隔离）8/8
node tests/m6_reconnect_smoke.js      # WS 断线重连 + 生成进度恢复 7/7
```

冒烟脚本自带 mock OpenAI 兼容服务（SSE），在临时目录起隔离实例，
不触碰真实 `data/`。

## 真实 ST 差分验收

仓库 `tests/parity/README.md` 提供完整安装与运行说明。Python 模型服务返回确定性请求摘要，
nast 和本地 SillyTavern 各自构造请求，再比较完整 JSON、回复及保存状态。

```powershell
.cache/st-parity-venv/Scripts/python.exe tests/parity/run.py --stage all
```

覆盖六种角色格式、内嵌书、世界书递归/计时、宏变量、三种群聊组装模式及候选/续写；
管理和 UI 阶段另做 nast 独立断言。基线验收为 239 项通过；范围与限制见
`docs/ST_PARITY_IMPLEMENTATION.md`，不代表所有 ST 功能均已对齐。

## 角色库异常回归

```powershell
npm --prefix web run test:characters
.cache/st-parity-venv/Scripts/python.exe tests/character_library_browser.py --live
```

浏览器测试复用前述 Playwright 环境，需启动已构建的 nast（默认端口 8000，
可用 `--base-url` 修改）。测试模拟正常/错误混合、全部错误和空角色列表；
`--live` 额外只读检查真实角色库与页面，不修改数据或调用模型。

## TTS 验收

```bash
node tests/tts_smoke.cjs          # 25 个 HTTP 提供方的模拟合约与管理接口
node tests/tts_browser.cjs        # 单聊/群聊播放、自动朗读和设置流程
node tests/tts_local_browser.cjs  # 可选：下载真实本地模型并合成，需要外网
npm --prefix web run test:tts     # 文本过滤、分段与旧配置兼容
```

依赖路径可通过 `NAST_WS_MODULE` / `NAST_PLAYWRIGHT_MODULE` 指定；
模拟合约通过不等同于所有商业服务已用真实密钥实测。

## 验收口径

任何聊天链路改动要求：`cargo test --workspace` 全绿 + 对应冒烟通过；
涉及 ST 对齐的修复同时在 `docs/ST_PARITY_AUDIT.md` 更新状态并注明提交。

## 多模型路由验收

构建最新服务端和网页后，使用现有 parity 虚拟环境执行：

```powershell
cargo test --workspace
cargo build --bin nast
npx tsc --noEmit -p web/tsconfig.json
npm --prefix web run build
.cache/st-parity-venv/Scripts/python.exe tests/parity/routing.py
.cache/st-parity-venv/Scripts/python.exe tests/parity/run.py --stage all
```

`routing.py` 使用临时数据目录，记录三协议请求、迁移、版本冲突和保存失败、准确重试次数、黏性隔离、Retry-After、取消与总预算、部分正文／思考落盘、命令和各生成入口。打印的 Evidence 目录保留 report.json、请求及服务端日志。虚拟服务 `plans[route]` 支持 status、retry_after、delay、chunk_delay、cutoff（before/reasoning/body/no_done）、invalid_json、stream_error。不访问生产模型或真实用户数据。

同级桥接仓库运行 `cargo test --workspace`，覆盖持久化会话与已提交生成断线不重发。升级迁移另外用新构建镜像、隔离 bind mount 检查备份、密钥引用和重复启动，不替换生产容器。

容器迁移可重复运行：

```powershell
docker build -t nast:model-routing-test .
python tests/parity/container_migration.py --image nast:model-routing-test
```

脚本只创建独立临时数据目录和随机名称的测试容器，退出后移除该测试容器，保留备份与报告。它验证原文件逐字节备份、密钥迁移以及重启后目录不变。
