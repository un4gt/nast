//! nast-plugin：mlua 插件层（服务端运行）。
//!
//! 插件以 Lua 脚本注册事件钩子与命令，通过 nast API 表访问宿主能力。
//! 所有 Lua 执行固定在一条专用 OS 线程上（Lua !Send），每次派发限时
//! （NAST_PLUGIN_TIMEOUT_SECS，默认 10s），超时放行不阻塞生成管线。
//!
//! 插件 API（宿主提供，无需样板代码）：
//! - nast.on(event, fn)               注册事件钩子（可多次注册）
//! - nast.register_command(name, fn)  注册斜杠命令 /name args... → fn(args)
//! - nast.get_var(key) / set_var      插件级 KV（plugin_vars.json 持久化）
//! - nast.log(...)                    日志
//! - nast.toast(message, type)        发送前端 toast（hub 广播）
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
use std::time::Duration;

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
        reply: std::sync::mpsc::Sender<Option<String>>,
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
    toast: Option<ToastFn>,
}

struct Plugin {
    name: String,
    lua: Lua,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: vec![],
            kv: Arc::new(Mutex::new(BTreeMap::new())),
            kv_path: None,
            toast: None,
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
        self.toast = toast;
    }

    /// 从源码加载插件（宿主注入 nast API 表）。
    pub fn load(&mut self, name: &str, code: &str) -> PluginResult<()> {
        let lua = Lua::new();
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
            let nast: mlua::Table = lua.globals().get("nast")?;
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
            let nast: mlua::Table = lua.globals().get("nast")?;
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
            lua.create_function(move |_, (key, value): (String, LuaValue)| {
                let full = format!("plugin:{plugin_name}:{key}");
                let json = lua_value_to_json(value);
                kv_write.lock().unwrap().insert(full, json);
                // 写穿落盘（小文件，直接全量）
                if let Some(path) = &kv_write_path {
                    if let Ok(pretty) = serde_json::to_string(&*kv_write.lock().unwrap()) {
                        let tmp = path.with_extension("tmp");
                        if std::fs::write(&tmp, pretty).is_ok() {
                            let _ = std::fs::rename(&tmp, path);
                        }
                    }
                }
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
                if let Some(cb) = &toast_cb {
                    cb(&message, &kind);
                }
                Ok(())
            })?,
        )?;

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
        lua.load(code).set_name(name).exec()?;

        self.plugins.push(Plugin {
            name: name.to_string(),
            lua,
        });
        Ok(())
    }

    /// 派发事件，收集 (插件名, 转换文本)。
    pub fn dispatch(&self, event: &str, data: &JsonValue) -> Vec<(String, String)> {
        let mut transforms = Vec::new();
        for plugin in &self.plugins {
            let hooks: mlua::Table = match plugin.lua.globals().get::<mlua::Table>("nast") {
                Ok(t) => t,
                Err(_) => continue,
            };
            let list = match hooks.get::<Option<mlua::Table>>("hooks") {
                Ok(Some(t)) => match t.get::<Option<mlua::Table>>(event) {
                    Ok(Some(l)) => l,
                    _ => continue,
                },
                _ => continue,
            };
            for pair in list.sequence_values::<mlua::Function>() {
                if let Ok(func) = pair {
                    let data_str = data.to_string();
                    let arg = match plugin.lua.create_string(&data_str) {
                        Ok(s) => LuaValue::String(s),
                        Err(_) => continue,
                    };
                    match func.call::<LuaValue>(arg) {
                        Ok(result) => {
                            if let Some(s) = result.as_str() {
                                transforms.push((plugin.name.clone(), s.to_string()));
                            }
                        }
                        Err(e) => tracing::warn!("[plugin:{}] hook {event} failed: {e}", plugin.name),
                    }
                }
            }
        }
        transforms
    }

    /// 结构化派发（prompt_built：返回 Lua 表 → JSON）。
    pub fn dispatch_json(&self, event: &str, data: &JsonValue) -> Option<JsonValue> {
        for plugin in &self.plugins {
            let hooks: mlua::Table = plugin.lua.globals().get::<mlua::Table>("nast").ok()?;
            let list = hooks
                .get::<Option<mlua::Table>>("hooks")
                .ok()
                .flatten()?
                .get::<Option<mlua::Table>>(event)
                .ok()
                .flatten()?;
            for pair in list.sequence_values::<mlua::Function>() {
                let Ok(func) = pair else { continue };
                let arg = LuaValue::String(plugin.lua.create_string(&data.to_string()).ok()?);
                if let Ok(result) = func.call::<LuaValue>(arg) {
                    if !matches!(result, LuaValue::Nil) {
                        let json = lua_to_json(&plugin.lua, &result);
                        // json_encode 产物（字符串）再解一层
                        if let Some(JsonValue::String(text)) = &json {
                            if let Ok(parsed) = serde_json::from_str::<JsonValue>(text) {
                                return Some(parsed);
                            }
                        }
                        return json;
                    }
                }
            }
        }
        None
    }

    /// 斜杠命令："/cmd args…" → 命中则返回命令结果（可为空串 = 吞掉消息）。
    pub fn run_command(&self, input: &str) -> Option<String> {
        let trimmed = input.trim();
        let rest = trimmed.strip_prefix('/')?;
        let (cmd, args) = match rest.split_once(' ') {
            Some((c, a)) => (c.to_lowercase(), a.to_string()),
            None => (rest.to_lowercase(), String::new()),
        };
        if cmd.is_empty() {
            return None;
        }
        for plugin in &self.plugins {
            let nast: mlua::Table = plugin.lua.globals().get::<mlua::Table>("nast").ok()?;
            let commands: mlua::Table = nast.get("commands").ok()?;
            if let Ok(Some(func)) = commands.get::<Option<mlua::Function>>(cmd.clone()) {
                return match func.call::<LuaValue>(args.clone()) {
                    Ok(LuaValue::Nil) => Some(String::new()),
                    Ok(v) => v.as_str().map(|s| s.to_string()),
                    Err(e) => {
                        tracing::warn!("[plugin:{}] command /{cmd} failed: {e}", plugin.name);
                        None
                    }
                };
            }
        }
        None
    }

    pub fn infos(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| {
                let mut hooks = vec![];
                let mut commands = vec![];
                if let Ok(nast) = p.lua.globals().get::<mlua::Table>("nast") {
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

fn lua_value_to_json(v: LuaValue) -> JsonValue {
    match v {
        LuaValue::Nil => JsonValue::Null,
        LuaValue::Boolean(b) => JsonValue::Bool(b),
        LuaValue::Integer(i) => JsonValue::Number(i.into()),
        LuaValue::Number(f) => {
            serde_json::Number::from_f64(f).map(JsonValue::Number).unwrap_or(JsonValue::Null)
        }
        LuaValue::String(s) => JsonValue::String(s.to_string_lossy().to_string()),
        _ => JsonValue::Null,
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
                    let item = t.get::<LuaValue>(i).ok()?;
                    arr.push(lua_to_json(lua, &item)?);
                }
                JsonValue::Array(arr)
            } else {
                let mut map = serde_json::Map::new();
                for pair in t.clone().pairs::<String, LuaValue>() {
                    let (k, val) = pair.ok()?;
                    map.insert(k, lua_to_json(lua, &val)?);
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
                        Cmd::RunCommand { input, reply } => {
                            let _ = reply.send(manager.run_command(&input));
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
        match reply_rx.recv_timeout(self.timeout) {
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
        let lua_result = match reply_rx.recv_timeout(self.timeout) {
            Ok(transforms) => transforms.into_iter().next().map(|(_, t)| t),
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
        match reply_rx.recv_timeout(self.timeout) {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("[plugins] dispatch_json '{event}' timed out");
                None
            }
        }
    }

    /// 斜杠命令（未命中返回 None）。
    pub fn run_command(&self, input: &str) -> Option<String> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        self.tx
            .send(Cmd::RunCommand {
                input: input.to_string(),
                reply: reply_tx,
            })
            .ok()?;
        match reply_rx.recv_timeout(self.timeout) {
            Ok(v) => v,
            Err(_) => {
                tracing::warn!("[plugins] command timed out");
                None
            }
        }
    }

    /// 插件清单（Lua；Rust 钩子另计）。
    pub fn list(&self) -> Vec<PluginInfo> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self.tx.send(Cmd::List { reply: reply_tx }).is_err() {
            return vec![];
        }
        reply_rx.recv_timeout(self.timeout).unwrap_or_default()
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
        for entry in entries.flatten() {
            let path = entry.path();
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
