//! 生成状态机：nast 版 Generate()。
//!
//! 行为契约（对照 refrence/SillyTavern/public/script.js Generate 1.18.0）：
//! - 类型字符串 normal/swipe/impersonate/continue/regenerate/quiet
//! - sendMessageAsUser 语义：用户消息先落盘 + message_sent 事件；用户消息无 swipes
//! - swipe：被重roll消息从 prompt 弹出；流式原地写 swipes[swipe_id] + swipe_info；
//!   每 swipe 独立 send_date；setFirstSwipe 镜像 mes↔swipes[0]
//! - regenerate：先生成前删掉最后一条 AI 消息（失败不恢复）
//! - impersonate：结果不落消息，经 impersonate_ready 事件送输入框
//! - quiet：返回字符串，不落消息不流式
//! - 错误不写入消息文本（toast 事件），流式保留半截文本
//! - continue：nudge 模式（末尾 [Continue...] 提示）为默认；Claude prefill 可选

use nast_model::chat::{ChatFile, ChatMessage as Msg, GenerationType, MessageExtra, SwipeInfo};
use nast_model::preset::{OaiSettings, CC_DUMMY_ID};
use nast_providers::{
    ChatMessage as ProviderMessage, GenRequest,
};
use serde_json::{json, Value};

use crate::prompt_bridge::{ExampleBlock, HistoryMessage, InChatInjection};
use crate::state::EventHub;
use nast_engine::prompt::AssembleOutput;

/// 生成参数。
pub struct GroupContext {
    pub group: nast_model::group::Group,
    pub members: Vec<(nast_engine::group_chat::GroupMember, nast_model::card::Character)>,
    pub generation_id: i64,
}

pub struct GenerateParams {
    pub group: Option<GroupContext>,
    pub generation_type: GenerationType,
    pub avatar: String,
    pub chat_file: String,
    pub user_message: String,
    pub character: nast_model::card::Character,
    pub is_group: bool,
}

/// 服务端解析出的 persona（personas.js 语义）。
struct ResolvedPersona {
    name: String,
    description: String,
    /// PDP_IN_PROMPT/TOP_AN/BOTTOM_AN/AT_DEPTH/NONE（0/2/3/4/9）
    position: i64,
    depth: i64,
    role: i64,
    lorebook: Option<String>,
}

/// 服务端构建的 Author's Note（AN.js setFloatingPrompt 语义）。
struct AnNote {
    text: String,
    /// 0 = 主提示后 / 1 = 聊天内深度 / 2 = 主提示前
    position: i64,
    depth: i64,
    role: i64,
}

pub struct GenerateResult {
    pub text: String,
    pub saved: bool,
    pub routing: Value,
    pub reasoning: String,
}

pub struct GenerateSession<'a> {
    pub user: &'a nast_storage::UserData,
    pub hub: &'a EventHub,
    pub oai: OaiSettings,
    pub shared_settings: &'a tokio::sync::RwLock<Value>,
    pub global_variables: std::cell::RefCell<serde_json::Map<String, Value>>,
    pub settings_json: &'a serde_json::Value,
    pub routing: crate::routing::Routing,
    pub abort: tokio_util::sync::CancellationToken,
    /// 插件宿主（Lua + Rust 钩子）
    pub plugins: &'a std::sync::Mutex<nast_plugin::PluginHost>,
    /// 流式进度镜像（断线重连恢复）
    pub progress: std::sync::Arc<std::sync::Mutex<String>>,
}

impl<'a> GenerateSession<'a> {
    async fn commit_globals(&self, input: &crate::prompt_bridge::BridgeInput<'_>) -> Result<(), String> {
        let next = input.macro_context.borrow().vars.global.clone();
        let previous = self.global_variables.borrow().clone();
        if next == previous { return Ok(()); }
        let mut settings = self.shared_settings.write().await;
        let mut updated = settings.clone();
        if !updated["extension_settings"].is_object() { updated["extension_settings"] = json!({}); }
        if !updated["extension_settings"]["variables"].is_object() { updated["extension_settings"]["variables"] = json!({}); }
        if !updated["extension_settings"]["variables"]["global"].is_object() { updated["extension_settings"]["variables"]["global"] = json!({}); }
        let target = updated["extension_settings"]["variables"]["global"].as_object_mut().unwrap();
        for key in previous.keys().chain(next.keys()) {
            if previous.get(key) != next.get(key) {
                match next.get(key) { Some(value) => { target.insert(key.clone(), value.clone()); }, None => { target.remove(key); } }
            }
        }
        self.user.save_settings(&updated).map_err(|e| e.to_string())?;
        *settings = updated;
        *self.global_variables.borrow_mut() = next;
        Ok(())
    }

    pub async fn run(&self, params: GenerateParams) -> Result<GenerateResult, String> {
        self.hub.emit(
            "generation_started",
            json!({"type": params.generation_type.as_str(),"task_id":self.routing.task_id,"conversation":self.routing.conversation}),
        );
        self.dispatch_plugin(
            "generation_started",
            &json!({"type": params.generation_type.as_str()}),
        );
        let result = self.run_content(&params).await;
        self.hub.emit("generation_ended", json!({"task_id":self.routing.task_id,"conversation":self.routing.conversation}));
        self.dispatch_plugin("generation_ended", &json!({}));
        result
    }

    pub(crate) async fn run_content(&self, params: &GenerateParams) -> Result<GenerateResult, String> {
        match params.generation_type {
            GenerationType::Normal | GenerationType::Regenerate | GenerationType::Swipe | GenerationType::Continue => {
                self.run_message_gen(params).await
            }
            GenerationType::Impersonate => self.run_impersonate(params).await,
            GenerationType::Quiet => self.run_quiet(params).await,
        }
    }

    // ---------- 主流程：normal/swipe/regenerate ----------

    async fn run_message_gen(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let gen_started = nast_storage::message_time_stamp();
        let gen_start_instant = std::time::Instant::now();
        let mut chat = self.read_chat_for(p)?;
        ensure_integrity(&mut chat);

        // regenerate：先删最后一条 AI 消息（失败不恢复）
        if p.generation_type == GenerationType::Regenerate {
            if let Some(last) = chat.0.last() {
                let is_user = last.get("is_user").and_then(|v| v.as_bool()).unwrap_or(true);
                if !is_user {
                    chat.0.pop();
                    self.emit_message_deleted(chat.0.len());
                }
            }
        }

        // normal：sendMessageAsUser —— 用户消息先落盘（无 swipes）
        if p.generation_type == GenerationType::Normal && !p.user_message.is_empty() {
            let metadata = self.read_chat_for(p).ok().map(|c| c.metadata());
            let user_name = metadata
                .map(|m| self.resolve_persona(&m).name)
                .unwrap_or_else(|| "User".into());
            // 斜杠命令：插件注册的 /cmd 优先于 user_input 钩子；空结果 = 吞掉消息
            let mut effective = p.user_message.clone();
            if effective.starts_with('/') {
                let cmd_result = {
                    let host = self.plugins.lock().unwrap();
                    host.run_command(&effective)
                };
                if let Some(applied) = cmd_result {
                    if applied.is_empty() {
                        self.hub
                            .emit("toast", json!({"message": "command handled", "type": "info"}));
                        return Ok(GenerateResult { text: String::new(), saved: false, routing: Value::Null, reasoning:String::new() });
                    }
                    effective = applied;
                }
            }
            // 插件钩子：user_input 可改写用户消息
            let user_text = self.transform_or(&"user_input", &json!({"text": effective}), &effective);
            let scripts = self.collect_regex_scripts(&p.character, &chat.metadata());
            let user_text = nast_engine::regex_engine::get_regexed_string(&user_text,
                nast_model::regex_script::RP_USER_INPUT, &scripts, &Default::default(),
                &|text| crate::prompt_bridge::substitute_basic(text, &user_name, &p.character.name));
            let msg = Msg {
                name: user_name,
                is_user: true,
                is_system: false,
                send_date: nast_storage::message_time_stamp(),
                mes: user_text,
                extra: MessageExtra::default(),
                ..Default::default()
            };
            let v = serde_json::to_value(&msg).map_err(|e| e.to_string())?;
            chat.0.push(v);
            self.save_chat_for(p, &chat)?;
            let chat_id = chat.0.len() as i64 - 1;
            self.hub.emit("message_sent", json!(chat_id));
            self.hub.emit("user_message_rendered", json!(chat_id));
        }

        // swipe：被重roll消息从 prompt 弹出（coreChat.pop），但仍留在聊天里
        let mut prompt_history: Vec<Msg> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        if p.generation_type == GenerationType::Swipe {
            prompt_history.pop();
        }

        // 拼装
        let input = self.build_assemble_input(p, &prompt_history);
        let continuing = p.generation_type == GenerationType::Continue;
        let prior = if continuing { Some(prompt_history.last().ok_or("nothing to continue")?) } else { None };
        let use_prefill = continuing && self.oai.continue_prefill && self.oai.chat_completion_source == "claude";
        let assembled = if let Some(prior) = prior {
            if use_prefill { crate::prompt_bridge::assemble_continue_prefill(&self.oai, &input, &prior.mes, "assistant") }
            else { crate::prompt_bridge::assemble_continue_nudge(&self.oai, &input, &prior.mes) }
        } else { crate::prompt_bridge::assemble_with_macros(&self.oai, &input) };
        self.check_prompt(&assembled)?;
        // 插件钩子：prompt_built 可整体重写拼装消息
        let assembled = apply_prompt_plugin(self.plugins, assembled);
        chat.0[0]["chat_metadata"] = json!(input.metadata_after_assembly());
        self.commit_globals(&input).await?;
        self.save_chat_for(p, &chat)?;

        // provider 请求
        let provider_msgs: Vec<ProviderMessage> = assembled
            .chat
            .iter()
            .map(|m| ProviderMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                name: m.name.clone(),
            })
            .collect();
        let char_name = p.character.name.clone();
        let gen_req = GenRequest {
            messages: provider_msgs,
            model: crate::connection::model_for(&self.oai),
            temperature: self.oai.temperature,
            top_p: self.oai.top_p,
            frequency_penalty: self.oai.frequency_penalty,
            presence_penalty: self.oai.presence_penalty,
            max_tokens: self.oai.openai_max_tokens,
            stop: self.stopping_strings(&input.name1, &char_name, 4),
            stream: self.oai.stream_openai,
            // The assembler already put the continuation in its final assistant message.
            assistant_prefill: None,
            use_sysprompt: true,
            extra_headers: crate::connection::extra_headers(&self.oai),
            extra_body: crate::connection::extra_body(&self.oai),
        };

        let outcome = self.routing.execute(&gen_req, self.hub, &self.abort, &self.progress, true).await;
        let route_metadata = outcome.metadata(&self.routing);
        if !outcome.complete && outcome.text.is_empty() && outcome.reasoning.is_empty() {
            return Err(outcome.error.unwrap_or(json!({"message":"generation incomplete"})).to_string());
        }
        self.routing.commit(&mut chat, &outcome);
        let first_token_at = outcome.first_token_at;
        let reasoning_started = outcome.reasoning_started;
        let streamed = outcome.text;
        let reasoning_streamed = outcome.reasoning;

        // auto_parse：从正文剥离 <think>…</think> 进 reasoning（reasoning.js:1517+）
        let auto_parse = self.power_bool("reasoning", "auto_parse", false);
        let mut reasoning = reasoning_streamed;
        let mut streamed = streamed;
        if auto_parse {
            let (text, parsed) = extract_think_blocks(&streamed);
            if !parsed.is_empty() {
                streamed = text;
                reasoning = if reasoning.is_empty() {
                    parsed
                } else {
                    format!("{parsed}\n{reasoning}")
                };
            }
        }

        // 插件钩子：ai_output 可改写 AI 回复
        let streamed = self
            .transform_or(&"ai_output", &json!({"text": streamed, "name": char_name}), &streamed);

        // cleanUpMessage 管线（script.js:6383-6533）：停止串剥离/正则默认 pass/
        // 名字清理/endoftext 截断/fixMarkdown
        let metadata_for_scripts = self
            .read_chat_for(p).ok().map(|c| c.metadata())
            .unwrap_or_default();
        let scripts_for_cleanup =
            self.collect_regex_scripts(&p.character, &metadata_for_scripts);
        let cleaned = self.clean_up_message(
            &streamed,
            &input.name1,
            &char_name,
            false,
            false,
            &self.stopping_strings(&input.name1, &char_name, 0),
            &scripts_for_cleanup,
        );
        if cleaned.chars().count() != streamed.chars().count() {
            tracing::info!(event="generation_text_processed", task_id=%self.routing.task_id,
                before_chars=streamed.chars().count(), after_chars=cleaned.chars().count());
        }
        let streamed = cleaned;

        // 正则 display pass（markdownOnly 脚本生效）→ extra.display_text
        let display_text = {
            let user_name = input.name1.clone();
            let char_name2 = char_name.clone();
            let macro_fn =
                move |s: &str| crate::prompt_bridge::substitute_basic(s, &user_name, &char_name2);
            let params = nast_engine::regex_engine::RegexParams {
                is_markdown: true,
                ..Default::default()
            };
            let metadata = self
                .read_chat_for(p).ok().map(|c| c.metadata())
                .unwrap_or_default();
            let scripts = self.collect_regex_scripts(&p.character, &metadata);
            nast_engine::regex_engine::get_regexed_string(
                &streamed,
                nast_model::regex_script::RP_AI_OUTPUT,
                &scripts,
                &params,
                &macro_fn,
            )
        };
        let reasoning_duration = reasoning_started.map(|t| t.elapsed().as_millis() as f64);
        // time_to_first_token：请求发出 → 首个 token 的秒数（extra.time_to_first_token）
        let time_to_first_token = first_token_at.map(|t| t.duration_since(gen_start_instant).as_secs_f64());
        // 落盘：saveReply 语义
        let now = nast_storage::message_time_stamp();
        match p.generation_type {
            GenerationType::Continue => {
                let last = chat.0.last_mut().ok_or("nothing to continue")?;
                let mut message: Msg = serde_json::from_value(last.clone()).map_err(|e| e.to_string())?;
                if !message.mes.ends_with(' ') { message.mes.push_str(&self.oai.continue_postfix); }
                message.mes.push_str(&streamed);
                // Render from the full updated text; stale display_text must not hide the continuation.
                message.extra.display_text = None;
                message.extra.api = Some(match outcome.route.protocol.as_str() { "anthropic"=>"claude", "gemini"=>"makersuite", _=>"custom" }.into());
                message.extra.model = Some(outcome.route.upstream_model.clone());
                message.extra.other.insert("nast_model".into(), route_metadata.clone());
                if !reasoning.is_empty() {
                    let previous = message.extra.reasoning.take().unwrap_or_default();
                    message.extra.reasoning = Some(format!("{previous}{reasoning}"));
                }
                message.gen_finished = Some(now.clone());
                message.extra.gen_finished = Some(now.clone());
                let swipe = message.swipe_id.unwrap_or(0).max(0) as usize;
                let swipes = message.swipes.get_or_insert_with(|| vec![message.mes.clone()]);
                if let Some(text) = swipes.get_mut(swipe) { *text = message.mes.clone(); }
                if let Some(info) = message.swipe_info.as_mut().and_then(|infos| infos.get_mut(swipe)) {
                    info.gen_finished = Some(now.clone()); info.extra = message.extra.clone();
                }
                *last = json!(message);
            }
            GenerationType::Swipe => {
                // ST swipe() 语义：swipe_id 前进到最后（追加新 swipe 槽位），
                // 生成结果写入该槽位。已有 swipes 时 swipe_id = swipes.len()
                // （即新增一条），首次 swipe 从镜像的 swipes[0] 之后追加。
                let last = chat.0.last_mut().ok_or("empty chat")?;
                let msg: Msg = serde_json::from_value(last.clone()).map_err(|e| e.to_string())?;
                let mut swipes = msg.swipes.clone().unwrap_or_else(|| vec![msg.mes.clone()]);
                let mut infos = msg.swipe_info.clone().unwrap_or_default();
                let swipe_id = swipes.len(); // 新槽位（追加）
                swipes.push(streamed.clone());
                let mut extra = msg.extra.clone();
                extra.reasoning = None;
                extra.reasoning_duration = None;
                extra.reasoning_type = None;
                extra.api = Some(match outcome.route.protocol.as_str() { "anthropic"=>"claude", "gemini"=>"makersuite", _=>"custom" }.into());
                extra.model = Some(outcome.route.upstream_model.clone());
                extra.other.insert("nast_model".into(), route_metadata.clone());
                extra.gen_started = Some(gen_started.clone());
                extra.gen_finished = Some(now.clone());
                extra.display_text = Some(display_text.clone());
                // A new candidate belongs to the original group batch (ST saveReply).
                if !reasoning.is_empty() {
                    extra.reasoning = Some(reasoning.clone());
                    extra.reasoning_duration = reasoning_duration;
                    extra.reasoning_type = Some("Model".into());
                }
                if let Some(ttft) = time_to_first_token {
                    extra.time_to_first_token = Some(ttft);
                }
                let swipe_info = SwipeInfo {
                    send_date: Some(now.clone()),
                    gen_started: Some(gen_started.clone()),
                    gen_finished: Some(now.clone()),
                    extra: extra.clone(),
                };
                infos.push(swipe_info);
                let mut new_msg = msg;
                new_msg.extra = extra;
                new_msg.swipes = Some(swipes);
                new_msg.swipe_info = Some(infos);
                new_msg.swipe_id = Some(swipe_id as i64);
                new_msg.send_date = now.clone(); // 每 swipe 独立 send_date
                new_msg.mes = streamed.clone();
                new_msg.gen_started = Some(gen_started.clone());
                new_msg.gen_finished = Some(now.clone());
                *last = serde_json::to_value(&new_msg).map_err(|e| e.to_string())?;
            }
            _ => {
                // normal/regenerate：新消息 + setFirstSwipe 镜像
                let mut extra = MessageExtra::default();
                extra.api = Some(match outcome.route.protocol.as_str() { "anthropic"=>"claude", "gemini"=>"makersuite", _=>"custom" }.into());
                extra.model = Some(outcome.route.upstream_model.clone());
                extra.other.insert("nast_model".into(), route_metadata.clone());
                extra.gen_started = Some(gen_started.clone());
                extra.gen_finished = Some(now.clone());
                extra.display_text = Some(display_text.clone());
                if let Some(group) = &p.group { extra.gen_id = Some(json!(group.generation_id)); }
                if !reasoning.is_empty() {
                    extra.reasoning = Some(reasoning.clone());
                    extra.reasoning_duration = reasoning_duration;
                    extra.reasoning_type = Some("Model".into());
                }
                if let Some(ttft) = time_to_first_token {
                    extra.time_to_first_token = Some(ttft);
                }
                let msg = Msg {
                    name: char_name.clone(),
                    original_avatar: p.is_group.then(|| p.avatar.clone()),
                    force_avatar: p.is_group.then(|| format!("/thumbnail?file={}", p.avatar)),
                    is_user: false,
                    is_system: false,
                    send_date: now.clone(),
                    mes: streamed.clone(),
                    gen_started: Some(gen_started.clone()),
                    gen_finished: Some(now.clone()),
                    extra: extra.clone(),
                    // 每条新 AI 消息都有 swipes 基础设施（setFirstSwipe）
                    swipes: Some(vec![streamed.clone()]),
                    swipe_id: Some(0),
                    swipe_info: Some(vec![SwipeInfo {
                        send_date: Some(now.clone()),
                        gen_started: Some(gen_started.clone()),
                        gen_finished: Some(now.clone()),
                        extra,
                    }]),
                    ..Default::default()
                };
                let v = serde_json::to_value(&msg).map_err(|e| e.to_string())?;
                chat.0.push(v);
            }
        }
        self.save_chat_for(p, &chat)?;
        if route_metadata["status"] == "complete" { self.hub.emit("conversation_model_changed", json!({"conversation":self.routing.conversation,"state":chat.0[0]["chat_metadata"]["nast_model"]})); }
        let chat_id = chat.0.len() as i64 - 1;
        self.hub.emit("message_received", json!(chat_id));
        self.hub.emit("character_message_rendered", json!(chat_id));
        self.dispatch_plugin("message_saved", &json!({"index": chat_id}));
        Ok(GenerateResult { text: streamed, saved: true, routing: route_metadata, reasoning })
    }

    // ---------- impersonate：不落消息 ----------

    async fn run_impersonate(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let mut chat = self.read_chat_for(p)?;
        let history: Vec<Msg> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let input = self.build_assemble_input(p, &history);
        let impersonation_prompt = if self.oai.impersonation_prompt.is_empty() {
            String::new()
        } else {
            crate::prompt_bridge::substitute_basic(&self.oai.impersonation_prompt, &input.name1, &p.character.name)
        };
        let assembled = crate::prompt_bridge::assemble_impersonate(&self.oai, &input, &impersonation_prompt);
        self.check_prompt(&assembled)?;
        chat.0[0]["chat_metadata"] = json!(input.metadata_after_assembly());
        self.commit_globals(&input).await?;
        self.save_chat_for(p, &chat)?;
        let outcome = self.call_provider(&assembled, None).await?;
        self.routing.commit(&mut chat, &outcome);
        self.save_chat_for(p, &chat)?;
        let routing = outcome.metadata(&self.routing);
        if outcome.complete { self.hub.emit("conversation_model_changed", json!({"conversation":self.routing.conversation,"state":chat.0[0]["chat_metadata"]["nast_model"]})); }
        let text = outcome.text;
        // cleanUpMessage（isImpersonate：USER_INPUT 正则 pass + 停止串剥离）
        let metadata = chat.metadata();
        let scripts = self.collect_regex_scripts(&p.character, &metadata);
        let text = self.clean_up_message(
            &text,
            &input.name1,
            &p.character.name,
            true,
            false,
            &self.stopping_strings(&input.name1, &p.character.name, 0),
            &scripts,
        );
        self.hub.emit("impersonate_ready", json!(text));
        Ok(GenerateResult { text, saved: false, routing, reasoning:outcome.reasoning })
    }

    // ---------- quiet：不落消息不流式 ----------

    async fn run_quiet(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let mut chat = self.read_chat_for(p)?;
        let history: Vec<Msg> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let input = self.build_assemble_input(p, &history);
        let assembled = crate::prompt_bridge::assemble_quiet(&self.oai, &input, &p.user_message);
        self.check_prompt(&assembled)?;
        chat.0[0]["chat_metadata"] = json!(input.metadata_after_assembly());
        self.commit_globals(&input).await?;
        self.save_chat_for(p, &chat)?;
        let outcome = self.call_provider(&assembled, None).await?;
        self.routing.commit(&mut chat, &outcome);
        self.save_chat_for(p, &chat)?;
        let routing = outcome.metadata(&self.routing);
        if outcome.complete { self.hub.emit("conversation_model_changed", json!({"conversation":self.routing.conversation,"state":chat.0[0]["chat_metadata"]["nast_model"]})); }
        let text = outcome.text;
        Ok(GenerateResult { text, saved: false, routing, reasoning:outcome.reasoning })
    }


    // ---------- 公共 ----------

    fn build_assemble_input<'b>(
        &'b self, p: &'b GenerateParams, history: &'b [Msg],
    ) -> crate::prompt_bridge::BridgeInput<'b> {
        let metadata = self.read_chat_for(p).ok().map(|c| c.metadata()).unwrap_or_default();
        self.build_with_metadata(p, history, metadata)
    }

    pub(crate) fn build_with_metadata<'b>(
        &'b self, p: &'b GenerateParams, history: &'b [Msg],
        mut metadata: nast_model::chat::ChatMetadata,
    ) -> crate::prompt_bridge::BridgeInput<'b> {
        let mut character = p.character.clone();
        if let Some(context) = &p.group {
            if context.group.generation_mode != 0 {
                let join = |field, getter| nast_engine::group_chat::append_field(
                    &context.group, &context.members, &p.avatar, field, getter);
                character.data.description = join("Description", |c| &c.data.description);
                character.data.personality = join("Personality", |c| &c.data.personality);
                character.data.scenario = join("Scenario", |c| &c.data.scenario);
                character.data.mes_example = join("Example Messages", |c| &c.data.mes_example);
                character.data.extensions.depth_prompt = None;
            }
        }
        if let Some(scenario) = metadata.scenario.as_ref().filter(|s| !s.is_empty()) {
            character.data.scenario = scenario.clone();
        }
        if let Some(examples) = metadata.mes_example.as_ref().filter(|s| !s.is_empty()) {
            character.data.mes_example = examples.clone();
        }
        let world_config = nast_model::settings::world_info_view(self.settings_json);
        let regex_scripts = self.collect_regex_scripts(&character, &metadata);
        let total = history.len();

        // persona 解析（服务端；chat 绑定 > 默认）
        let persona = self.resolve_persona(&metadata);
        let name1 = persona.name.clone();
        let name2 = character.name.clone();

        let mut macro_env = nast_engine::macros::MacroEnv {
            user: name1.clone(), char: name2.clone(),
            group: p.group.as_ref().map(|g| g.members.iter().map(|(m,_)| m.name.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_else(|| name2.clone()),
            group_not_muted: p.group.as_ref().map(|g| g.members.iter().filter(|(m,_)| !g.group.disabled_members.contains(&m.avatar))
                .map(|(m,_)| m.name.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            not_char: p.group.as_ref().map(|g| g.members.iter().filter(|(m,_)| m.avatar != p.avatar)
                .map(|(m,_)| m.name.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
            description: character.data.description.clone(),
            personality: character.data.personality.clone(), scenario: character.data.scenario.clone(),
            persona: persona.description.clone(), char_prompt: character.data.system_prompt.clone(),
            char_instruction: character.data.post_history_instructions.clone(),
            char_version: character.data.character_version.clone(),
            creator_notes: character.data.creator_notes.clone(),
            mes_examples_raw: character.data.mes_example.clone(),
            model: crate::connection::model_for(&self.oai), outlets: Default::default(),
            ..Default::default()
        };
        let mut macro_context = nast_engine::macros::MacroContext {
            input: p.user_message.clone(), chat_length: history.len(),
            max_context_tokens: self.oai.openai_max_context,
            max_response_tokens: self.oai.openai_max_tokens,
            max_prompt_tokens: self.oai.openai_max_context - self.oai.openai_max_tokens,
            last_message: history.last().map(|m| m.mes.clone()).unwrap_or_default(),
            last_user_message: history.iter().rev().find(|m| m.is_user).map(|m| m.mes.clone()).unwrap_or_default(),
            last_char_message: history.iter().rev().find(|m| !m.is_user).map(|m| m.mes.clone()).unwrap_or_default(),
            last_swipe_id: history.last().and_then(|m| m.swipe_id),
            current_swipe_id: history.last().and_then(|m| m.swipe_id),
            outlets: Default::default(), ..Default::default()
        };
        macro_context.vars.local = metadata.variables.clone().into_iter().collect();
        macro_context.vars.global = self.global_variables.borrow().clone();
        let macro_context = std::cell::RefCell::new(macro_context);


        let messages: Vec<HistoryMessage> = history
            .iter()
            .filter(|m| !m.is_system)
            .enumerate()
            .map(|(idx, m)| {
                // 距底深度（0 = 最新）
                let depth = (total - 1 - idx) as i64;
                let placement = if m.is_user {
                    nast_model::regex_script::RP_USER_INPUT
                } else {
                    nast_model::regex_script::RP_AI_OUTPUT
                };
                let user_name = name1.clone();
                let char_name = name2.clone();
                let macro_fn =
                    move |s: &str| crate::prompt_bridge::substitute_basic(s, &user_name, &char_name);
                let params = nast_engine::regex_engine::RegexParams {
                    depth: Some(depth),
                    is_prompt: true,
                    ..Default::default()
                };
                let role = if m.is_user { "user" } else { "assistant" };
                let mut content = nast_engine::regex_engine::get_regexed_string(
                    &m.mes, placement, &regex_scripts, &params, &macro_fn,
                );
                let is_prefix = p.generation_type == GenerationType::Continue && idx + 1 == total;
                let reasoning = m.extra.reasoning.as_deref().unwrap_or("");
                let reasoning_enabled = self.power_bool("reasoning", "add_to_prompts", false);
                let limit = self.settings_json.pointer("/power_user/reasoning/max_additions").and_then(Value::as_u64).unwrap_or(1) as usize;
                let newer_reasoning = history.iter().skip(idx + 1).filter(|message| message.extra.reasoning.as_ref().is_some_and(|r| !r.is_empty())).count();
                if !reasoning.is_empty() && (is_prefix || (reasoning_enabled && newer_reasoning < limit)) {
                    let option = |key: &str, fallback: &str| self.settings_json.get("power_user")
                        .and_then(|power| power.get("reasoning")).and_then(|settings| settings.get(key))
                        .and_then(Value::as_str).unwrap_or(fallback).to_string();
                    content = format!("{}{}{}{}{}", option("prefix", "<think>"), reasoning,
                        option("suffix", "</think>"), option("separator", "\n"), content);
                }
                // names_behavior CONTENT（2）：所有人加前缀；DEFAULT（0）：仅群/强制头像
                let names_behavior = self.oai.character_names_behavior;
                let is_narrator = m.extra.kind.as_deref() == Some("narrator");
                if (names_behavior == 2 && !is_narrator)
                    || (names_behavior == 0 && p.is_group && m.name != name1 && !is_narrator)
                {
                    content = format!("{}: {}", m.name, content);
                }
                content = content.replace(String::from("\r").as_str(), "");
                // COMPLETION（1）：带 name 字段（openai.js setOpenAIMessages）
                let msg_name = if names_behavior == 1 {
                    Some(m.name.clone())
                } else {
                    None
                };
                HistoryMessage {
                    role: role.into(),
                    content,
                    name: msg_name,
                    is_narrator,
                    injected: false,
                }
            })
            .collect();
        let _ = CC_DUMMY_ID;

        // ---------- 世界书四源组装（getSortedEntries：chat→persona→char/global by strategy） ----------
        let wi_settings = self.wi_settings();
        let mut chat_books: Vec<(String, nast_model::world::WorldInfoBook)> = Vec::new();
        let mut persona_books: Vec<(String, nast_model::world::WorldInfoBook)> = Vec::new();
        let mut char_books: Vec<(String, nast_model::world::WorldInfoBook)> = Vec::new();
        let mut global_books: Vec<(String, nast_model::world::WorldInfoBook)> = Vec::new();
        let mut selected: std::collections::HashSet<String> = std::collections::HashSet::new();

        if let Some(name) = &metadata.world {
            if let Ok(b) = self.user.read_world(name) {
                selected.insert(name.clone());
                chat_books.push((name.clone(), b));
            }
        }
        if let Some(name) = &persona.lorebook {
            if !selected.contains(name) {
                if let Ok(b) = self.user.read_world(name) {
                    selected.insert(name.clone());
                    persona_books.push((name.clone(), b));
                }
            }
        }
        // 角色内嵌书 + charLore 辅助书
        let mut char_book_names: Vec<String> = Vec::new();
        if let Some(name) = &character.data.extensions.world {
            char_book_names.push(name.clone());
        }
        if let Some(char_lore) = world_config
            .get("char_lore")
            .and_then(|v| v.as_array())
        {
            let avatar_key = p.avatar.trim_end_matches(".png").to_string();
            for cl in char_lore {
                if cl.get("name").and_then(|n| n.as_str()) == Some(avatar_key.as_str()) {
                    if let Some(books) = cl.get("extraBooks").and_then(|b| b.as_array()) {
                        for b in books {
                            if let Some(bn) = b.as_str() {
                                char_book_names.push(bn.to_string());
                            }
                        }
                    }
                }
            }
        }
        for name in char_book_names {
            if !selected.contains(&name) {
                if let Ok(b) = self.user.read_world(&name) {
                    selected.insert(name.clone());
                    char_books.push((name.clone(), b));
                }
            }
        }
        // 全局激活书
        if let Some(globals) = world_config
            .get("global_select")
            .or_else(|| {
                self.settings_json
                    .get("world_info")
                    .and_then(|w| w.get("globalSelect"))
            })
            .and_then(|v| v.as_array())
        {
            for g in globals {
                if let Some(name) = g.as_str() {
                    if !selected.contains(name) {
                        if let Ok(b) = self.user.read_world(name) {
                            selected.insert(name.to_string());
                            global_books.push((name.to_string(), b));
                        }
                    }
                }
            }
        }

        let any_books = !chat_books.is_empty()
            || !persona_books.is_empty()
            || !char_books.is_empty()
            || !global_books.is_empty();

        // ---------- AN（interval 门控 + 角色卡 note 合并；WI an_top/bottom 稍后并入） ----------
        let mut an_note = self.build_an_note(p, &metadata, history);

        let mut wi_before = String::new();
        let mut wi_after = String::new();
        let mut wi_depth_injections: Vec<InChatInjection> = Vec::new();
        let mut em_blocks: Vec<(i64, Vec<ExampleBlock>)> = Vec::new();
        let mut outlets = serde_json::Map::new();
        let mut an_top = String::new();
        let mut an_bottom = String::new();
        {
            // AN 允许扫描（extension_settings.note.allowWIScan，默认 false）
            let allow_wi_scan = self
                .settings_json
                .get("extension_settings")
                .and_then(|e| e.get("note"))
                .and_then(|n| n.get("allowWIScan"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let extra_scan = if allow_wi_scan {
                an_note.as_ref().map(|a| a.text.clone()).unwrap_or_default()
            } else {
                String::new()
            };
            let scan_source = nast_engine::world_info::ScanSource {
                generation_type: p.generation_type.as_str().into(),
                chat: history.iter().map(|m| m.mes.clone()).collect(),
                persona_description: persona.description.clone(),
                char_description: character.data.description.clone(),
                char_personality: character.data.personality.clone(),
                char_depth_prompt: p
                    .character
                    .data
                    .extensions
                    .depth_prompt
                    .as_ref()
                    .map(|d| d.prompt.clone())
                    .unwrap_or_default(),
                scenario: character.data.scenario.clone(),
                creator_notes: character.data.creator_notes.clone(),
                char_file: p.avatar.trim_end_matches(".png").to_string(),
                char_tags: character.data.tags.clone(),
                extra_scan,
            };
            if any_books {
                // timedWorldInfo 持久化到聊天元数据
                let mut timed = metadata.timed_world_info.clone().unwrap_or_default();
                let chat_length = history.len() as i64;
                let max_context = self.oai.openai_max_context - self.oai.openai_max_tokens;
                let mut state = nast_engine::world_info::WiState {
                    timed: &mut timed,
                    chat_length,
                };
                let chat_refs: Vec<(&str, &nast_model::world::WorldInfoBook)> = chat_books
                    .iter()
                    .map(|(n, b)| (n.as_str(), b))
                    .collect();
                let persona_refs: Vec<(&str, &nast_model::world::WorldInfoBook)> = persona_books
                    .iter()
                    .map(|(n, b)| (n.as_str(), b))
                    .collect();
                let char_refs: Vec<(&str, &nast_model::world::WorldInfoBook)> = char_books
                    .iter()
                    .map(|(n, b)| (n.as_str(), b))
                    .collect();
                let global_refs: Vec<(&str, &nast_model::world::WorldInfoBook)> = global_books
                    .iter()
                    .map(|(n, b)| (n.as_str(), b))
                    .collect();
                let books = nast_engine::world_info::WiBooks {
                    chat_lore: chat_refs,
                    persona_lore: persona_refs,
                    character_lore: char_refs,
                    global_lore: global_refs,
                };
                let wi = nast_engine::world_info::check_world_info_with_context(
                    &books, &wi_settings, &scan_source, &mut state, &macro_env, &regex_scripts,
                    max_context, &macro_context,
                );
                metadata.timed_world_info = Some(timed);
                wi_before = wi.world_info_before;
                wi_after = wi.world_info_after;
                an_top = wi.an_top;
                an_bottom = wi.an_bottom;
                for de in &wi.depth_entries {
                    wi_depth_injections.push(InChatInjection {
                        content: de.content.clone(),
                        depth: de.depth,
                        role: de.role,
                        injection_order: de.order,
                    });
                }
                // EM 锚点：条目内容按示例对话解析，前后拼接（script.js:4580-4594）
                for (pos, content) in &wi.em_entries {
                    if content.trim().is_empty() {
                        continue;
                    }
                    let blocks = parse_examples(content, &name1, &name2);
                    if !blocks.is_empty() {
                        em_blocks.push((*pos, blocks));
                    }
                }
                for (name, content) in &wi.outlet_entries {
                    outlets.insert(name.clone(), serde_json::Value::String(content.clone()));
                }
            }
        }

        // ---------- AN 最终合并：ANTop + note + ANBottom（world-info.js:5151-5154） ----------
        if let Some(an) = &mut an_note {
            let mut parts: Vec<String> = Vec::new();
            if !an_top.is_empty() {
                parts.push(an_top.clone());
            }
            if !an.text.is_empty() {
                parts.push(an.text.clone());
            }
            if !an_bottom.is_empty() {
                parts.push(an_bottom.clone());
            }
            an.text = parts.join("\n");
        }

        // ---------- 示例区：EM 块前后拼接（before 逆序 prepend / after append） ----------
        let mut message_examples =
            parse_examples(&character.data.mes_example, &name1, &name2);
        for (pos, blocks) in em_blocks {
            if pos == 0 {
                for b in blocks.into_iter().rev() {
                    message_examples.insert(0, b);
                }
            } else {
                message_examples.extend(blocks);
            }
        }

        // ---------- 注入集合 ----------
        let mut all_injections = build_injections(p);
        if let Some(context) = &p.group {
            if context.group.generation_mode != 0 {
                all_injections.clear();
                for (depth, role, content) in nast_engine::group_chat::group_depth_prompts_for(&context.group, &context.members, Some(&p.avatar)) {
                    all_injections.push(InChatInjection { content, depth,
                        role: match role.as_str() { "user"=>1, "assistant"=>2, _=>0 }, injection_order:100 });
                }
            }
        }
        all_injections.extend(wi_depth_injections);
        // AN 聊天内深度（position 1）
        if let Some(an) = &an_note {
            if an.position == 1 && !an.text.is_empty() {
                all_injections.push(InChatInjection {
                    content: an.text.clone(),
                    depth: an.depth,
                    role: an.role,
                    injection_order: 100,
                });
            }
        }
        // persona AT_DEPTH（4）
        if persona.position == nast_model::persona::PDP_AT_DEPTH && !persona.description.is_empty()
        {
            all_injections.push(InChatInjection {
                content: persona.description.clone(),
                depth: persona.depth,
                role: persona.role,
                injection_order: 100,
            });
        }

        // persona 位置分发：IN_PROMPT(0) → marker；TOP_AN(2)/BOTTOM_AN(3) → 并入 AN（仅当 AN 存在）
        let mut persona_description = persona.description.clone();
        let mut persona_position_in_prompt = false;
        match persona.position {
            nast_model::persona::PDP_IN_PROMPT => {
                persona_position_in_prompt = !persona_description.is_empty();
            }
            nast_model::persona::PDP_TOP_AN => {
                if let Some(an) = &mut an_note {
                    an.text = format!("{}\n{}", persona.description, an.text);
                    persona_description = String::new();
                }
            }
            nast_model::persona::PDP_BOTTOM_AN => {
                if let Some(an) = &mut an_note {
                    an.text = format!("{}\n{}", an.text, persona.description);
                    persona_description = String::new();
                }
            }
            _ => {
                // AT_DEPTH 走注入；NONE(9) 不进 prompt
                persona_description = String::new();
            }
        }

        // AN 相对注入（position 0/2）
        let authors_note = an_note.and_then(|an| {
            if an.text.trim().is_empty() {
                None
            } else if an.position == 0 || an.position == 2 {
                Some(crate::prompt_bridge::AuthorsNote {
                    text: an.text,
                    position: an.position,
                })
            } else {
                None
            }
        });

        macro_env.outlets = outlets.clone();
        macro_context.borrow_mut().outlets = outlets.clone();
        crate::prompt_bridge::BridgeInput {
            metadata,
            macro_env,
            macro_context,
            oai: &self.oai,
            generation_type: p.generation_type.as_str(),
            name1: name1,
            name2: name2,
            is_group: p.is_group,
            char_description: character.data.description.clone(),
            char_personality: character.data.personality.clone(),
            scenario: character.data.scenario.clone(),
            persona_description,
            persona_position_in_prompt,
            world_info_before: wi_before,
            world_info_after: wi_after,
            messages,
            message_examples,
            pin_examples: false,
            in_chat_injections: all_injections,
            authors_note,
            outlets,
            system_prompt_override: {
                let sp = &character.data.system_prompt;
                if !sp.is_empty() { Some(sp.clone()) } else { None }
            },
            jailbreak_prompt_override: {
                let phi = &character.data.post_history_instructions;
                if !phi.is_empty() { Some(phi.clone()) } else { None }
            },
            cycle_prompt: None,
            last_role: None,
        }
    }

    fn check_prompt(&self, assembled: &AssembleOutput) -> Result<(), String> {
        if let Some(error) = &assembled.error {
            // Assembly errors contain budget counts only, never prompt content.
            tracing::warn!(event="generation_prompt_rejected", task_id=%self.routing.task_id,
                logical_model=%self.routing.model_id, max_output=self.oai.openai_max_tokens,
                context_limit=self.oai.openai_max_context, reason=%error);
            return Err(error.clone());
        }
        Ok(())
    }

    pub async fn call_provider(
        &self,
        assembled: &AssembleOutput,
        prefill: Option<&str>,
    ) -> Result<crate::routing::Outcome, String> {
        self.check_prompt(&assembled)?;
        let provider_msgs: Vec<ProviderMessage> = assembled
            .chat
            .iter()
            .map(|m| ProviderMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                name: m.name.clone(),
            })
            .collect();
        let gen_req = GenRequest {
            messages: provider_msgs,
            model: crate::connection::model_for(&self.oai),
            temperature: self.oai.temperature,
            top_p: self.oai.top_p,
            frequency_penalty: self.oai.frequency_penalty,
            presence_penalty: self.oai.presence_penalty,
            max_tokens: self.oai.openai_max_tokens,
            stop: vec![],
            stream: false,
            assistant_prefill: prefill.map(|s| s.to_string()),
            use_sysprompt: true,
            extra_headers: crate::connection::extra_headers(&self.oai),
            extra_body: crate::connection::extra_body(&self.oai),
        };
        let outcome = self.routing.execute(&gen_req, self.hub, &self.abort, &self.progress, false).await;
        if !outcome.complete && outcome.text.is_empty() && outcome.reasoning.is_empty() { return Err(outcome.error.unwrap_or(Value::Null).to_string()); }
        Ok(outcome)
    }

    /// 派发插件事件（无转换返回）。
    fn dispatch_plugin(&self, event: &str, data: &serde_json::Value) {
        if let Ok(host) = self.plugins.lock() {
            let _ = host.dispatch(event, data);
        }
    }

    /// 派发转换类钩子：返回插件改写文本或 fallback 原文。
    fn transform_or(&self, event: &str, data: &serde_json::Value, fallback: &str) -> String {
        if let Ok(host) = self.plugins.lock() {
            if let Some(text) = host.dispatch(event, data) {
                return text;
            }
        }
        fallback.to_string()
    }

    fn read_chat_for(&self, p: &GenerateParams) -> Result<ChatFile, String> {
        let result = if p.is_group { self.user.read_group_chat(&p.chat_file) }
            else { self.user.read_chat(&p.avatar, &p.chat_file) };
        result.map_err(|e| e.to_string())
    }

    fn save_chat_for(&self, p: &GenerateParams, chat: &ChatFile) -> Result<(), String> {
        if p.is_group { self.user.save_group_chat(&p.chat_file, chat, false).map_err(|e| e.to_string()) }
        else { self.save_chat(&p.avatar, &p.chat_file, chat) }
    }

    fn save_chat(&self, avatar: &str, file: &str, chat: &ChatFile) -> Result<(), String> {
        self.user
            .save_chat(avatar, file, chat, false)
            .map_err(|e| e.to_string())
    }

    fn emit_message_deleted(&self, at: usize) {
        self.hub.emit("message_deleted", json!(at));
    }

    /// 解析 persona（personas.js：chat_metadata.persona 绑定 > power_user.default_persona；
    /// 名字 = personas[pid]，描述/位置/深度/角色/世界书 = persona_descriptions[pid]，
    /// 缺省回落 power_user 直存字段，最终回落 username/"User"）。
    fn resolve_persona(&self, metadata: &nast_model::chat::ChatMetadata) -> ResolvedPersona {
        let power = self.settings_json.get("power_user");
        let str_of = |k: &str| -> String {
            power
                .and_then(|p| p.get(k))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let i64_of = |k: &str, d: i64| -> i64 {
            power
                .and_then(|p| p.get(k))
                .and_then(|v| v.as_i64())
                .unwrap_or(d)
        };
        // pid：聊天绑定 > 默认
        let pid = metadata
            .persona
            .clone()
            .or_else(|| {
                power
                    .and_then(|p| p.get("default_persona"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
            .unwrap_or_default();

        let mut name = String::new();
        let mut description = String::new();
        let mut position = i64_of("persona_description_position", nast_model::persona::PDP_IN_PROMPT);
        let mut depth = i64_of("persona_description_depth", 2);
        let mut role = i64_of("persona_description_role", 0);
        let lorebook = str_of("persona_description_lorebook");
        let mut lorebook_opt = (!lorebook.is_empty()).then_some(lorebook.clone());

        if !pid.is_empty() {
            if let Some(p) = power.and_then(|p| p.get("personas")).and_then(|v| v.as_object()) {
                if let Some(Value::String(n)) = p.get(&pid) {
                    name = n.clone();
                }
            }
            if let Some(d) = power
                .and_then(|p| p.get("persona_descriptions"))
                .and_then(|v| v.as_object())
                .and_then(|m| m.get(&pid))
            {
                if let Some(Value::String(s)) = d.get("description") {
                    description = s.clone();
                }
                if let Some(v) = d.get("position").and_then(|v| v.as_i64()) {
                    position = v;
                }
                if let Some(v) = d.get("depth").and_then(|v| v.as_i64()) {
                    depth = v;
                }
                if let Some(v) = d.get("role").and_then(|v| v.as_i64()) {
                    role = v;
                }
                if let Some(Value::String(s)) = d.get("lorebook") {
                    lorebook_opt = (!s.is_empty()).then_some(s.clone());
                }
            }
        }
        // 无 pid 或解析为空：回落 power_user 直存（单 persona 模式）
        if name.is_empty() {
            name = str_of("username");
        }
        if description.is_empty() {
            description = str_of("persona_description");
        }
        if name.is_empty() {
            name = "User".into();
        }
        ResolvedPersona { name, description, position, depth, role, lorebook: lorebook_opt }
    }

    /// 构建 Author's Note（AN.js setFloatingPrompt 324-392）：
    /// - 文本：chat note > extension_settings.note.default；角色卡 note 按 0 替换/1 前置/2 后缀合并
    /// - interval 门控：用户消息数取模（interval<=1 恒插）
    /// - 位置/深度/角色：chat_metadata.note_*（缺省 1/4/0）
    fn build_an_note(
        &self,
        p: &GenerateParams,
        metadata: &nast_model::chat::ChatMetadata,
        history: &[Msg],
    ) -> Option<AnNote> {
        let note_ext = self.settings_json.get("extension_settings").and_then(|e| e.get("note"));
        let default_note = note_ext
            .and_then(|n| n.get("default"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let mut text = metadata
            .note_prompt
            .clone()
            .unwrap_or_else(|| default_note.clone());
        // 位置缺省 = 1（聊天内深度）
        let position = metadata.note_position.unwrap_or(1);
        let depth = metadata.note_depth.unwrap_or(4);
        let role = metadata.note_role.unwrap_or(0);
        let interval = metadata.note_interval.unwrap_or(1).max(1);

        // 角色卡 note（extension_settings.note.chara[{name, prompt, useChara, position}]）
        let char_key = p.avatar.trim_end_matches(".png").to_string();
        if let Some(chara) = note_ext
            .and_then(|n| n.get("chara"))
            .and_then(|c| c.as_array())
        {
            if let Some(entry) = chara.iter().find(|c| {
                c.get("name").and_then(|n| n.as_str()) == Some(char_key.as_str())
                    && c.get("useChara").and_then(|u| u.as_bool()).unwrap_or(false)
            }) {
                let chara_prompt = entry
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !chara_prompt.is_empty() {
                    match entry.get("position").and_then(|v| v.as_i64()).unwrap_or(0) {
                        1 => text = format!("{chara_prompt}\n{text}"),
                        2 => text = format!("{text}\n{chara_prompt}"),
                        _ => text = chara_prompt.to_string(),
                    }
                }
            }
        }

        // interval 门控：lastMessageNumber = 用户消息数（AN.js:334-341）
        let last_message_number = history.iter().filter(|m| m.is_user).count() as i64;
        let should_add = if interval <= 1 {
            true
        } else if last_message_number >= interval {
            last_message_number % interval == 0
        } else {
            interval - last_message_number == 0
        };
        if !should_add {
            return None;
        }
        if text.trim().is_empty() {
            // 无文本时仍返回占位（persona TOP/BOTTOM_AN 可能并入）——由调用方丢弃空文本
            return Some(AnNote { text: String::new(), position, depth, role });
        }
        Some(AnNote { text, position, depth, role })
    }

    /// 聚合三作用域正则脚本：全局（settings）→ 角色内嵌 → 聊天级。
    pub fn collect_regex_scripts(        &self,
        character: &nast_model::card::Character,
        metadata: &nast_model::chat::ChatMetadata,
    ) -> Vec<nast_model::regex_script::RegexScript> {
        collect_regex_scripts_for(self.settings_json, character, metadata)
    }

    /// WI 全局设置（settings.json 的 world_info 切片；缺省用默认值）。
    fn wi_settings(&self) -> nast_engine::world_info::WiSettings {
        let world_config = nast_model::settings::world_info_view(self.settings_json);
        let default = nast_engine::world_info::WiSettings::default();
        let Some(wi) = world_config.as_object()
        else {
            return default;
        };
        nast_engine::world_info::WiSettings {
            depth: wi.get("world_info_depth").and_then(|v| v.as_i64()).unwrap_or(default.depth),
            min_activations: wi.get("world_info_min_activations").and_then(|v| v.as_i64()).unwrap_or(default.min_activations),
            min_activations_depth_max: wi.get("world_info_min_activations_depth_max").and_then(|v| v.as_i64()).unwrap_or(default.min_activations_depth_max),
            budget: wi.get("world_info_budget").and_then(|v| v.as_i64()).unwrap_or(default.budget),
            budget_cap: wi.get("world_info_budget_cap").and_then(|v| v.as_i64()).unwrap_or(default.budget_cap),
            recursive: wi.get("world_info_recursive").and_then(|v| v.as_bool()).unwrap_or(default.recursive),
            case_sensitive: wi.get("world_info_case_sensitive").and_then(|v| v.as_bool()).unwrap_or(default.case_sensitive),
            match_whole_words: wi.get("world_info_match_whole_words").and_then(|v| v.as_bool()).unwrap_or(default.match_whole_words),
            use_group_scoring: wi.get("world_info_use_group_scoring").and_then(|v| v.as_bool()).unwrap_or(default.use_group_scoring),
            max_recursion_steps: wi
                .get("world_info_max_recursion_steps")
                .map(|v| {
                    v.as_i64()
                        .or_else(|| v.as_bool().map(|b| b as i64))
                        .unwrap_or(0)
                })
                .unwrap_or(0),
            character_strategy: wi.get("world_info_character_strategy").and_then(|v| v.as_i64()).unwrap_or(default.character_strategy),
        }
    }
}

/// integrity slug：新聊天没有就生成（保存时校验）。
fn ensure_integrity(chat: &mut ChatFile) {
    if let Some(header) = chat.0.first_mut() {
        let has = header
            .get("chat_metadata")
            .and_then(|m| m.get("integrity"))
            .and_then(|v| v.as_str())
            .is_some();
        if !has {
            let slug = uuid::Uuid::new_v4().to_string();
            header["chat_metadata"]["integrity"] = json!(slug);
        }
    }
}

/// mes_example 解析（对齐 openai.js parseMesExamples + parseExampleIntoIndividual）：
/// - `<START>`（大小写不敏感）分块
/// - 每块首行（"This is how X should talk"）跳过
/// - 行首 `name1:`（user）或 `name2:`（char，宏已替换为实际名）切换发言者
/// - 无前缀续行并入当前消息
/// - 产出消息剥离名字前缀、trim；name 为 example_user/example_assistant，role 一律 system
pub fn parse_examples(raw: &str, name1: &str, name2: &str) -> Vec<ExampleBlock> {
    use once_cell::sync::Lazy;
    static START_RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"(?i)<START>").unwrap());

    let mut blocks = Vec::new();
    for block in START_RE.split(raw) {
        let tmp: Vec<&str> = block.lines().collect();
        if tmp.is_empty() {
            continue;
        }
        let mut msgs: Vec<(String, String, String)> = Vec::new();
        let mut cur_lines: Vec<String> = Vec::new();
        let mut in_user = false;
        let mut in_bot = false;

        let add_msg = |cur: &mut Vec<String>, msgs: &mut Vec<(String, String, String)>, speaker: &str, system_name: &str| {
            // 剥离 "speaker:" 前缀（取首个出现）并 trim
            let joined = cur.join("
");
            let parsed = joined
                .replacen(&format!("{}:", speaker), "", 1)
                .trim()
                .to_string();
            msgs.push(("system".into(), system_name.into(), parsed));
            cur.clear();
        };

        // ST: skip first line as it'll always be "This is how {bot} should talk"
        for line in tmp.iter().skip(1) {
            let cur_str = line;
            let user_prefix = format!("{}:", name1);
            let char_prefix = format!("{}:", name2);
            if cur_str.starts_with(&user_prefix) {
                in_user = true;
                if in_bot {
                    add_msg(&mut cur_lines, &mut msgs, name2, "example_assistant");
                }
                in_bot = false;
            } else if cur_str.starts_with(&char_prefix) {
                in_bot = true;
                if in_user {
                    add_msg(&mut cur_lines, &mut msgs, name1, "example_user");
                }
                in_user = false;
            }
            cur_lines.push(cur_str.to_string());
        }
        if in_user {
            add_msg(&mut cur_lines, &mut msgs, name1, "example_user");
        } else if in_bot {
            add_msg(&mut cur_lines, &mut msgs, name2, "example_assistant");
        }

        if !msgs.is_empty() {
            blocks.push(ExampleBlock { messages: msgs });
        }
    }
    blocks
}

/// rpc 侧（非生成会话）使用的正则脚本聚合：全局 + 角色内嵌 + 聊天级。
pub fn collect_regex_scripts_for(
    settings: &serde_json::Value,
    character: &nast_model::card::Character,
    metadata: &nast_model::chat::ChatMetadata,
) -> Vec<nast_model::regex_script::RegexScript> {
        let mut out: Vec<nast_model::regex_script::RegexScript> = Vec::new();
        if settings.pointer("/extension_settings/disabledExtensions")
            .and_then(Value::as_array).is_some_and(|values| values.iter().any(|v| v == "regex")) {
            return out;
        }
        // 全局：extension_settings.regex
        if let Some(list) = settings
            .get("extension_settings")
            .and_then(|e| e.get("regex"))
            .and_then(|v| v.as_array())
        {
            for s in list {
                if let Ok(script) =
                    serde_json::from_value::<nast_model::regex_script::RegexScript>(s.clone())
                {
                    out.push(script);
                }
            }
        }
        // ST order: global -> allowed preset -> allowed character.
        let preset_name = settings.pointer("/oai_settings/preset_settings_openai").and_then(Value::as_str).unwrap_or("");
        if settings.pointer("/extension_settings/preset_allowed_regex/openai")
            .and_then(Value::as_array).is_some_and(|values| values.iter().any(|v| v == preset_name)) {
            if let Some(scripts) = settings.pointer("/oai_settings/extensions").and_then(|v| v.get("regex_scripts")).and_then(Value::as_array) {
                out.extend(scripts.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()));
            }
        }
        if character.avatar.as_ref().is_some_and(|avatar|
            settings.pointer("/extension_settings/character_allowed_regex")
                .and_then(Value::as_array).is_some_and(|values| values.iter().any(|v| v == avatar))) {
            out.extend(character.data.extensions.regex_scripts.clone());
        }
        // 聊天级
        out.extend(metadata.regex_scripts.clone());
        out
    }

/// 角色卡 @depth 注入（AN/persona 已服务端化，不再从前端透传）。
pub fn build_injections(p: &GenerateParams) -> Vec<InChatInjection> {    let mut out = Vec::new();
    if let Some(dp) = &p.character.data.extensions.depth_prompt {
        if !dp.prompt.is_empty() {
            let role = match dp.role.as_str() {
                "user" => 1,
                "assistant" => 2,
                _ => 0,
            };
            out.push(InChatInjection {
                content: dp.prompt.clone(),
                depth: dp.depth,
                role,
                injection_order: 100,
            });
        }
    }
    out
}

/// prompt_built 插件钩子：{messages=[{role,content},...]} 重写拼装结果。
fn apply_prompt_plugin(
    plugins: &std::sync::Mutex<nast_plugin::PluginHost>,
    assembled: AssembleOutput,
) -> AssembleOutput {
    let data = json!({
        "messages": assembled
            .chat
            .iter()
            .map(|m| json!({"role": m.role, "content": m.content, "name": m.name}))
            .collect::<Vec<_>>(),
    });
    let rewritten = {
        let host = plugins.lock().unwrap();
        host.dispatch_json("prompt_built", &data)
    };
    let arr = match rewritten {
        Some(Value::Array(arr)) => arr,
        // {messages: [...]} 包装形态
        Some(Value::Object(map)) => match map.get("messages") {
            Some(Value::Array(arr)) => arr.clone(),
            _ => return assembled,
        },
        _ => return assembled,
    };
    let mut chat: Vec<nast_engine::prompt::PromptMessage> = Vec::new();
    for m in arr {
        let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("system").to_string();
        let content = m.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let name = m.get("name").and_then(|v| v.as_str()).map(String::from);
        chat.push(nast_engine::prompt::PromptMessage {
            tokens: nast_engine::tokens::count_tokens(
                &content,
                nast_engine::tokens::resolve_tokenizer("gpt-4o"),
            ) as i64,
            role,
            content,
            name,
            identifier: "plugin".into(),
            injected: false,
        });
    }
    if chat.is_empty() {
        return assembled;
    }
    let token_counts = chat.iter().map(|m| m.tokens).collect();
    AssembleOutput { chat, token_counts, error: assembled.error }
}

// ---------- cleanUpMessage / stopping strings（script.js:6383-6533, power-user.js:3068-3112） ----------

impl<'a> GenerateSession<'a> {
    /// power_user 嵌套读取（bool）。
    fn power_bool(&self, section: &str, key: &str, default: bool) -> bool {
        self.settings_json
            .get("power_user")
            .and_then(|p| p.get(section))
            .and_then(|s| s.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }

    /// power_user 顶层读取（bool）。
    fn power_top_bool(&self, key: &str, default: bool) -> bool {
        self.settings_json
            .get("power_user")
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }

    /// power_user 顶层读取（String）。
    fn power_top_str(&self, key: &str) -> String {
        self.settings_json
            .get("power_user")
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// 自定义停止串（power-user.js:3068-3112）：JSON 数组字符串 + 宏替换；
    /// limit > 0 时截断（CC 上限 4；0 = 不限，用于输出侧剥离）。
    pub fn stopping_strings(&self, name1: &str, name2: &str, limit: usize) -> Vec<String> {
        let raw = self.power_top_str("custom_stopping_strings");
        if raw.trim().is_empty() {
            return vec![];
        }
        let subst = self.power_top_bool("custom_stopping_strings_macro", false);
        let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) else {
            return vec![];
        };
        let mut out: Vec<String> = arr
            .into_iter()
            .filter_map(|v| v.as_str().map(String::from))
            .filter(|s| !s.is_empty())
            .map(|s| {
                if subst {
                    crate::prompt_bridge::substitute_basic(&s, name1, name2)
                } else {
                    s
                }
            })
            .collect();
        if limit > 0 && out.len() > limit {
            out.truncate(limit);
        }
        out
    }

    /// 生成输出清理管线（script.js cleanUpMessage 6383-6533）。
    /// 仅作用于生成结果（不影响 prompt 构建）；各开关走 power_user，默认值与 ST 一致。
    pub fn clean_up_message(
        &self,
        text: &str,
        name1: &str,
        name2: &str,
        is_impersonate: bool,
        is_continue: bool,
        stopping_strings: &[String],
        regex_scripts: &[nast_model::regex_script::RegexScript],
    ) -> String {
        let mut text = text.to_string();

        // 1. user_prompt_bias 前置（非 impersonate/continue）
        if !is_impersonate && !is_continue {
            let bias = self.power_top_str("user_prompt_bias");
            if !bias.is_empty() {
                text = format!("{bias}{text}");
            }
        }

        // 2. 停止串尾部部分前缀剥离（script.js:6418-6428，流式友好）
        for s in stopping_strings {
            loop {
                let mut cut = false;
                for j in 1..=s.chars().count() {
                    let prefix: String = s.chars().take(j).collect();
                    if text.ends_with(&prefix) {
                        let keep = text.chars().count() - j;
                        text = text.chars().take(keep).collect();
                        cut = true;
                        break;
                    }
                }
                if !cut {
                    break;
                }
            }
        }

        // 3. 正则默认 pass（AI_OUTPUT / USER_INPUT；script.js:6430）
        {
            let user_name = name1.to_string();
            let char_name = name2.to_string();
            let macro_fn =
                move |s: &str| crate::prompt_bridge::substitute_basic(s, &user_name, &char_name);
            let placement = if is_impersonate {
                nast_model::regex_script::RP_USER_INPUT
            } else {
                nast_model::regex_script::RP_AI_OUTPUT
            };
            let params = nast_engine::regex_engine::RegexParams::default();
            text = nast_engine::regex_engine::get_regexed_string(
                &text, placement, regex_scripts, &params, &macro_fn,
            );
        }

        // 4. collapseNewlines
        if self.power_top_bool("collapse_newlines", false) {
            let mut collapsed = String::new();
            let mut last_nl = false;
            for c in text.chars() {
                if c == '\n' {
                    if !last_nl {
                        collapsed.push(c);
                    }
                    last_nl = true;
                } else {
                    collapsed.push(c);
                    last_nl = false;
                }
            }
            text = collapsed;
        }

        // 5. 行尾空白剥离（/[^\S\r\n]+$/gm）
        text = strip_trailing_whitespace_per_line(&text);

        // 6. 错误说话人移除（allow_name1/2_display 默认 false）
        let allow1 = self.power_top_bool("allow_name1_display", false);
        let allow2 = self.power_top_bool("allow_name2_display", false);
        if !allow1 && !text.is_empty() && text.starts_with(&format!("{name1}:")) {
            return String::new();
        }
        if !allow2 {
            // 尾部 "\nName2:" 块清除
            loop {
                let suffix = format!("\n{name2}:");
                if text.ends_with(&suffix) {
                    let keep = text.chars().count() - suffix.chars().count();
                    text = text.chars().take(keep).collect();
                } else {
                    break;
                }
            }
        }

        // 7. <|endoftext|> 截断
        if let Some(pos) = text.find("<|endoftext|>") {
            text.truncate(pos);
        }

        // 8. 角色名前缀剥离（!allow_name2_display 时开头 "Name2:" 去除）
        if !allow2 {
            let prefix = format!("{name2}:");
            if text.starts_with(&prefix) {
                text = text[prefix.len()..].to_string();
            }
        }

        // 9. fixMarkdown(false)：* / _ 间距修正（auto_fix_generated_markdown 默认 true）
        if self.power_top_bool("auto_fix_generated_markdown", true) {
            text = fix_markdown(&text, false);
        }

        // 10. trim_to_end_sentence（trim_sentences 默认 false）
        if self.power_top_bool("trim_sentences", false) {
            text = trim_to_end_sentence(&text);
        }

        // 11. trim_spaces（默认 true）
        if self.power_top_bool("trim_spaces", true) {
            text = text.trim().to_string();
        }
        text
    }
}

/// 行尾空白剥离（保留换行结构）。
fn strip_trailing_whitespace_per_line(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_end_matches([' ', '\t', '\u{a0}']))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 句尾截断（toolops trimToEndSentence 简化：最后一个 . ! ? ？。」」…… 处截断）。
fn trim_to_end_sentence(text: &str) -> String {
    let trimmed = text.trim_end();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut last = None;
    for (i, c) in chars.iter().enumerate() {
        if ".!?？！。…\"」』)".contains(*c) {
            last = Some(i);
        }
    }
    match last {
        Some(i) => chars[..=i].iter().collect::<String>(),
        None => trimmed.to_string(),
    }
}

/// fixMarkdown（power-user.js:429-469）：
/// - 通用：成对 * / _ 标记内侧空白修正（`* text *` → `*text*`）
/// - forDisplay：逐行补齐奇数个 * / " 
pub fn fix_markdown(text: &str, for_display: bool) -> String {
    use once_cell::sync::Lazy;
    static SPACE_SIDE_RE: Lazy<regex::Regex> = Lazy::new(|| {
        regex::Regex::new(
            r"(\*|_)[\t \u{a0}\u{1680}\u{2000}-\u{200a}\u{202f}\u{205f}\u{3000}\u{feff}]+|[\t \u{a0}\u{1680}\u{2000}-\u{200a}\u{202f}\u{205f}\u{3000}\u{feff}]+(\*|_)",
        )
        .unwrap()
    });

    // 成对标记扫描（regex crate 无反向引用，手写 /([*_]{1,2})([\s\S]*?)\1/g 语义：
    // 优先双字符标记，闭合取最近出现 = 非贪婪；匹配后从尾部继续）
    let bytes = text.as_bytes();
    let mut new_text = text.to_string();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'*' || c == b'_' {
            let len = if i + 1 < bytes.len() && bytes[i + 1] == c { 2 } else { 1 };
            let marker = &text[i..i + len];
            if let Some(rel) = text[i + len..].find(marker) {
                let end = i + len + rel + len;
                pairs.push((i, end));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    // 倒序替换标记内侧空白（保持字节索引有效）
    for (start, end) in pairs.iter().rev() {
        let matched = &new_text[*start..*end];
        let replacement = SPACE_SIDE_RE.replace_all(matched, "${1}${2}").to_string();
        if replacement != matched {
            new_text.replace_range(*start..*end, &replacement);
        }
    }

    if !for_display {
        return new_text;
    }
    // 逐行补齐奇数个 * / "
    let mut lines: Vec<String> = new_text.split('\n').map(String::from).collect();
    for line in lines.iter_mut() {
        for ch in ['*', '"'] {
            let count = line.matches(ch).count();
            if count % 2 == 1 {
                let trimmed_end = line.trim_end().to_string();
                *line = format!("{trimmed_end}{ch}");
            }
        }
    }
    lines.join("\n")
}

/// <think>…</thinking> 块剥离（reasoning auto_parse）。
/// 返回 (正文, reasoning)。支持 <think> 与 <thinking> 两种标签。
pub fn extract_think_blocks(text: &str) -> (String, String) {
    use once_cell::sync::Lazy;
    static THINK_RE: Lazy<regex::Regex> =
        Lazy::new(|| regex::Regex::new(r"(?s)<think(?:ing)?>(.*?)</think(?:ing)?>").unwrap());
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut out = String::new();
    let mut last = 0usize;
    for caps in THINK_RE.captures_iter(text) {
        let m = caps.get(0).unwrap();
        out.push_str(&text[last..m.start()]);
        reasoning_parts.push(caps.get(1).map(|g| g.as_str().trim().to_string()).unwrap_or_default());
        last = m.end();
    }
    out.push_str(&text[last..]);
    (out, reasoning_parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{extract_think_blocks, fix_markdown, parse_examples};

    #[test]
    fn parse_examples_st_semantics() {
        let raw = "This is how Seraphina should talk\n<START>\nUser: hello there\nSeraphina: hi! I am Seraphina.\ncontinuation line\nUser: bye";
        let blocks = parse_examples(raw, "User", "Seraphina");
        assert_eq!(blocks.len(), 1);
        let msgs = &blocks[0].messages;
        // 3 条消息：user hello / assistant hi+continuation / user bye
        assert_eq!(msgs.len(), 3);
        // role 全部是 system（CC 示例约定）
        assert!(msgs.iter().all(|(r, _, _)| r == "system"));
        // name 分别 example_user / example_assistant / example_user
        assert_eq!(msgs[0].1, "example_user");
        assert_eq!(msgs[1].1, "example_assistant");
        assert_eq!(msgs[2].1, "example_user");
        // 前缀剥离
        assert_eq!(msgs[0].2, "hello there");
        // 续行合并 + 前缀剥离
        assert_eq!(msgs[1].2, "hi! I am Seraphina.\ncontinuation line");
        assert_eq!(msgs[2].2, "bye");
    }

    #[test]
    fn parse_examples_start_case_insensitive() {
        let raw = "<start>\nUser: a\nSeraphina: b";
        let blocks = parse_examples(raw, "User", "Seraphina");
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn think_block_extraction() {
        let (text, reasoning) = extract_think_blocks("<think>step one</think>Hello!");
        assert_eq!(text, "Hello!");
        assert_eq!(reasoning, "step one");
        let (text, reasoning) =
            extract_think_blocks("<thinking>a</thinking>mid<think>b</think>tail");
        assert_eq!(text, "midtail");
        assert_eq!(reasoning, "a
b");
        let (text, reasoning) = extract_think_blocks("plain");
        assert_eq!((text.as_str(), reasoning.as_str()), ("plain", ""));
    }

    #[test]
    fn fix_markdown_spacing_and_display() {
        assert_eq!(fix_markdown("* text *", false), "*text*");
        assert_eq!(fix_markdown("he said \"hi", true), "he said \"hi\"");
        assert_eq!(fix_markdown("he said \"hi", false), "he said \"hi");
    }
}
