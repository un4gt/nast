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

use futures_util::StreamExt;
use nast_model::chat::{ChatFile, ChatMessage as Msg, GenerationType, MessageExtra, SwipeInfo};
use nast_model::preset::{OaiSettings, CC_DUMMY_ID};
use nast_providers::{
    ChatMessage as ProviderMessage, GenRequest, Provider, ProviderKind, StreamEvent,
};
use serde_json::json;

use crate::prompt_bridge::{ExampleBlock, HistoryMessage, InChatInjection};
use crate::state::EventHub;
use nast_engine::prompt::AssembleOutput;

/// 生成参数。
pub struct GenerateParams {
    pub generation_type: GenerationType,
    pub avatar: String,
    pub chat_file: String,
    pub user_message: String,
    pub character: nast_model::card::Character,
    pub persona_description: String,
    pub persona_position_in_prompt: bool,
    pub is_group: bool,
}

pub struct GenerateResult {
    pub text: String,
    pub saved: bool,
}

pub struct GenerateSession<'a> {
    pub user: &'a nast_storage::UserData,
    pub hub: &'a EventHub,
    pub oai: OaiSettings,
    pub provider: Provider,
    pub abort: tokio_util::sync::CancellationToken,
}

impl<'a> GenerateSession<'a> {
    pub async fn run(&self, params: GenerateParams) -> Result<GenerateResult, String> {
        self.hub.emit(
            "generation_started",
            json!({"type": params.generation_type.as_str()}),
        );
        let result = match params.generation_type {
            GenerationType::Normal | GenerationType::Regenerate | GenerationType::Swipe => {
                self.run_message_gen(&params).await
            }
            GenerationType::Impersonate => self.run_impersonate(&params).await,
            GenerationType::Quiet => self.run_quiet(&params).await,
            GenerationType::Continue => self.run_continue(&params).await,
        };
        // GENERATION_ENDED（ST 由 hideStopButton 发出）
        self.hub.emit("generation_ended", json!({}));
        result
    }

    // ---------- 主流程：normal/swipe/regenerate ----------

    async fn run_message_gen(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let gen_started = nast_storage::message_time_stamp();
        let mut chat = self.user.read_chat(&p.avatar, &p.chat_file).map_err(|e| e.to_string())?;
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
            let msg = Msg {
                name: "User".into(),
                is_user: true,
                is_system: false,
                send_date: nast_storage::message_time_stamp(),
                mes: p.user_message.clone(),
                extra: MessageExtra::default(),
                ..Default::default()
            };
            let v = serde_json::to_value(&msg).map_err(|e| e.to_string())?;
            chat.0.push(v);
            self.save_chat(&p.avatar, &p.chat_file, &chat)?;
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
        let assembled = crate::prompt_bridge::assemble_with_macros(&self.oai, &input);

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
        let gen_req = GenRequest {
            messages: provider_msgs,
            model: self.oai.openai_model.clone(),
            temperature: self.oai.temperature,
            top_p: self.oai.top_p,
            frequency_penalty: self.oai.frequency_penalty,
            presence_penalty: self.oai.presence_penalty,
            max_tokens: self.oai.openai_max_tokens,
            stop: vec![],
            stream: true,
            assistant_prefill: None,
            use_sysprompt: true,
            extra_headers: vec![],
            extra_body: json!({}),
        };

        // 流式接收
        let mut stream = self
            .provider
            .generate_stream(&gen_req)
            .await
            .map_err(|e| e.to_string())?;
        tokio::pin!(stream);

        let char_name = p.character.name.clone();
        let mut streamed = String::new();
        let mut errored: Option<String> = None;
        while let Some(ev) = stream.next().await {
            if self.abort.is_cancelled() {
                // 停止：保留半截文本并落盘（ST stopGeneration 行为）
                break;
            }
            match ev {
                Ok(StreamEvent::Token(t)) => {
                    streamed.push_str(&t);
                    self.hub.emit("stream_token_received", json!({"text": t}));
                }
                Ok(StreamEvent::Error(e)) => {
                    errored = Some(e);
                    break;
                }
                Ok(StreamEvent::Done) => break,
                Ok(_) => {}
                Err(e) => {
                    errored = Some(e.to_string());
                    break;
                }
            }
        }

        if let Some(e) = errored {
            // 错误不写入消息（ST 1.18.0：toastr only）；已流出的半截文本保留
            self.hub.emit("toast", json!({"message": e, "type": "error"}));
            if streamed.is_empty() {
                self.hub.emit("generation_stopped", json!({}));
                return Err(e);
            }
        }

        // 落盘：saveReply 语义
        let now = nast_storage::message_time_stamp();
        match p.generation_type {
            GenerationType::Swipe => {
                // 原地写 swipes[swipe_id] + swipe_info + 新 send_date
                let last = chat.0.last_mut().ok_or("empty chat")?;
                let msg: Msg = serde_json::from_value(last.clone()).map_err(|e| e.to_string())?;
                let mut swipes = msg.swipes.clone().unwrap_or_else(|| vec![msg.mes.clone()]);
                let swipe_id = msg.swipe_id.unwrap_or(0) as usize;
                while swipes.len() <= swipe_id {
                    swipes.push(String::new());
                }
                swipes[swipe_id] = streamed.clone();
                let mut extra = msg.extra.clone();
                extra.api = Some("openai".into());
                extra.model = Some(self.oai.openai_model.clone());
                extra.gen_started = Some(gen_started.clone());
                extra.gen_finished = Some(now.clone());
                let swipe_info = SwipeInfo {
                    send_date: Some(now.clone()),
                    gen_started: Some(gen_started.clone()),
                    gen_finished: Some(now.clone()),
                    extra: extra.clone(),
                };
                let mut infos = msg.swipe_info.clone().unwrap_or_default();
                while infos.len() <= swipe_id {
                    infos.push(SwipeInfo::default());
                }
                infos[swipe_id] = swipe_info;
                let mut new_msg = msg;
                new_msg.swipes = Some(swipes);
                new_msg.swipe_info = Some(infos);
                new_msg.send_date = now.clone(); // 每 swipe 独立 send_date
                new_msg.mes = streamed.clone();
                *last = serde_json::to_value(&new_msg).map_err(|e| e.to_string())?;
            }
            _ => {
                // normal/regenerate：新消息 + setFirstSwipe 镜像
                let mut extra = MessageExtra::default();
                extra.api = Some("openai".into());
                extra.model = Some(self.oai.openai_model.clone());
                extra.gen_started = Some(gen_started.clone());
                extra.gen_finished = Some(now.clone());
                let msg = Msg {
                    name: char_name.clone(),
                    is_user: false,
                    is_system: false,
                    send_date: now.clone(),
                    mes: streamed.clone(),
                    extra,
                    // 每条新 AI 消息都有 swipes 基础设施（setFirstSwipe）
                    swipes: Some(vec![streamed.clone()]),
                    swipe_id: Some(0),
                    swipe_info: Some(vec![SwipeInfo {
                        send_date: Some(now.clone()),
                        gen_started: Some(gen_started.clone()),
                        gen_finished: Some(now.clone()),
                        extra: MessageExtra::default(),
                    }]),
                    ..Default::default()
                };
                let v = serde_json::to_value(&msg).map_err(|e| e.to_string())?;
                chat.0.push(v);
            }
        }
        self.save_chat(&p.avatar, &p.chat_file, &chat)?;
        let chat_id = chat.0.len() as i64 - 1;
        self.hub.emit("message_received", json!(chat_id));
        self.hub.emit("character_message_rendered", json!(chat_id));
        Ok(GenerateResult { text: streamed, saved: true })
    }

    // ---------- impersonate：不落消息 ----------

    async fn run_impersonate(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let chat = self.user.read_chat(&p.avatar, &p.chat_file).map_err(|e| e.to_string())?;
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
            crate::prompt_bridge::substitute_basic(&self.oai.impersonation_prompt, "User", &p.character.name)
        };
        let assembled = crate::prompt_bridge::assemble_impersonate(&self.oai, &input, &impersonation_prompt);
        let text = self.call_provider(&assembled, None).await?;
        self.hub.emit("impersonate_ready", json!(text));
        Ok(GenerateResult { text, saved: false })
    }

    // ---------- quiet：不落消息不流式 ----------

    async fn run_quiet(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let chat = self.user.read_chat(&p.avatar, &p.chat_file).map_err(|e| e.to_string())?;
        let history: Vec<Msg> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let input = self.build_assemble_input(p, &history);
        let assembled = crate::prompt_bridge::assemble_quiet(&self.oai, &input, &p.user_message);
        let text = self.call_provider(&assembled, None).await?;
        Ok(GenerateResult { text, saved: false })
    }

    // ---------- continue：nudge / prefill ----------

    async fn run_continue(&self, p: &GenerateParams) -> Result<GenerateResult, String> {
        let mut chat = self.user.read_chat(&p.avatar, &p.chat_file).map_err(|e| e.to_string())?;
        ensure_integrity(&mut chat);
        let history: Vec<Msg> = chat
            .0
            .iter()
            .skip(1)
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        let last = history.last().ok_or("nothing to continue")?.clone();
        let use_prefill = self.oai.continue_prefill && self.oai.chat_completion_source == "claude";

        let input = self.build_assemble_input(p, &history);
        let assembled = if use_prefill {
            crate::prompt_bridge::assemble_continue_prefill(
                &self.oai,
                &input,
                &last.mes,
                "assistant",
            )
        } else {
            crate::prompt_bridge::assemble_continue_nudge(&self.oai, &input, &last.mes)
        };

        let prefill = if use_prefill {
            let postfix = &self.oai.continue_postfix;
            Some(format!("{}{postfix}", last.mes))
        } else {
            None
        };
        let text = self.call_provider(&assembled, prefill.as_deref()).await?;
        let full = if use_prefill {
            // prefill 模式：新文本接在被续消息后
            format!("{}{}", last.mes, text)
        } else {
            last.mes.clone()
        };

        // 更新最后一条消息（appendFinal）
        if let Some(last_v) = chat.0.last_mut() {
            let mut msg: Msg =
                serde_json::from_value(last_v.clone()).map_err(|e| e.to_string())?;
            let now = nast_storage::message_time_stamp();
            msg.mes = full;
            msg.extra.gen_finished = Some(now);
            *last_v = serde_json::to_value(&msg).map_err(|e| e.to_string())?;
        }
        self.save_chat(&p.avatar, &p.chat_file, &chat)?;
        self.hub
            .emit("message_received", json!(chat.0.len() as i64 - 1));
        Ok(GenerateResult { text, saved: true })
    }

    // ---------- 公共 ----------

    fn build_assemble_input<'b>(
        &'b self,
        p: &'b GenerateParams,
        history: &'b [Msg],
    ) -> crate::prompt_bridge::BridgeInput<'b> {
        let messages: Vec<HistoryMessage> = history
            .iter()
            .filter(|m| !m.is_system)
            .map(|m| {
                let role = if m.is_user { "user" } else { "assistant" };
                let mut content = m.mes.clone();
                // names_behavior CONTENT（2）：所有人加前缀；DEFAULT（0）：仅群/强制头像
                let names_behavior = self.oai.character_names_behavior;
                if (names_behavior == 2 && m.extra.kind.as_deref() != Some("narrator"))
                    || (names_behavior == 0 && p.is_group && m.name != "User")
                {
                    if m.extra.kind.as_deref() != Some("narrator") {
                        content = format!("{}: {}", m.name, content);
                    }
                }
                content = content.replace('\r', "");
                HistoryMessage {
                    role: role.into(),
                    content,
                    name: None, // COMPLETION(1) 才带 name 字段，v1 默认 0
                    is_narrator: m.extra.kind.as_deref() == Some("narrator"),
                    injected: false,
                }
            })
            .collect();
        let _ = CC_DUMMY_ID;
        crate::prompt_bridge::BridgeInput {
            oai: &self.oai,
            generation_type: p.generation_type.as_str(),
            name1: "User",
            name2: &p.character.name,
            is_group: p.is_group,
            char_description: p.character.data.description.clone(),
            char_personality: p.character.data.personality.clone(),
            scenario: p.character.data.scenario.clone(),
            persona_description: p.persona_description.clone(),
            persona_position_in_prompt: p.persona_position_in_prompt,
            world_info_before: String::new(), // M3 接入世界书
            world_info_after: String::new(),
            messages,
            message_examples: parse_examples(&p.character.data.mes_example),
            pin_examples: false,
            in_chat_injections: build_injections(p),
            system_prompt_override: {
                let sp = &p.character.data.system_prompt;
                if !sp.is_empty() { Some(sp.clone()) } else { None }
            },
            jailbreak_prompt_override: {
                let phi = &p.character.data.post_history_instructions;
                if !phi.is_empty() { Some(phi.clone()) } else { None }
            },
            cycle_prompt: None,
            last_role: None,
        }
    }

    async fn call_provider(
        &self,
        assembled: &AssembleOutput,
        prefill: Option<&str>,
    ) -> Result<String, String> {
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
            model: self.oai.openai_model.clone(),
            temperature: self.oai.temperature,
            top_p: self.oai.top_p,
            frequency_penalty: self.oai.frequency_penalty,
            presence_penalty: self.oai.presence_penalty,
            max_tokens: self.oai.openai_max_tokens,
            stop: vec![],
            stream: false,
            assistant_prefill: prefill.map(|s| s.to_string()),
            use_sysprompt: true,
            extra_headers: vec![],
            extra_body: json!({}),
        };
        self.provider.generate(&gen_req).await.map_err(|e| e.to_string())
    }

    fn save_chat(&self, avatar: &str, file: &str, chat: &ChatFile) -> Result<(), String> {
        self.user
            .save_chat(avatar, file, chat, false)
            .map_err(|e| e.to_string())
    }

    fn emit_message_deleted(&self, at: usize) {
        self.hub.emit("message_deleted", json!(at));
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

/// mes_example 解析：<START> 分块，{{user}}/{{char}} 行对。
fn parse_examples(raw: &str) -> Vec<ExampleBlock> {
    let mut blocks = Vec::new();
    for block in raw.split("<START>") {
        let mut msgs = Vec::new();
        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("{{user}}:") {
                msgs.push(("user".into(), "example_user".into(), rest.trim().to_string()));
            } else if let Some(rest) = line.strip_prefix("{{char}}:") {
                msgs.push((
                    "assistant".into(),
                    "example_assistant".into(),
                    rest.trim().to_string(),
                ));
            }
        }
        if !msgs.is_empty() {
            blocks.push(ExampleBlock { messages: msgs });
        }
    }
    blocks
}

/// 角色卡 @depth 注入 + AN（v1：仅角色 depth_prompt；AN 由聊天元数据传入）。
fn build_injections(p: &GenerateParams) -> Vec<InChatInjection> {
    let mut out = Vec::new();
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
