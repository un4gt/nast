# 群聊实施与验收记录

参照：`refrence/SillyTavern`，1.18.0，提交 `8172dcd0ee672d3cd9a5e5f7af134f91a45cd2b8`。

判据为服务端行为、模型请求、JSON/JSONL 往返。Lua 仅运行在后端；不实现 STScript、ST JavaScript 扩展运行时或浏览器 UI 插件。未知扩展字段保留不代表可执行。

## 功能对照

| 功能 | ST 源码 | NAST 入口 | 验收与状态 |
| --- | --- | --- | --- |
| 群管理、成员排序/禁言/健谈度 | `public/scripts/group-chats.js`、`src/endpoints/groups.js` | `src/rpc.rs`、`GroupPanel`、`GroupSettingsDialog` | 已接通名称/头像/标签/收藏/搜索、排序、禁用、角色卡编辑；UI 矩阵与无网页命令验证 |
| NATURAL/LIST/MANUAL/POOLED | `group-chats.js:945–1316`、`utils.js:384,1357` | `crates/nast-engine/src/group_chat.rs` | 12,000 组 ST 原函数随机重放全部通过；涵盖同名、名字分词、缺失/静音成员、最后发言者、单成员 |
| 三种卡片模式、深度提示/宏 | `group-chats.js`、`script.js` | `group_chat.rs`、`src/prompt_bridge.rs` | 三种模式真实双端完整请求差分；沿用世界书/Persona/作者注记/正则与宏管线，覆盖不代表穷举组合 |
| 逐成员生成、候选/续写/整批重生成 | `group-chats.js:945`、`script.js` | `src/group_gen.rs`、`src/generate.rs` | quiet/impersonate 已接通；候选元数据恢复/删除、整批重生成失败恢复、逐成员状态与流式落盘 |
| 会话、导入导出、备份 | `src/endpoints/chats.js` | `src/group_sessions.rs` | 历史/分支/书签/返回主会话/单聊复制转换已接通；重命名更新关联；未知字段保留与 2,000 次操作验证 |
| 命令统一入口 | `group-chats.js`、`slash-commands.js` | `src/commands.rs`（新增） | 后端处理，客户端无需获取清单 |
| Lua 生命周期与队列 | NAST 独立能力 | `crates/nast-plugin`、`src/plugins.rs` | 串行转换、缺失钩子跳过、队列合法性、实际中断含 pcall 死循环、异常继续、异步等待、KV 重启恢复 |
| 状态/断连/停止 | `group-chats.js` | `src/state.rs`、`src/ws.rs` | 后端自动对话、任务重放、队列/成员/阶段上下文、断连继续、停止不启动下一成员 |
| 群详情、绑定与草稿 | 现有 Figma 风格 | `web/src` | 六种主题/视口验证；草稿按群与会话隔离；面板实例 ID 独立；订阅后台自动生成及落盘 |

## 验收门槛

- 调度：至少 10,000 个可重放输入，以仓库内 ST 原函数为 oracle。
- 双端：至少 300 个具名场景，真实启动 ST/NAST，比对请求、消息、候选、批次和落盘。
- 浏览器：明/暗 × 390/768/1440。
- 故障：至少 2,000 次操作；慢流、断流、429/5xx、停止、断连、重启、Lua 超时。
- 无网页 RPC：不预读插件清单，命令、队列、生成、KV 重启恢复、超时后插件/重载仍工作。
- 真实模型：仅独立测试环境变量；最多 1,000 请求，每次输入不超过 8K、输出不超过 512；未配置时标为未执行，不使用日常配置。

这里列出的验收门槛不代表已经通过；最终结果以实际命令和证据为准。

## 运行与证据

复现命令位于 `tests/parity/README.md`。API 契约见 `docs/GROUPCHAT_LUA.md`，无需网页的 Lua 示例见 `examples/plugins/group_context.lua`。

统一结果索引：`.cache/groupchat/summary.json`。最终无浏览器、UI 与 12 场景回归使用的可执行文件 SHA-256 已核对一致。

| 检查 | 证据位置 | 判据 |
| --- | --- | --- |
| 12,000 组调度差分 | `.cache/groupchat/scheduling.json` | 固定 ST 原函数 + 同一随机序列，成员队列逐项相等 |
| 300 个双端生成场景 | `.cache/groupchat/parity-300/report.json` | 三种组装模式 × 25 轮 × 四种操作；完整模型请求、消息/候选/批次关系及持久化 |
| 最终二进制差分回归 | `.cache/groupchat/parity-final/report.json` | 最终 release 上再次执行三种模式 × 四种操作，全部通过 |
| 无浏览器与 2,000 次操作 | `.cache/groupchat/headless-latest.json` 指向独立证据目录 | 从未获取插件清单；请求、通知、钩子、命令、归属、队列、停止、断连、重启、429/503/断流及 Lua 故障 |
| 六种界面组合 | `.cache/groupchat/ui-latest.json` 指向独立证据目录 | 草稿/成员/绑定/Lua 同业务跨客户端比较、自动生成显示、刷新恢复、截图与无浏览器异常 |
| Rust 工作区 | `.cache/group-workspace-tests-verified.log` | 全部测试通过 |
| 桥接工作区 | `.cache/group-bridge-tests.log` | 全部测试通过；未连接真实桥接平台 |
| 网页构建与类型检查 | `.cache/group-web-build-final.log`、`.cache/group-ts-final.log` | 构建成功，TypeScript 无错误 |
| 真实模型预算保护 | `.cache/group-real-budget-tests.log` | 三个本地代理测试；每次上游尝试计费数、超额拒绝、证据无密钥 |

300 个场景是带累积历史和不同输入的生成场景，并非 300 类独立功能。忽略时间戳和随机 ID 的具体值，但比较批次相等关系；完整原始状态仍保留。每个脚本失败时非零退出，不能只用日志中若干 PASS 判定整套通过。

本轮 300 场景汇总为 1,501 项检查全部通过，浏览器未捕获异常为 0；最终二进制的 12 场景回归为 61 项检查全部通过。未执行真实外部模型调用。

## 边界与未完成验收

- 真实模型未配置独立 `NAST_GROUP_TEST_*` 环境变量，实际外部调用为 0。已提供 `--real-model` 入口、两端共享硬调用额度和输入/输出限制；真实语义身份、上下文和外部服务表现尚未验证。
- 不运行通用 STScript、ST JavaScript 扩展或浏览器 UI 插件。命令是文档列出的子集，STScript 的任意命名参数、管道、闭包不等价执行；`member-peek` 返回数据而不强制打开浏览器面板。
- 任务重放窗口为当前进程内最多 256 项，未实现跨重启的持久任务去重。
- 文件写入使用原子替换及备份；重命名有快照与失败还原，但多文件修改不提供断电级事务保证。磁盘耗尽、写保护和进程恰在多文件提交中退出的全面故障矩阵尚未穷举。
- 300 场景的确定性请求差分与六种 UI 操作覆盖不证明所有 ST 复杂扩展字段、宏、深度提示、世界书、Persona 和作者注记组合完全等价。上述额外矩阵和真实模型证据补齐前，不应宣称整个计划的全部验收门槛已完成。
