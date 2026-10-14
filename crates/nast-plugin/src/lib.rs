//! nast-plugin：mlua 插件层（服务端运行）。
//!
//! 插件以 Lua 脚本注册事件钩子与命令，通过 nast API 表访问宿主能力。
//! 所有 Lua 执行固定在一条专用 OS 线程上（Lua !Send），每次派发限时
//! （NAST_PLUGIN_TIMEOUT_SECS，默认 10s），并用指令预算实际中断超时回调。
//!
//! 插件 API（宿主提供，无需样板代码）：
//! - nast.on(event, fn)               注册事件钩子（可多次注册）
//! - nast.register_command(name, fn)  注册斜杠命令 /name args... → fn(args, context)
//! - nast.get_var(key) / set_var      插件级 KV（plugin_vars.json 持久化）
//! - nast.log(...)                    日志
//! - nast.notify(message, type)       通用通知（日志与 hub 广播，不等待客户端）
//! - nast.toast(message, type)        notify 的兼容入口
//!
//! 钩子返回值：返回非 nil 字符串 = 转换事件携带文本（user_input/ai_output）；
//! prompt_built 事件可返回 {messages={{role=,content=},...}} 重写拼装结果。
//!
//! 事件名：generation_started / user_input / prompt_built / ai_output /
//! message_saved / generation_ended（与 WS 事件总线一致）。

use mlua::{Lua, Value as LuaValue};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("lua: {0}")]
    Lua(#[from] mlua::Error),
    #[error("plugin error: {0}")]
    Custom(String),
}

pub type PluginResult<T> = Result<T, PluginError>;

/// toast 回调（接线到 EventHub）。
pub type ToastFn = Arc<dyn Fn(&str, &str) + Send + Sync + 'static>;
/// KV 共享存储（plugin_vars.json 的内存镜像）。
type KvStore = Arc<Mutex<BTreeMap<String, JsonValue>>>;

/// 插件运行信息（UI/调试）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub hooks: Vec<String>,
    pub commands: Vec<String>,
}

enum Cmd {
    LoadDir {
        dir: std::path::PathBuf,
        kv_path: std::path::PathBuf,
        reply: std::sync::mpsc::Sender<Result<Vec<String>, String>>,
    },
    Dispatch {
        event: String,
        data: JsonValue,
        reply: std::sync::mpsc::Sender<Vec<(String, String)>>,
    },
    DispatchJson {
        event: String,
        data: JsonValue,
        reply: std::sync::mpsc::Sender<Option<JsonValue>>,
    },
    RunCommand {
        input: String,
        context: JsonValue,
        reply: std::sync::mpsc::Sender<Result<Option<String>, String>>,
    },
    List {
        reply: std::sync::mpsc::Sender<Vec<PluginInfo>>,
    },
    SetToast {
        toast: Option<ToastFn>,
    },
}

/// 插件管理器：每插件一个 Lua 实例；须在单一亲和线程上使用（!Send）。
pub struct PluginManager {
    plugins: Vec<Plugin>,
    kv: KvStore,
    kv_path: Option<std::path::PathBuf>,
    toast: Arc<Mutex<Option<ToastFn>>>,
}

struct Plugin {
    name: String,
    lua: Lua,
}

fn arm_budget(lua: &Lua) {
    let deadline = Instant::now() + Duration::from_secs(std::env::var("NAST_PLUGIN_TIMEOUT_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(10).max(1));
    let instructions = Arc::new(AtomicU64::new(0));
    lua.set_hook(mlua::HookTriggers::new().every_nth_instruction(10_000), move |_, _| {
        if Instant::now() >= deadline || instructions.fetch_add(10_000, Ordering::Relaxed) >= 5_000_000 {
            std::panic::resume_unwind(Box::new(BudgetExceeded));
        }
        Ok(mlua::VmState::Continue)
    });
}

struct BudgetExceeded;

fn bounded<T>(lua: &Lua, call: impl FnOnce() -> mlua::Result<T>) -> mlua::Result<T> {
    arm_budget(lua);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(value) => value,
        Err(error) if error.is::<BudgetExceeded>() => Err(mlua::Error::RuntimeError("plugin execution budget exceeded".into())),
        Err(_) => Err(mlua::Error::RuntimeError("plugin callback panicked".into())),
    }
}

fn event_hooks(lua: &Lua, event: &str) -> Option<mlua::Table> {
    lua.globals().raw_get::<mlua::Table>("nast").ok()?.raw_get::<mlua::Table>("hooks").ok()?.raw_get::<Option<mlua::Table>>(event).ok().flatten()
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: vec![],
            kv: Arc::new(Mutex::new(BTreeMap::new())),
            kv_path: None,
            toast: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_kv(mut self, kv: KvStore, kv_path: std::path::PathBuf) -> Self {
        self.kv = kv;
        if let Ok(raw) = std::fs::read_to_string(&kv_path) {
            if let Ok(map) = serde_json::from_str::<BTreeMap<String, JsonValue>>(&raw) {
                *self.kv.lock().unwrap() = map;
            }
        }
        self.kv_path = Some(kv_path);
        self
    }

    pub fn set_toast(&mut self, toast: Option<ToastFn>) {
        *self.toast.lock().unwrap() = toast;
    }

    /// 从源码加载插件（宿主注入 nast API 表）。
    pub fn load(&mut self, name: &str, code: &str) -> PluginResult<()> {
        let lua = Lua::new_with(mlua::StdLib::ALL_SAFE, mlua::LuaOptions::new().catch_rust_panics(false))?;
        lua.set_memory_limit(32 * 1024 * 1024)?;
        // Blocking native I/O and new coroutine threads bypass instruction deadlines.
        for name in ["io", "os", "package", "require", "dofile", "loadfile", "coroutine"] {
            lua.globals().set(name, LuaValue::Nil)?;
        }
        arm_budget(&lua);
        let kv = self.kv.clone();
        let kv_path_save = self.kv.clone();
        let plugin_name = name.to_string();
        let toast = self.toast.clone();

        let nast = lua.create_table()?;

        // nast.hooks / nast.commands（宿主侧注册表）
        let hooks = lua.create_table()?;
        nast.set("hooks", hooks)?;
        let commands = lua.create_table()?;
        nast.set("commands", commands)?;

        // nast.on(event, fn)
        let on_fn = lua.create_function(|lua, (event, func): (String, mlua::Function)| {
            let nast: mlua::Table = lua.globals().raw_get("nast")?;
            let hooks: mlua::Table = nast.get("hooks")?;
            let list: mlua::Table = match hooks.get::<Option<mlua::Table>>(event.clone())? {
                Some(t) => t,
                None => lua.create_table()?,
            };
            list.set(list.raw_len() + 1, func)?;
            hooks.set(event, list)?;
            Ok(())
        })?;
        nast.set("on", on_fn)?;

        // nast.register_command(name, fn(args) -> string)
        let reg_fn = lua.create_function(|lua, (cmd, func): (String, mlua::Function)| {
            let nast: mlua::Table = lua.globals().raw_get("nast")?;
            let commands: mlua::Table = nast.get("commands")?;
            commands.set(cmd.trim_start_matches('/').to_lowercase(), func)?;
            Ok(())
        })?;
        nast.set("register_command", reg_fn)?;

        // nast.get_var / set_var（plugin:<name>: 命名空间，写穿落盘）
        let kv_read = kv.clone();
        let prefix = format!("plugin:{name}:");
        nast.set(
            "get_var",
            lua.create_function(move |lua, key: String| {
                let full = format!("{prefix}{key}");
                let map = kv_read.lock().unwrap();
                Ok(match map.get(&full) {
                    Some(v) => json_to_lua(lua, v)?,
                    None => LuaValue::Nil,
                })
            })?,
        )?;
        let kv_write = kv_path_save;
        let kv_write_path = self.kv_path.clone();
        nast.set(
            "set_var",
            lua.create_function(move |lua, (key, value): (String, LuaValue)| {
                let full = format!("plugin:{plugin_name}:{key}");
                let json = lua_to_json(lua, &value).ok_or_else(|| mlua::Error::external("unsupported KV value"))?;
                let mut guard = kv_write.lock().unwrap();
                let mut next = guard.clone();
                next.insert(full, json);
                if let Some(path) = &kv_write_path {
                    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(mlua::Error::external)?; }
                    let bytes = serde_json::to_vec(&next).map_err(mlua::Error::external)?;
                    let tmp = path.with_extension("tmp");
                    std::fs::write(&tmp, bytes).map_err(mlua::Error::external)?;
                    std::fs::rename(&tmp, path).map_err(mlua::Error::external)?;
                }
                *guard = next;
                Ok(())
            })?,
        )?;

        // nast.log
        let log_name = name.to_string();
        nast.set(
            "log",
            lua.create_function(move |_, msgs: mlua::MultiValue| {
                let parts: Vec<String> = msgs
                    .into_iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                tracing::info!("[plugin:{log_name}] {}", parts.join(" "));
                Ok(())
            })?,
        )?;

        // nast.toast(message, type)
        let toast_name = name.to_string();
        let toast_cb = toast.clone();
        nast.set(
            "toast",
            lua.create_function(move |_, (message, kind): (String, Option<String>)| {
                let kind = kind.unwrap_or_else(|| "info".into());
                tracing::info!("[plugin:{toast_name}] toast: {message}");
                if let Some(cb) = &*toast_cb.lock().unwrap() {
                    cb(&message, &kind);
                }
                Ok(())
            })?,
        )?;

        nast.set("notify", nast.get::<mlua::Function>("toast")?)?;

        // nast.json_decode / json_encode（结构化钩子用）
        nast.set(
            "json_decode",
            lua.create_function(|lua, text: String| {
                let v: JsonValue = serde_json::from_str(&text)
                    .map_err(|e| mlua::Error::external(format!("json_decode: {e}")))?;
                json_to_lua(lua, &v)
            })?,
        )?;
        nast.set(
            "json_encode",
            lua.create_function(|lua, value: LuaValue| {
                let j = lua_to_json(lua, &value)
                    .ok_or_else(|| mlua::Error::external("json_encode: unsupported value"))?;
                Ok(j.to_string())
            })?,
        )?;

        lua.globals().set("nast", nast)?;
        bounded(&lua, || lua.load(code).set_name(name).exec())?;

        self.plugins.push(Plugin {
            name: name.to_string(),
            lua,
        });
        self.plugins.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(())
    }

    /// 派发事件，收集 (插件名, 转换文本)。
    pub fn dispatch(&self, event: &str, data: &JsonValue) -> Vec<(String, String)> {
        let mut transforms = Vec::new();
        let mut current = data.clone();
        for plugin in &self.plugins {
            let Some(list) = event_hooks(&plugin.lua, event) else { continue };
            for func in list.sequence_values::<mlua::Function>().flatten() {
                match bounded(&plugin.lua, || func.call::<LuaValue>(current.to_string())) {
                    Ok(LuaValue::String(text)) => {
                        let text = text.to_string_lossy();
                        current["text"] = JsonValue::String(text.clone());
                        transforms.push((plugin.name.clone(), text));
                    }
                    Ok(_) => {},
                    Err(e) => tracing::warn!("[plugin:{}] hook {event} failed: {e}", plugin.name),
                }
            }
        }
        transforms
    }

    /// Serial transformations retain context and the last valid result on failure.
    pub fn dispatch_json(&self, event: &str, data: &JsonValue) -> Option<JsonValue> {
        let mut current = data.clone();
        let mut last = None;
        for plugin in &self.plugins {
            let Some(list) = event_hooks(&plugin.lua, event) else { continue };
            for func in list.sequence_values::<mlua::Function>().flatten() {
                let result = match bounded(&plugin.lua, || func.call::<LuaValue>(current.to_string())) {
                    Ok(LuaValue::Nil) => continue,
                    Ok(value) => lua_to_json(&plugin.lua, &value),
                    Err(e) => { tracing::warn!("[plugin:{}] hook {event} failed: {e}", plugin.name); continue; }
                };
                let Some(mut result) = result else { continue };
                if let JsonValue::String(text) = &result {
                    if let Ok(parsed) = serde_json::from_str(text) { result = parsed; }
                }
                let field = if event == "group_queue" { "queue" } else { "messages" };
                // Lua {} is the empty array in the queue API.
                if event == "group_queue" && result.as_object().is_some_and(|m| m.is_empty()) {
                    result = serde_json::json!([]);
                }
                let values = result.get(field).unwrap_or(&result);
                let valid = if event == "group_queue" {
                    values.as_array().is_some_and(|queue| {
                        let mut seen = std::collections::HashSet::new();
                        queue.iter().all(|a| a.as_str().is_some_and(|a|
                            data["eligible_members"].as_array().is_some_and(|members| members.iter().any(|m| m == a))
                            && seen.insert(a)))
                    })
                } else {
                    values.as_array().is_some_and(|messages| !messages.is_empty() && messages.iter().all(|m|
                        m["content"].is_string() && m["role"].as_str().is_some_and(|r| ["system", "user", "assistant", "tool"].contains(&r))))
                };
                if !valid {
                    tracing::warn!("[plugin:{}] invalid {event} result ignored", plugin.name);
                    continue;
                }
                current[field] = values.clone();
                last = Some(current.clone());
            }
        }
        last
    }

    pub fn run_command(&self, input: &str) -> Option<String> {
        self.run_command_context(input, &JsonValue::Null)
    }

    pub fn run_command_context(&self, input: &str, context: &JsonValue) -> Option<String> {
        self.try_run_command_context(input, context).unwrap_or_else(|_| Some(String::new()))
    }

    pub fn try_run_command_context(&self, input: &str, context: &JsonValue) -> Result<Option<String>, String> {
        let Some(rest) = input.trim().strip_prefix('/') else { return Ok(None) };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or_default().to_lowercase();
        let args = parts.next().unwrap_or("").trim_start();
        for plugin in &self.plugins {
            let Ok(nast) = plugin.lua.globals().raw_get::<mlua::Table>("nast") else { continue };
            let Ok(commands) = nast.raw_get::<mlua::Table>("commands") else { continue };
            if let Ok(Some(func)) = commands.raw_get::<Option<mlua::Function>>(cmd.as_str()) {
                let context = json_to_lua(&plugin.lua, context).unwrap_or(LuaValue::Nil);
                return match bounded(&plugin.lua, || func.call::<LuaValue>((args, context))) {
                    Ok(LuaValue::String(v)) => Ok(Some(v.to_string_lossy())),
                    Ok(_) => Ok(Some(String::new())),
                    Err(e) => {
                        tracing::warn!("[plugin:{}] command /{cmd} failed: {e}", plugin.name);
                        // A failed known command is consumed, never sent to a model as user text.
                        Err(format!("plugin {} command /{cmd}: {e}", plugin.name))
                    }
                };
            }
        }
        Ok(None)
    }

    pub fn infos(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| {
                let mut hooks = vec![];
                let mut commands = vec![];
                if let Ok(nast) = p.lua.globals().raw_get::<mlua::Table>("nast") {
                    if let Ok(Some(t)) = nast.get::<Option<mlua::Table>>("hooks") {
                        for pair in t.pairs::<String, mlua::Table>() {
                            if let Ok((k, _)) = pair {
                                hooks.push(k);
                            }
                        }
                    }
                    if let Ok(Some(t)) = nast.get::<Option<mlua::Table>>("commands") {
                        for pair in t.pairs::<String, mlua::Function>() {
                            if let Ok((k, _)) = pair {
                                commands.push(k);
                            }
                        }
                    }
                }
                PluginInfo {
                    name: p.name.clone(),
                    hooks,
                    commands,
                }
            })
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name.clone()).collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

fn json_to_lua(lua: &Lua, v: &JsonValue) -> mlua::Result<LuaValue> {
    Ok(match v {
        JsonValue::Null => LuaValue::Nil,
        JsonValue::Bool(b) => LuaValue::Boolean(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                LuaValue::Integer(i)
            } else {
                LuaValue::Number(n.as_f64().unwrap_or(0.0))
            }
        }
        JsonValue::String(s) => LuaValue::String(lua.create_string(s)?),
        JsonValue::Array(arr) => {
            let t = lua.create_table()?;
            for (i, item) in arr.iter().enumerate() {
                t.set(i + 1, json_to_lua(lua, item)?)?;
            }
            LuaValue::Table(t)
        }
        JsonValue::Object(map) => {
            let t = lua.create_table()?;
            for (k, item) in map {
                t.set(k.as_str(), json_to_lua(lua, item)?)?;
            }
            LuaValue::Table(t)
        }
    })
}

/// Lua 表 → JSON（数组/对象自适应；仅用于 prompt_built 重写）。
fn lua_to_json(lua: &Lua, v: &LuaValue) -> Option<JsonValue> {
    lua_to_json_depth(lua, v, 0)
}

fn lua_to_json_depth(lua: &Lua, v: &LuaValue, depth: usize) -> Option<JsonValue> {
    if depth > 64 { return None; }
    Some(match v {
        LuaValue::Nil => JsonValue::Null,
        LuaValue::Boolean(b) => JsonValue::Bool(*b),
        LuaValue::Integer(i) => JsonValue::Number((*i).into()),
        LuaValue::Number(f) => serde_json::Number::from_f64(*f).map(JsonValue::Number)?,
        LuaValue::String(s) => JsonValue::String(s.to_string_lossy().to_string()),
        LuaValue::Table(t) => {
            let len = t.raw_len();
            if len > 0 {
                let mut arr = Vec::with_capacity(len);
                for i in 1..=len {
                    let item = t.raw_get::<LuaValue>(i).ok()?;
                    arr.push(lua_to_json_depth(lua, &item, depth + 1)?);
                }
                JsonValue::Array(arr)
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.clone().pairs::<String, LuaValue>() {
                    let (k, val) = pair.ok()?;
                    map.insert(k, lua_to_json_depth(lua, &val, depth + 1)?);
                }
                JsonValue::Object(map)
            }
        }
        _ => return None,
    })
}

/// 生成管线钩子 trait：Rust 扩展点（与 Lua 插件同等地位）。
pub trait GenerationHook: Send + Sync {
    fn name(&self) -> &str;
    /// 事件名见模块注释
    fn on_event(&self, event: &str, data: &JsonValue) -> Option<String>;
}

/// 插件宿主：专用线程 + 限时派发。
#[derive(Clone)]
pub struct PluginHost {
    tx: std::sync::mpsc::Sender<Cmd>,
    timeout: Duration,
    rust_hooks: Vec<std::sync::Arc<dyn GenerationHook>>,
    dir: std::path::PathBuf,
}

impl PluginHost {
    pub fn new(dir: std::path::PathBuf, kv_path: std::path::PathBuf) -> Self {
        let timeout = Duration::from_secs(
            std::env::var("NAST_PLUGIN_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10)
                .max(1),
        );
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let thread_dir = dir.clone();
        let thread_kv = kv_path.clone();
        std::thread::Builder::new()
            .name("nast-plugins".into())
            .spawn(move || {
                let mut manager = PluginManager::new().with_kv(
                    Arc::new(Mutex::new(BTreeMap::new())),
                    thread_kv,
                );
                // 启动时自动扫描
                let _ = reload_dir(&mut manager, &thread_dir);
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        Cmd::SetToast { toast } => manager.set_toast(toast),
                        Cmd::LoadDir { dir, kv_path, reply } => {
                            let _ = reply.send(reload_dir_with_kv(&mut manager, &dir, &kv_path));
                        }
                        Cmd::Dispatch { event, data, reply } => {
                            let _ = reply.send(manager.dispatch(&event, &data));
                        }
                        Cmd::DispatchJson { event, data, reply } => {
                            let _ = reply.send(manager.dispatch_json(&event, &data));
                        }
                        Cmd::RunCommand { input, context, reply } => {
                        let _ = reply.send(manager.try_run_command_context(&input, &context));
                        }
                        Cmd::List { reply } => {
                            let _ = reply.send(manager.infos());
                        }
                    }
                }
            })
            .expect("spawn plugin thread");
        let _ = thread_dir;
        Self {
            tx,
            timeout,
            rust_hooks: Vec::new(),
            dir,
        }
    }

    /// 接线 toast 回调（EventHub）。
    pub fn set_toast(&self, toast: ToastFn) {
        let _ = self.tx.send(Cmd::SetToast { toast: Some(toast) });
    }

    /// 扫描插件目录并重载。
    pub fn load_dir(&self) -> PluginResult<Vec<String>> {
        // 线程持 kv；这里传路径仅供重置
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.tx
            .send(Cmd::LoadDir {
                dir: self.dir.clone(),
                kv_path: std::path::PathBuf::new(),
                reply: reply_tx,
            })
            .map_err(|e| PluginError::Custom(e.to_string()))?;
        match reply_rx.recv() {
            Ok(Ok(names)) => Ok(names),
            Ok(Err(e)) => Err(PluginError::Custom(e)),
            Err(_) => Err(PluginError::Custom("plugin thread timed out".into())),
        }
    }

    /// 派发事件（限时；超时放行）。
    pub fn dispatch(&self, event: &str, data: &JsonValue) -> Option<String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self
            .tx
            .send(Cmd::Dispatch {
                event: event.to_string(),
                data: data.clone(),
                reply: reply_tx,
            })
            .is_err()
        {
            return None;
        }
        let lua_result = match reply_rx.recv() {
            Ok(transforms) => transforms.into_iter().last().map(|(_, t)| t),
            Err(_) => {
                tracing::warn!("[plugins] dispatch '{event}' timed out after {:?}", self.timeout);
                None
            }
        };
        if let Some(text) = lua_result {
            return Some(text);
        }
        for hook in &self.rust_hooks {
            if let Some(text) = hook.on_event(event, data) {
                return Some(text);
            }
        }
        None
    }

    /// 结构化派发（prompt_built）。
    pub fn dispatch_json(&self, event: &str, data: &JsonValue) -> Option<JsonValue> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self
            .tx
            .send(Cmd::DispatchJson {
                event: event.to_string(),
                data: data.clone(),
                reply: reply_tx,
            })
            .is_err()
        {
            return None;
        }
        match reply_rx.recv() {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("[plugins] dispatch_json '{event}' timed out");
                None
            }
        }
    }

    /// 斜杠命令（未命中返回 None）。
    pub fn run_command(&self, input: &str) -> Option<String> {
        self.run_command_context(input, &JsonValue::Null)
    }

    pub fn run_command_context(&self, input: &str, context: &JsonValue) -> Option<String> {
        self.try_run_command_context(input, context).unwrap_or_else(|_| Some(String::new()))
    }

    pub fn try_run_command_context(&self, input: &str, context: &JsonValue) -> Result<Option<String>, String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.tx
            .send(Cmd::RunCommand {
                input: input.to_string(),
                context: context.clone(),
                reply: reply_tx,
            })
            .map_err(|_| "plugin worker unavailable".to_string())?;
        match reply_rx.recv() {
            Ok(v) => v,
            Err(_) => {
                Err("plugin worker disconnected".into())
            }
        }
    }

    /// 插件清单（Lua；Rust 钩子另计）。
    pub fn list(&self) -> Vec<PluginInfo> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self.tx.send(Cmd::List { reply: reply_tx }).is_err() {
            return vec![];
        }
        reply_rx.recv().unwrap_or_default()
    }

    pub fn register_rust_hook(&mut self, hook: std::sync::Arc<dyn GenerationHook>) {
        self.rust_hooks.push(hook);
    }

    pub fn lua_names(&self) -> Vec<String> {
        self.list().into_iter().map(|p| p.name).collect()
    }
}

fn reload_dir(manager: &mut PluginManager, dir: &std::path::Path) -> Result<Vec<String>, String> {
    let mut fresh = PluginManager::new();
    // 保留 KV 与 toast
    let kv = manager.kv.clone();
    let kv_path = manager.kv_path.clone().unwrap_or_default();
    let toast = manager.toast.clone();
    fresh.kv = kv;
    fresh.kv_path = Some(kv_path);
    fresh.toast = toast;
    let mut loaded = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plugin")
                    .to_string();
                match std::fs::read_to_string(&path) {
                    Ok(code) => match fresh.load(&name, &code) {
                        Ok(()) => loaded.push(name),
                        Err(e) => tracing::warn!("[plugin:{name}] load failed: {e}"),
                    },
                    Err(e) => tracing::warn!("[plugin:{name}] read failed: {e}"),
                }
            }
        }
    }
    *manager = fresh;
    Ok(loaded)
}

fn reload_dir_with_kv(
    manager: &mut PluginManager,
    dir: &std::path::Path,
    kv_path: &std::path::Path,
) -> Result<Vec<String>, String> {
    if !kv_path.as_os_str().is_empty() {
        manager.kv_path = Some(kv_path.to_path_buf());
    }
    reload_dir(manager, dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_side_on_and_transform() {
        let mut pm = PluginManager::new();
        pm.load(
            "t",
            r#"
nast.on("user_input", function(dataJson)
  local text = string.match(dataJson, '"text":"([^"]*)"') or ""
  if string.find(text, "badword") then
    return string.gsub(text, "badword", "[censored]")
  end
  return nil
end)
"#,
        )
        .unwrap();
        let t = pm.dispatch("user_input", &serde_json::json!({"text": "a badword here"}));
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].1, "a [censored] here");
        assert!(pm
            .dispatch("user_input", &serde_json::json!({"text": "clean"}))
            .is_empty());
    }

    #[test]
    fn commands() {
        let mut pm = PluginManager::new();
        pm.load(
            "cmd",
            r#"
nast.register_command("echo", function(args) return "echo:" .. args end)
nast.register_command("/mute", function() return nil end)
"#,
        )
        .unwrap();
        assert_eq!(pm.run_command("/echo hi there").as_deref(), Some("echo:hi there"));
        assert_eq!(pm.run_command("/mute").as_deref(), Some(""));
        assert_eq!(pm.run_command("/unknown x"), None);
        assert_eq!(pm.run_command("plain text"), None);
    }

    #[test]
    fn kv_persisted_across_reload() {
        let dir = tempfile::tempdir().unwrap();
        let kv_path = dir.path().join("plugin_vars.json");
        {
            let mut pm = PluginManager::new().with_kv(Arc::new(Mutex::new(BTreeMap::new())), kv_path.clone());
            pm.load("kv", r#"nast.set_var("counter", 42)"#).unwrap();
        }
        {
            let mut pm = PluginManager::new().with_kv(Arc::new(Mutex::new(BTreeMap::new())), kv_path);
            pm.load("kv", "").unwrap();
            let v: i64 = pm.plugins[0]
                .lua
                .load("return nast.get_var('counter')")
                .eval()
                .unwrap();
            assert_eq!(v, 42);
        }
    }

    #[test]
    fn json_roundtrip_table() {
        let lua = Lua::new();
        let v = lua
            .load("return {messages = {{role='system', content='a'}, {role='user', content='b'}}, n = 3}")
            .eval::<LuaValue>()
            .unwrap();
        let j = lua_to_json(&lua, &v).unwrap();
        assert_eq!(j["n"], 3);
        assert_eq!(j["messages"][0]["role"], "system");
        assert_eq!(j["messages"][1]["content"], "b");
    }

    #[test]
    fn syntax_error_reported() {
        let mut pm = PluginManager::new();
        let err = pm.load("broken", "this is not lua )(");
        assert!(err.is_err());
    }

    #[test]
    fn infos_lists_hooks_and_commands() {
        let mut pm = PluginManager::new();
        pm.load(
            "meta",
            r#"
nast.on("user_input", function() end)
nast.register_command("ping", function() return "pong" end)
"#,
        )
        .unwrap();
        let infos = pm.infos();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].hooks, vec!["user_input"]);
        assert_eq!(infos[0].commands, vec!["ping"]);
    }
}

#[cfg(test)]
mod host_tests {
    use super::*;

    #[test]
    fn interrupted_callbacks_preserve_chain_and_worker_can_reload() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.lua"), "nast.on('user_input', function() while true do pcall(function() while true do end end) end end)").unwrap();
        std::fs::write(dir.path().join("b.lua"), "nast.on('user_input', function(d) return nast.json_decode(d).text .. 'b' end)").unwrap();
        let host = PluginHost::new(dir.path().into(), dir.path().join("kv.json"));
        let started = Instant::now();
        assert_eq!(host.dispatch("user_input", &serde_json::json!({"text":"a"})).as_deref(), Some("ab"));
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(host.list().len(), 2);
        assert_eq!(host.load_dir().unwrap().len(), 2);
        assert_eq!(host.dispatch("user_input", &serde_json::json!({"text":"c"})).as_deref(), Some("cb"));
    }

    #[test]
    fn queue_chain_skips_missing_hooks_and_rejects_invalid_replacements() {
        let mut pm = PluginManager::new();
        pm.load("a", "nast.on('other', function() end)").unwrap();
        pm.load("b", "nast.on('group_queue', function(d) return {'b.png','a.png'} end)").unwrap();
        pm.load("c", "nast.on('group_queue', function(d) local q=nast.json_decode(d).queue; assert(q[1]=='b.png'); return {'outsider'} end)").unwrap();
        let data = serde_json::json!({"queue":["a.png"],"eligible_members":["a.png","b.png"],"task_id":"test"});
        assert_eq!(pm.dispatch_json("group_queue", &data).unwrap()["queue"], serde_json::json!(["b.png","a.png"]));
        pm.load("d", "nast.on('group_queue', function() return {} end)").unwrap();
        assert_eq!(pm.dispatch_json("group_queue", &data).unwrap()["queue"], serde_json::json!([]));
    }

    #[test]
    fn command_context_and_cyclic_return_are_bounded() {
        let mut pm = PluginManager::new();
        pm.load("a", "nast.register_command('ctx', function(args,ctx) return ctx.group_id .. ':' .. args end); nast.on('prompt_built', function() local t={};t.t=t;return t end)").unwrap();
        assert_eq!(pm.run_command_context("/ctx\thello", &serde_json::json!({"group_id":"g"})).as_deref(), Some("g:hello"));
        assert!(pm.dispatch_json("prompt_built", &serde_json::json!({"messages":[]})).is_none());
    }

    #[test]
    fn host_dispatch_latency_with_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("no-such-plugins");
        let host = PluginHost::new(plugins_dir, dir.path().join("kv.json"));
        let t = std::time::Instant::now();
        let r = host.dispatch("user_input", &serde_json::json!({"text": "x"}));
        assert!(r.is_none());
        assert!(t.elapsed() < std::time::Duration::from_secs(3), "took {:?}", t.elapsed());
    }

    #[test]
    fn host_loads_plugin_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        let plugins_dir = dir.path().join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("hi.lua"),
            "nast.on(\"user_input\", function(d) return \"hi!\" end)",
        )
        .unwrap();
        let host = PluginHost::new(plugins_dir, dir.path().join("kv.json"));
        let r = host.dispatch("user_input", &serde_json::json!({"text": "x"}));
        assert_eq!(r.as_deref(), Some("hi!"));
    }
}
