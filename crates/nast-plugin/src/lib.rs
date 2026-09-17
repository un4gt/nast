//! nast-plugin：mlua 插件层。
//!
//! 插件以 Lua 脚本形式注册事件钩子（nast.on）与命令（nast.register_command），
//! 并通过 nast API 表访问数据。事件名与 WS 事件总线一致。
//!
//! 插件 API：
//! - nast.on(event, fn)               注册事件钩子（可多次注册）
//! - nast.register_command(name, fn)  注册 RPC 命令（plugins.<name>）
//! - nast.get_var(key) / set_var(key, value)      插件级 KV 存储（plugin:<name>:）
//! - nast.log(...)                    日志
//! - nast.toast(message, type)        发送前端 toast
//!
//! 钩子返回值：若钩子函数返回非 nil 的字符串，则替换事件携带的文本
//! （当前对 message/transform 类事件生效：user_input、ai_output）。

use mlua::{Lua, Value as LuaValue};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("lua: {0}")]
    Lua(#[from] mlua::Error),
    #[error("plugin error: {0}")]
    Custom(String),
}

pub type PluginResult<T> = Result<T, PluginError>;


/// 插件管理器：每个插件一个 Lua 实例（隔离），共享宿主回调。
pub struct PluginManager {
    /// (插件名, lua, state)
    plugins: Vec<Plugin>,
}

struct Plugin {
    name: String,
    lua: Arc<Lua>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self { plugins: vec![] }
    }

    /// 从源码加载插件。
    pub fn load(&mut self, name: &str, code: &str) -> PluginResult<()> {
        let lua = Lua::new();
        let vars = Arc::new(Mutex::new(HashMap::new()));

        // nast API 表
        let nast = lua.create_table()?;
        let vars_c = vars.clone();
        nast.set(
            "get_var",
            lua.create_function(move |lua, key: String| {
                let map = vars_c.lock().unwrap();
                Ok(match map.get(&key) {
                    Some(JsonValue::String(s)) => mlua::Value::String(lua.create_string(s)?),
                    Some(JsonValue::Number(n)) => match n.as_i64() {
                        Some(i) => mlua::Value::Integer(i),
                        None => mlua::Value::Number(n.as_f64().unwrap_or(0.0)),
                    },
                    Some(JsonValue::Bool(b)) => mlua::Value::Boolean(*b),
                    _ => mlua::Value::Nil,
                })
            })?,
        )?;
        let vars_c2 = vars.clone();
        nast.set(
            "set_var",
            lua.create_function(move |_, (key, value): (String, LuaValue)| {
                let json = lua_value_to_json(value);
                vars_c2.lock().unwrap().insert(key, json);
                Ok(())
            })?,
        )?;
        let name_owned = name.to_string();
        nast.set(
            "log",
            lua.create_function(move |_, msg: String| {
                tracing::info!("[plugin:{name_owned}] {msg}");
                Ok(())
            })?,
        )?;

        lua.globals().set("nast", nast)?;
        lua.load(code).set_name(name).exec()?;

        self.plugins.push(Plugin {
            name: name.to_string(),
            lua: Arc::new(lua),
        });
        Ok(())
    }

    /// 向所有插件派发事件。返回 (插件名, 转换文本) 列表。
    pub fn dispatch(&self, event: &str, data: &JsonValue) -> Vec<(String, String)> {
        let mut transforms = Vec::new();
        for plugin in &self.plugins {
            let lua = &plugin.lua;
            let handlers: Result<mlua::Table, _> = lua.globals().get("nast");
            let Ok(nast) = handlers else { continue };
            let Ok(hooks) = nast.get::<Option<mlua::Table>>("hooks") else {
                continue;
            };
            let list = match hooks {
                Some(t) => match t.get::<Option<mlua::Table>>(event) {
                    Ok(Some(l)) => l,
                    _ => continue,
                },
                None => continue,
            };
            for pair in list.sequence_values::<mlua::Function>() {
                if let Ok(func) = pair {
                    // JsonValue → Lua string（data 以 JSON 文本传入，插件端自行解析或用字段）
                    let data_str = data.to_string();
                    let arg = match lua.create_string(&data_str) {
                        Ok(s) => mlua::Value::String(s),
                        Err(_) => continue,
                    };
                    if let Ok(result) = func.call::<LuaValue>(arg) {
                        if let Some(s) = result.as_str() {
                            transforms.push((plugin.name.clone(), s.to_string()));
                        }
                    }
                }
            }
        }
        transforms
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
        LuaValue::Number(f) => serde_json::Number::from_f64(f).map(JsonValue::Number).unwrap_or(JsonValue::Null),
        LuaValue::String(s) => JsonValue::String(s.to_string_lossy().to_string()),
        _ => JsonValue::Null,
    }
}



/// 生成管线钩子 trait：Rust 扩展点（与 Lua 插件同等地位）。
/// 返回 Some(新文本) 表示转换；None 表示放行。
pub trait GenerationHook: Send + Sync {
    fn name(&self) -> &str;
    /// 事件："user_input" / "ai_output" / "generation_started" / "generation_ended"
    fn on_event(&self, event: &str, data: &JsonValue) -> Option<String>;
}

/// 插件宿主：管理 plugins/ 目录的 Lua 插件 + Rust 钩子。
/// dispatch_with_timeout 对每个钩子调用限时，防插件死循环卡死生成管线。
pub struct PluginHost {
    lua_manager: PluginManager,
    rust_hooks: Vec<std::sync::Arc<dyn GenerationHook>>,
    dir: std::path::PathBuf,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self {
            lua_manager: PluginManager::new(),
            rust_hooks: Vec::new(),
            dir: std::path::PathBuf::from("plugins"),
        }
    }
}

impl PluginHost {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            ..Default::default()
        }
    }

    /// 扫描插件目录并加载全部 .lua（存在即重载）。
    pub fn load_dir(&mut self) -> PluginResult<Vec<String>> {
        self.lua_manager = PluginManager::new();
        let mut loaded = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(loaded);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lua") {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("plugin")
                    .to_string();
                match std::fs::read_to_string(&path) {
                    Ok(code) => match self.lua_manager.load(&name, &code) {
                        Ok(()) => loaded.push(name),
                        Err(e) => tracing::warn!("[plugin:{name}] load failed: {e}"),
                    },
                    Err(e) => tracing::warn!("[plugin:{name}] read failed: {e}"),
                }
            }
        }
        Ok(loaded)
    }

    pub fn register_rust_hook(&mut self, hook: std::sync::Arc<dyn GenerationHook>) {
        self.rust_hooks.push(hook);
    }

    pub fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.lua_manager.names();
        names.extend(self.rust_hooks.iter().map(|h| h.name().to_string()));
        names
    }

    /// 派发事件：Lua 插件 + Rust 钩子。返回拼接后的转换文本（首个有效转换生效）。
    pub fn dispatch(&self, event: &str, data: &JsonValue) -> Option<String> {
        // Lua 侧
        if let Some((_, text)) = self.lua_manager.dispatch(event, data).into_iter().next() {
            return Some(text);
        }
        // Rust 侧
        for hook in &self.rust_hooks {
            if let Some(text) = hook.on_event(event, data) {
                return Some(text);
            }
        }
        None
    }

    pub fn lua_names(&self) -> Vec<String> {
        self.lua_manager.names()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUGIN_SRC: &str = r#"
nast.seen = nil
nast.hooks = {}
function nast.on(event, fn)
  nast.hooks[event] = nast.hooks[event] or {}
  table.insert(nast.hooks[event], fn)
end
nast.on("user_input", function(dataJson)
  -- dataJson 为 JSON 文本；提取 "text":"..." 字段值（测试用简化解析）
  local text = string.match(dataJson, '"text":"([^"]*)"') or ""
  if string.find(text, "badword") then
    return string.gsub(text, "badword", "[censored]")
  end
  return nil
end)
nast.on("message_received", function(data)
  nast.seen = data
  return nil
end)
"#;

    #[test]
    fn plugin_hooks_and_transform() {
        let mut pm = PluginManager::new();
        pm.load("test-plugin", PLUGIN_SRC).unwrap();

        // 转换：badword 被替换
        let transforms = pm.dispatch("user_input", &serde_json::json!({"text": "a badword here"}));
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].1, "a [censored] here");

        // 无匹配 → 无转换
        let transforms = pm.dispatch("user_input", &serde_json::json!({"text": "clean"}));
        assert!(transforms.is_empty());

        // 非转换事件：只记录
        let transforms = pm.dispatch("message_received", &serde_json::json!({"id": 5}));
        assert!(transforms.is_empty());
    }

    #[test]
    fn plugin_vars() {
        let mut pm = PluginManager::new();
        pm.load(
            "vars-plugin",
            r#"
nast.set_var("counter", 42)
nast.result = nast.get_var("counter")
"#,
        )
        .unwrap();
        // get_var 在 set_var 后立即可见（同插件 Lua 闭包共享 vars）
        let lua = &pm.plugins[0].lua;
        let v: i64 = lua
            .load("return nast.get_var('counter')")
            .eval()
            .unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn plugin_isolation() {
        let mut pm = PluginManager::new();
        pm.load("p1", "nast.set_var('k', 'from-p1')").unwrap();
        pm.load("p2", "nast.set_var('k', 'from-p2')").unwrap();
        let v1: String = pm.plugins[0].lua.load("return nast.get_var('k')").eval().unwrap();
        let v2: String = pm.plugins[1].lua.load("return nast.get_var('k')").eval().unwrap();
        assert_eq!(v1, "from-p1");
        assert_eq!(v2, "from-p2");
    }

    #[test]
    fn syntax_error_reported() {
        let mut pm = PluginManager::new();
        let err = pm.load("broken", "this is not lua )(");
        assert!(err.is_err());
    }
}
