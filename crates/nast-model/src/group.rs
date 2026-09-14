//! 群组对象（groups/<uuid>.json；refrence/SillyTavern/public/scripts/group-chats.js）。

use serde::{Deserialize, Serialize};

/// group_activation_strategy
pub const GA_NATURAL: i64 = 0;
pub const GA_LIST: i64 = 1;
pub const GA_MANUAL: i64 = 2;
pub const GA_POOLED: i64 = 3;

/// group_generation_mode
pub const GG_SWAP: i64 = 0;
pub const GG_APPEND: i64 = 1;
pub const GG_APPEND_DISABLED: i64 = 2;

pub const DEFAULT_AUTO_MODE_DELAY: i64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    /// 成员 avatar 文件名，手工排序
    pub members: Vec<String>,
    pub avatar_url: String,
    pub allow_self_responses: bool,
    pub hide_muted_sprites: bool,
    pub activation_strategy: i64,
    pub generation_mode: i64,
    pub disabled_members: Vec<String>,
    pub fav: bool,
    pub chat_id: String,
    pub chats: Vec<String>,
    pub auto_mode_delay: i64,
    pub date_last_chat: i64,
    /// APPEND 拼接模板
    pub generation_mode_join_prefix: Option<String>,
    pub generation_mode_join_suffix: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            members: vec![],
            avatar_url: String::new(),
            allow_self_responses: false,
            hide_muted_sprites: false,
            activation_strategy: GA_NATURAL,
            generation_mode: GG_SWAP,
            disabled_members: vec![],
            fav: false,
            chat_id: String::new(),
            chats: vec![],
            auto_mode_delay: DEFAULT_AUTO_MODE_DELAY,
            date_last_chat: 0,
            generation_mode_join_prefix: None,
            generation_mode_join_suffix: None,
            extra: Default::default(),
        }
    }
}
