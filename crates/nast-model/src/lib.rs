//! nast-model: ST 1.18.0 (HEAD 8172dcd0) 同构数据类型。
//!
//! 命名与字段与 SillyTavern 客户端/服务端文件格式逐字段对齐（snake_case JSON），
//! 保证卡片/世界书/聊天/settings 文件可直接互换。类型按 refrence/SillyTavern
//! 源码行为契约编写，字段注释标注来源。

pub mod card;
pub mod compat;
pub mod chat;
pub mod group;
pub mod persona;
pub mod preset;
pub mod regex_script;
pub mod settings;
pub mod world;

pub use card::*;
pub use chat::*;
pub use group::*;
pub use persona::*;
pub use preset::*;
pub use regex_script::*;
pub use settings::*;
pub use world::*;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid value for {field}: {reason}")]
    InvalidValue { field: &'static str, reason: String },
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ModelResult<T> = Result<T, ModelError>;
