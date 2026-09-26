# 真实应用差分验收

本目录启动 Python OpenAI-compatible 模型服务、nast 和本地 SillyTavern，再由 Playwright 操作真实页面/API。提示词由两个应用各自构建，测试程序不复制 ST 的提示词算法。

## 运行

在仓库根目录（PowerShell）：

```powershell
python -m venv .cache/st-parity-venv
.cache/st-parity-venv/Scripts/python.exe -m pip install -r tests/parity/requirements.txt
.cache/st-parity-venv/Scripts/python.exe -m playwright install chromium
npm --prefix refrence/SillyTavern ci
.cache/st-parity-venv/Scripts/python.exe tests/parity/run.py --stage all
```

默认先构建 Rust 可执行文件和前端。`--skip-build` 仅用于已经构建最新版本的定向重跑。可选阶段：`cards`、`embedded`、`worlds`、`macros`、`groups`、`management`、`ui`。`--output <目录>` 指定证据目录；默认使用系统临时目录。退出时停止测试启动的进程，保留证据，不读取或修改真实用户数据。

只启动模型服务：

```powershell
python tests/parity/model_server.py --port 19999 --output requests.jsonl
```

应用连接地址为 `http://127.0.0.1:19999/v1`，模型为 `gpt-4o`，不需要真实密钥。该服务支持 `/models`、非流式 JSON、含 reasoning 的 SSE、UTF-8 分片；URL 中包含 `/error/` 时返回 503，包含 `/abort/` 时提前结束 SSE。

## 覆盖与判据

| 阶段 | 主要检查 |
| --- | --- |
| cards | PNG/JSON/YAML/YML/CHARX/BYAF 基础导入、私聊、PNG/JSON 导出后交给真实 ST 再导入 |
| embedded | 从角色内嵌书导入独立书、绑定角色、连续两轮注入 |
| worlds | 全局＋聊天书、JS lookbehind 正则关键词、五轮递归和 sticky/cooldown 生命周期 |
| macros | 局部/全局变量；移除设置变量的宏后，下轮继续读取；全局变量写入磁盘 |
| groups | 三种组装模式；成员主书/辅助书、全局书、群会话书；普通/批次重生成/swipe/continue |
| management | nast 独立集成测试：角色重命名引用、群会话管理/备份、候选元数据恢复、资源 HTTP 访问 |
| ui | nast 真实界面：明暗主题 × 390/768/1440、私聊/群聊/世界书、IME、草稿、Escape 保存、角色编辑和重命名 |

每个生成场景比较完整解析后的模型请求 JSON、确定性回复和已保存消息。状态投影包含消息身份/正文/候选/推理/模型/提供方、角色来源、批次相等关系、局部变量、世界书关联和定时区间。`management`、`ui` 是 nast 行为断言，不是双端差分。

回复是规范化请求的 SHA-256 标记，用来发现上下文差异，不评价真实语言模型的文本质量。请求本身仍逐字段比较，不能因回复相同就忽略采样或流式设置差异。

时间戳、UI 遥测和随机批次 ID 的具体值不比较；批次之间是否相同会比较。ST 对原始 JSON 字段顺序计算世界书 hash，nast 对规范化条目计算，因此定时 hash 映射为 `world.uid` 身份后比较。原始 hash、完整状态及原始请求都保留在证据中；这不证明跨应用恢复活动计时记录等价。

`report.json` 记录检查结果和浏览器未捕获异常；场景目录保存两端 JSON、原始状态和差异；`ui/` 保存截图。非零退出码表示至少一项失败。该套件是可扩展的确定性回归基线，尚不穷举 ST 的全部字段、随机策略或高级规则组合。

## 模型添加与普通 HTTP

构建最新服务端和网页后运行：

```powershell
.cache/st-parity-venv/Scripts/python.exe tests/parity/model_setup.py
.cache/st-parity-venv/Scripts/python.exe tests/parity/routing.py
cargo test --manifest-path nast-bridges/Cargo.toml --workspace --locked
```

`model_setup.py` 使用真实局域网 HTTP 地址，断言 `isSecureContext=false` 且 `crypto.randomUUID` 不存在。默认取本机主机名解析到的 IPv4；需要时用 `NAST_E2E_HTTP_HOST` 指定非回环 IPv4，浏览器必须能访问该地址。测试短暂监听 0.0.0.0，使用隔离数据和测试账号，退出关闭服务。

覆盖登录错误、键盘提交、添加模型、保存前获取列表、三种协议输出与思考参数、草稿冲突恢复、角色卡 UI 导入及首条聊天；抓取真正的上游请求进行断言。覆盖明暗主题、390／1440 像素宽度，证据含 basic/parameters/chat 截图和 report.json，不调用真实模型。

`routing.py` 额外验证三协议流式和 JSON 的输出额度用尽、部分结果保存、不切换备用模型、冻结输入预算以及无密钥日志。长中文历史覆盖三种协议与自定义模型名，检查最新问题保留、旧历史裁剪和 Anthropic 续写只发送一份正文。模拟模型服务可用 `finish_reason` 编排结束原因。

桥接测试使用本地假 QQ 网关、假 NAST WebSocket 和真实 HTTP 请求接收器，验证慢生成期间网关心跳不中断、重复事件不重复生成、超长中文／emoji 全字符发送、发送失败保留尾部与重启后续读。未连接真实 QQ 账号，不代表已在真实 QQ 平台完成端到端验收。

模型表单复用项目已有的 shadcn Dialog、Field、Select、Collapsible，以及官方 Popover + Command combobox 示例；基础字段参考 [Cline OpenAI Compatible 配置](https://docs.cline.bot/provider-config/openai-compatible)。未新增自制基础 UI 组件。
