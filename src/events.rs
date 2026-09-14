//! 事件名常量：对齐 refrence/SillyTavern/public/scripts/events.js 的
//! event_types 实际字符串值（1.18.0，注意大小写不统一是 ST 原样）。

pub const GENERATION_STARTED: &str = "generation_started";
pub const GENERATION_AFTER_COMMANDS: &str = "GENERATION_AFTER_COMMANDS"; // 大写原样
pub const GENERATE_AFTER_DATA: &str = "generate_after_data";
pub const MESSAGE_SENT: &str = "message_sent";
pub const MESSAGE_RECEIVED: &str = "message_received";
pub const MESSAGE_DELETED: &str = "message_deleted";
pub const MESSAGE_UPDATED: &str = "message_updated";
pub const MESSAGE_SWIPED: &str = "message_swiped";
pub const USER_MESSAGE_RENDERED: &str = "user_message_rendered";
pub const CHARACTER_MESSAGE_RENDERED: &str = "character_message_rendered";
pub const STREAM_TOKEN_RECEIVED: &str = "stream_token_received";
pub const GENERATION_STOPPED: &str = "generation_stopped";
pub const GENERATION_ENDED: &str = "generation_ended";
pub const IMPERSONATE_READY: &str = "impersonate_ready";
pub const CHAT_ID_CHANGED: &str = "chat_id_changed";
pub const MORE_MESSAGES_LOADED: &str = "more_messages_loaded";
pub const GROUP_MEMBER_DRAFTED: &str = "group_member_drafted";
pub const GROUP_WRAPPER_STARTED: &str = "group_wrapper_started";
pub const GROUP_WRAPPER_FINISHED: &str = "group_wrapper_finished";
pub const SETTINGS_UPDATED: &str = "settings_updated";
pub const CHARACTER_MESSAGE_EDITED: &str = "character_message_edited";
pub const USER_MESSAGE_EDITED: &str = "user_message_edited";
pub const CHAT_CHANGED: &str = "CHAT_CHANGED";
pub const TOAST: &str = "toast";
