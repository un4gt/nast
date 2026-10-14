//! Wait for the dedicated Lua worker without blocking an Actix request worker.
use serde_json::Value;
use nast_plugin::PluginHost;
use std::sync::Mutex;

pub async fn dispatch(plugins: &Mutex<PluginHost>, event: &str, data: Value) -> Option<String> {
    let host = plugins.lock().unwrap().clone();
    let event = event.to_string();
    tokio::task::spawn_blocking(move || host.dispatch(&event, &data)).await.ok().flatten()
}

pub async fn dispatch_json(plugins: &Mutex<PluginHost>, event: &str, data: Value) -> Option<Value> {
    let host = plugins.lock().unwrap().clone();
    let event = event.to_string();
    tokio::task::spawn_blocking(move || host.dispatch_json(&event, &data)).await.ok().flatten()
}

pub async fn command(plugins: &Mutex<PluginHost>, input: String, context: Value) -> Result<Option<String>, String> {
    let host = plugins.lock().unwrap().clone();
    tokio::task::spawn_blocking(move || host.try_run_command_context(&input, &context)).await.map_err(|e|e.to_string())?
}
