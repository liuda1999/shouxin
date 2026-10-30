use chrono::{DateTime, Utc};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::core::Message;
use crate::error::ShouxinError;
use crate::heartbeat::types::{
    ContactProfile, EventMergeCandidate, HeartbeatAction, MajorEvent,
    PersonalInfo, PotentialRelation, Question, QuestionCategory, QuestionPriority,
    SocialRelation,
};
use crate::stores::dictionary::store::DictionaryStore;
use crate::stores::relational::store::RelationalStore;
use crate::traits::llm::LlmClient;

const EVENT_TIME_DIFF_THRESHOLD_HOURS: f64 = 24.0;
const MIN_SIMILARITY_SCORE: f32 = 0.7;
// 默认值与 [heartbeat] 配置一致，实际运行值由 with_profile_limits() 注入
const PROFILE_UPDATE_INTERVAL_HOURS: i64 = 8;
const MAX_PROFILE_UPDATES_PER_DAY: u32 = 2;

/// 从LLM响应中提取JSON字符串
///
/// LLM可能返回：
/// 1. 纯JSON：`{"key": "value"}`
/// 2. markdown代码块：```` ```json\n{...}\n``` ````
/// 3. 带前缀文本：`以下是JSON：\n```json\n{...}\n````
/// 4. 截断的markdown代码块：```` ```json\n{... ````（LLM输出被截断，无结束标记）
/// 5. 截断的JSON：`{"key": "val`（LLM输出被截断，JSON不完整）
fn balance_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '{' | '[' => {
                stack.push(ch);
                out.push(ch);
            }
            '}' => {
                if stack.last() == Some(&'{') {
                    stack.pop();
                }
                out.push(ch);
            }
            ']' => {
                if stack.last() == Some(&'[') {
                    stack.pop();
                }
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }

    if in_string {
        out.push('"');
    }
    while let Some(open) = stack.pop() {
        out.push(if open == '{' { '}' } else { ']' });
    }
    out
}

fn extract_json_from_response(response: &str) -> String {
    let trimmed = response.trim();

    // 情况1：已经是纯JSON
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return repair_truncated_json(trimmed);
    }

    // 情况2/3/4：包含markdown代码块
    if let Some(start) = trimmed.find("```") {
        // 跳过 ``` 和可能的 json 标记
        let after_backticks = &trimmed[start + 3..];
        let after_lang = if after_backticks.starts_with("json") {
            &after_backticks[4..]
        } else {
            after_backticks
        };

        // 找到结束的 ```
        if let Some(end) = after_lang.find("```") {
            let json_content = after_lang[..end].trim();
            return json_content.to_string();
        }

        // 情况4：代码块未闭合（LLM输出被截断），提取从 ``` 后到字符串末尾的内容
        let json_content = after_lang.trim();
        if !json_content.is_empty() {
            return repair_truncated_json(json_content);
        }
    }

    // 情况5：尝试找到第一个 { 到最后一个 } 之间的内容
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return trimmed[start..=end].to_string();
            }
        }
        // 没有 } 或 } 在 { 之前，说明JSON被严重截断
        return repair_truncated_json(&trimmed[start..]);
    }

    trimmed.to_string()
}

/// 修复被截断的JSON
///
/// LLM输出可能因token限制被截断，导致JSON不完整。
/// 本函数通过计算未闭合的 `{` 和 `[` 数量，补全缺失的 `}` 和 `]`。
/// 注意：这是尽力而为的修复，无法保证语义正确性。
fn repair_truncated_json(json_str: &str) -> String {
    // 先尝试直接解析，如果成功就不需要修复
    if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
        return json_str.to_string();
    }

    // 计算未闭合的括号
    let mut open_braces = 0i32;  // {
    let mut open_brackets = 0i32; // [
    let mut in_string = false;
    let mut escape = false;

    for ch in json_str.chars() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => open_braces += 1,
            '}' => open_braces -= 1,
            '[' => open_brackets += 1,
            ']' => open_brackets -= 1,
            _ => {}
        }
    }

    // 如果括号已平衡但仍解析失败，说明有其他语法错误，直接返回原文
    if open_braces == 0 && open_brackets == 0 {
        return json_str.to_string();
    }

    // 修复截断的字符串值：如果字符串未闭合，补上引号
    let mut repaired = json_str.to_string();
    if in_string {
        repaired.push('"');
    }

    // 移除末尾可能的逗号（JSON不允许末尾逗号）
    let trimmed_end = repaired.trim_end();
    if trimmed_end.ends_with(',') {
        repaired = trimmed_end[..trimmed_end.len()-1].to_string();
    }

    // 补全缺失的闭合括号
    for _ in 0..open_brackets.max(0) {
        repaired.push(']');
    }
    for _ in 0..open_braces.max(0) {
        repaired.push('}');
    }

    repaired
}

/// 单次画像生成的默认超时（秒）
///
/// CPU 上 4B 模型生成 500 token 的 JSON 需要数分钟，90s 必然失败。
pub const DEFAULT_PROFILE_TIMEOUT_SECS: u64 = 240;

pub struct HeartbeatAnalyzer {
    llm_client: Arc<dyn LlmClient + Send + Sync>,
    dict_store: Arc<DictionaryStore>,
    rel_store: Arc<RelationalStore>,
    /// 单联系人每天最多几次画像（来自 [heartbeat] 配置）
    profile_max_updates_per_day: u32,
    /// 同一联系人两次画像的最小间隔（小时）
    profile_update_interval_hours: i64,
    /// 单次画像生成的超时（秒）
    ///
    /// 原来写死 90s：Gemma-4 这类 4B 模型在纯 CPU 上生成 500 token 的 JSON
    /// 需要 2-5 分钟，于是**每次都在 90s 被掐断**，画像永远落不了库
    /// （实测 26 次尝试、0 条成功）。改为可配置，默认给足 240s。
    profile_timeout_secs: u64,
    /// 是否跳过"从对话里抽实体"这一步
    ///
    /// 只要后台队列在跑，这一步就是纯重复劳动：
    /// 队列已经对同一批对话做了 contacts/events/todos 抽取，
    /// 心跳再抽一遍会让算力翻倍，并产生重复事件。
    /// 队列自身有失败重放与重启恢复，不需要心跳兜底。
    skip_conversation_extraction: bool,
}

impl HeartbeatAnalyzer {
    pub fn new(
        llm_client: Arc<dyn LlmClient + Send + Sync>,
        dict_store: Arc<DictionaryStore>,
        rel_store: Arc<RelationalStore>,
    ) -> Self {
        Self {
            llm_client,
            dict_store,
            rel_store,
            profile_max_updates_per_day: MAX_PROFILE_UPDATES_PER_DAY,
            profile_update_interval_hours: PROFILE_UPDATE_INTERVAL_HOURS,
            profile_timeout_secs: DEFAULT_PROFILE_TIMEOUT_SECS,
            skip_conversation_extraction: true,
        }
    }

    pub async fn analyze(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let mut actions = Vec::new();

        actions.extend(self.analyze_pending_ambiguities().await?);
        actions.extend(self.analyze_contact_relations().await?);
        actions.extend(self.detect_knowledge_conflicts().await?);
        actions.extend(self.normalize_entities().await?);
        actions.extend(self.check_event_merges().await?);

        Ok(actions)
    }

    /// 注入画像频率限制（来自 [heartbeat] 配置）
    pub fn with_profile_limits(mut self, max_per_day: u32, interval_hours: i64) -> Self {
        self.profile_max_updates_per_day = max_per_day.max(1);
        self.profile_update_interval_hours = interval_hours.max(0);
        self
    }

    /// 注入单次画像生成的超时（秒）
    pub fn with_profile_timeout_secs(mut self, secs: u64) -> Self {
        self.profile_timeout_secs = secs.max(30);
        self
    }

    /// 显式设置是否跳过多余的对话抽取（默认 true）
    pub fn with_skip_conversation_extraction(mut self, skip: bool) -> Self {
        self.skip_conversation_extraction = skip;
        self
    }

    pub async fn process_new_conversations(
        &self,
        since: DateTime<Utc>,
    ) -> Result<(Vec<HeartbeatAction>, Option<DateTime<Utc>>), ShouxinError> {
        let mut actions = Vec::new();

        if self.skip_conversation_extraction {
            // 后台队列是对话抽取的唯一入口，这里直接返回，避免重复 LLM 调用与重复事件。
            debug!("Skipping heartbeat conversation extraction (background queue owns it)");
            return Ok((Vec::new(), None));
        }

        let all_conversations = self.dict_store.get_conversations_since(since).await?;
        let total = all_conversations.len();

        // 只处理**后台队列还没处理完**的对话。
        //
        // 为什么必须过滤：`BackgroundTaskQueue` 已经在抽取同一批对话（contacts/events/todos）。
        // 心跳这里再跑一遍，等于对同一段文本做两次 LLM 抽取，后果是
        //   1) 算力翻倍（200 条语料实测：心跳一启动就把 144 条全部重跑）；
        //   2) 事件重复入库（事件去重靠相似度合并，阈值以上的会合并，其余会变成重复事件）。
        // 因此 completed 的直接跳过；pending/failed 的才由心跳兜底。
        let conversations: Vec<_> = all_conversations
            .into_iter()
            .filter(|c| c.extracted_status != "completed")
            .collect();

        info!(
            "Processing {} conversations not yet extracted since {} (skipped {} already completed)",
            conversations.len(),
            since,
            total - conversations.len()
        );

        let mut last_conv_ts: Option<DateTime<Utc>> = None;

        for conv in &conversations {
            debug!("Processing conversation: {}", conv.content);
            last_conv_ts = Some(conv.timestamp);

            let extracted = self.extract_entities_from_conversation(conv).await?;
            debug!("Extracted: {} contacts, {} events", extracted.contact_names.len(), extracted.events.len());

            for contact_name in extracted.contact_names {
                let action = self.process_contact_name(&contact_name, conv.owner_type, &conv.owner_id).await?;
                if let Some(a) = action {
                    actions.push(a);
                }
            }

            for event in extracted.events {
                let action = self.process_extracted_event(&event, conv.owner_type, &conv.owner_id).await?;
                if let Some(a) = action {
                    actions.push(a);
                }
            }

            for todo_mention in extracted.todo_updates {
                let action = self.process_todo_update(&todo_mention).await?;
                if let Some(a) = action {
                    actions.push(a);
                }
            }
        }

        Ok((actions, last_conv_ts))
    }

    async fn extract_entities_from_conversation(
        &self,
        conv: &crate::stores::dictionary::store::ConversationRecord,
    ) -> Result<ExtractedEntities, ShouxinError> {
        let mut result = ExtractedEntities {
            contact_names: Vec::new(),
            events: Vec::new(),
            todo_updates: Vec::new(),
        };

        let prompt = format!(
            r#"从以下对话中提取实体信息：

**重要语言约束：所有输出内容（姓名、事件摘要、待办描述等所有字段）必须使用中文，禁止使用英语、西班牙语或其他语言。**

对话内容：{}

提取要求：
1. 联系人姓名：所有人名（**重要**：输出完整的姓名，不要截断，避免将"曹操"提取为"曹操很厉"等不完整名称）
2. 事件：包含动作（做、参加、买、去等）的事件描述，**重要：事件摘要必须包含主语姓名**
3. 待办更新：关于待办事项的进度描述
4. 虚拟角色识别：如遇到游戏角色（如林克、马里奥）、AI助手（如ChatGPT）、历史人物（如曹操）等虚拟实体，请在full_name中输出完整名称
5. 事件重要性(importance)：0.0-1.0浮点数。>=0.9为关键事件(如deadline、合同签署)；0.7-0.9为重要事件(如会议、项目讨论)；0.4-0.7为普通事件(如日常沟通)；<0.4为低价值信息

输出格式（JSON）：
{{
  "contact_names": ["姓名1", "姓名2"],
  "contacts_with_full_name": [
    {{
      "name": "姓名",
      "full_name": "完整姓名（优先使用，避免截断）",
      "is_virtual_hint": true或false（是否为虚拟角色的参考标记）
    }}
  ],
  "events": [
    {{
      "summary": "事件摘要（必须包含人物姓名）",
      "timestamp": "事件发生时间",
      "contact_names": ["相关联系人"],
      "importance": 0.5
    }}
  ],
  "todo_updates": [
    {{
      "original_text": "原始描述",
      "status": "完成/未完成/进行中"
    }}
  ]
}}"#,
            conv.content
        );

        let messages = vec![Message::user(&prompt).unwrap()];
        match tokio::time::timeout(Duration::from_secs(90), self.llm_client.chat(messages)).await {
            Ok(Ok(response)) => {
                if let Ok(entities) = serde_json::from_str::<serde_json::Value>(&response) {
                    if let Some(names) = entities.get("contact_names").and_then(|v| v.as_array()) {
                        for name in names {
                            if let Some(n) = name.as_str() {
                                result.contact_names.push(n.to_string());
                            }
                        }
                    }
                    if let Some(contacts) = entities.get("contacts_with_full_name").and_then(|v| v.as_array()) {
                        for contact in contacts {
                            let full_name = contact.get("full_name")
                                .and_then(|v| v.as_str())
                                .map(String::from)
                                .unwrap_or_else(|| {
                                    contact.get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string()
                                });
                            if !full_name.is_empty() && !result.contact_names.contains(&full_name) {
                                result.contact_names.push(full_name);
                            }
                        }
                    }
                    if let Some(events) = entities.get("events").and_then(|v| v.as_array()) {
                        for event_val in events {
                            let summary = event_val.get("summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let timestamp = event_val.get("timestamp").and_then(|v| v.as_str())
                                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or(conv.timestamp);
                            let mut event_contacts = Vec::new();
                            if let Some(names) = event_val.get("contact_names").and_then(|v| v.as_array()) {
                                for name in names {
                                    if let Some(n) = name.as_str() {
                                        event_contacts.push(n.to_string());
                                    }
                                }
                            }
                            // 从LLM输出解析importance，默认0.5
                            let importance = event_val.get("importance")
                                .and_then(|v| v.as_f64())
                                .map(|f| f as f32)
                                .unwrap_or(0.5);
                            // 限制在 0.0-1.0 范围
                            let importance = importance.clamp(0.0, 1.0);
                            result.events.push(ExtractedEvent {
                                summary,
                                timestamp,
                                contact_names: event_contacts,
                                conversation_id: conv.id.clone(),
                                importance,
                            });
                        }
                    }
                    if let Some(todos) = entities.get("todo_updates").and_then(|v| v.as_array()) {
                        for todo_val in todos {
                            let original_text = todo_val.get("original_text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let status = todo_val.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            result.todo_updates.push(TodoUpdate {
                                original_text,
                                status,
                            });
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                warn!("LLM extraction failed for conversation {}: {:?}", conv.id, e);
            }
            Err(_elapsed) => {
                warn!("LLM extraction timed out (90s) for conversation {}", conv.id);
            }
        }

        Ok(result)
    }

    async fn process_contact_name(
        &self,
        name: &str,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<HeartbeatAction>, ShouxinError> {
        let fixed_name = self.fix_name_truncation(name);
        if fixed_name != name {
            warn!("Name truncation detected: '{}' -> '{}', using fixed name", name, fixed_name);
        }
        
        let existing = self.rel_store.find_contact_by_any_name(&fixed_name, owner_type, owner_id).await?;

        match existing {
            Some(contact) => {
                debug!("Found existing contact: {} (id: {})", fixed_name, contact.id);
                let aliases_str = &contact.aliases;
                let aliases: Vec<String> = serde_json::from_str(aliases_str).unwrap_or_default();

                if !aliases.contains(&fixed_name.to_string()) {
                    let mut new_aliases = aliases;
                    new_aliases.push(fixed_name.to_string());
                    let mut updated_contact = contact.clone();
                    updated_contact.aliases = serde_json::to_string(&new_aliases).unwrap_or_default();
                    self.rel_store.upsert_contact_index(&updated_contact).await?;
                    return Ok(Some(HeartbeatAction::UpdateContactSummary {
                        contact_id: contact.id,
                        new_summary: serde_json::to_string(&updated_contact).unwrap_or_default(),
                    }));
                }
            }
            None => {
                // 先检查是否为虚拟角色，虚拟角色不创建联系人副本
                let is_virtual = self.check_virtual_entity(&fixed_name).await.unwrap_or(false);
                if is_virtual {
                    info!("Contact '{}' is a virtual entity, skipping creation", fixed_name);
                    return Ok(None);
                }

                info!("Creating new contact: {}", fixed_name);
                let aliases = vec![fixed_name.to_string()];
                let new_contact = crate::core::ContactIndex {
                    id: Uuid::new_v4().to_string(),
                    name: fixed_name.to_string(),
                    disambiguation: String::new(),
                    aliases: serde_json::to_string(&aliases).unwrap_or_default(),
                    is_self: false,
                    created_at: Utc::now(),
                    last_interaction_at: Utc::now(),
                    related_contact_ids: String::new(),
                    relationship_tags: String::new(),
                    relation_weight: String::new(),
                    background: String::new(),
                    notes: String::new(),
                    owner_type,
                    owner_id: owner_id.to_string(),
                    is_agent: false,
                    parent_agent_id: None,
                    is_virtual: false,
                    is_archived: false,
                    archived_at: None,
                    activity_score: 0.0,
                };
                self.rel_store.upsert_contact_index(&new_contact).await?;
                let new_contact_id = new_contact.id.clone();
                let new_contact_summary = serde_json::to_string(&new_contact).unwrap_or_default();
                return Ok(Some(HeartbeatAction::UpdateContactSummary {
                    contact_id: new_contact_id,
                    new_summary: new_contact_summary,
                }));
            }
        }

        Ok(None)
    }

    async fn check_virtual_entity(&self, name: &str) -> Result<bool, ShouxinError> {
        let match_result = self.rel_store.check_virtual_character_match(name).await?;
        Ok(match_result.is_some())
    }

    fn fix_name_truncation(&self, name: &str) -> String {
        let truncated_markers = ["很", "的", "了", "是", "在", "和", "与", "或", "很厉", "厉", "害"];
        
        for marker in &truncated_markers {
            if name.ends_with(marker) {
                let cleaned = name.trim_end_matches(marker).trim();
                if !cleaned.is_empty() && cleaned.chars().all(|c| {
                    !c.is_ascii_digit() && !matches!(c, '+' | '-' | '*' | '/')
                }) {
                    debug!("Fixed truncated name '{}' -> '{}'", name, cleaned);
                    return cleaned.to_string();
                }
            }
        }
        
        if name.chars().any(|c| matches!(c, '0'..='9')) && name.len() > 3 {
            let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                let cleaned = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim();
                if !cleaned.is_empty() {
                    debug!("Fixed name with trailing digits '{}' -> '{}'", name, cleaned);
                    return cleaned.to_string();
                }
            }
        }
        
        name.to_string()
    }

    async fn process_extracted_event(
        &self,
        event: &ExtractedEvent,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<Option<HeartbeatAction>, ShouxinError> {
        // 去重检查：如果已存在相同 summary 的事件，跳过创建
        let owner_type_str = format!("{:?}", owner_type);
        let existing_events = self.rel_store
            .search_events_by_summary(&event.summary, &owner_type_str, owner_id)
            .await?;
        if !existing_events.is_empty() {
            debug!(
                "事件已存在，跳过重复创建: {} (existing event_id={})",
                event.summary.chars().take(50).collect::<String>(),
                existing_events[0].event_id
            );
            return Ok(None);
        }

        let mut contact_ids = Vec::new();
        let mut contact_names_to_find: Vec<String> = event.contact_names.clone();

        for name in &event.contact_names {
            if let Ok(Some(contact)) = self.rel_store.find_contact_by_any_name(name, owner_type, owner_id).await {
                contact_ids.push(contact.id);
            }
        }

        if contact_ids.is_empty() && !event.contact_names.is_empty() {
            for name in &event.contact_names {
                if let Ok(Some(contact)) = self.rel_store.find_contact_by_any_name(name, owner_type, owner_id).await {
                    if !contact_ids.contains(&contact.id) {
                        contact_ids.push(contact.id);
                    }
                }
            }
        }

        if contact_ids.is_empty() {
            debug!("No contact IDs found from names, checking fallback...");
            let existing_contacts = self.rel_store.get_active_contacts(owner_type, owner_id, 50).await?;
            debug!("Fallback: looking for contacts in event '{}', found {} active contacts",
                event.summary, existing_contacts.len());
            for contact in existing_contacts.iter().take(20) {
                debug!("Checking if '{}' contains '{}'", event.summary, contact.name);
                if event.summary.contains(&contact.name) {
                    contact_ids.push(contact.id.clone());
                    break;
                }
            }
            if contact_ids.is_empty() {
                debug!("No contact found for event '{}'", event.summary);
            }
        }

        // 使用 LLM 提取的 importance，而非硬编码 0.5
        let importance_label = if event.importance >= 0.8 {
            "high".to_string()
        } else if event.importance >= 0.5 {
            "medium".to_string()
        } else {
            "low".to_string()
        };

        let mut new_event = crate::core::EventIndex {
            event_id: Uuid::new_v4().to_string(),
            contact_ids: serde_json::to_string(&contact_ids).unwrap_or_default(),
            owner_id: owner_id.to_string(),
            owner_type,
            time_range_start: event.timestamp,
            time_range_end: event.timestamp,
            importance: importance_label,
            status: "pending".to_string(),
            created_at: Utc::now(),
            completed_at: None,
            summary: event.summary.clone(),
            title: None,
            event_type: String::new(),
            deadline: None,
            auto_invalid_at: None,
            is_auto_invalid: false,
            is_merged: false,
            merge_target_id: None,
            last_merge_check_at: None,
            event_time: None,
            importance_score: event.importance,
            is_archived: false,
            archived_at: None,
        };
        new_event.compute_auto_invalid_at();

        if let Err(e) = self.rel_store.insert_event_index(&new_event).await {
            warn!("Failed to insert event: {:?}", e);
        }

        Ok(None)
    }

    async fn process_todo_update(&self, update: &TodoUpdate) -> Result<Option<HeartbeatAction>, ShouxinError> {
        debug!("Processing todo update: {} -> {}", update.original_text, update.status);
        Ok(None)
    }

    async fn analyze_pending_ambiguities(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let mut actions = Vec::new();

        let pending_ambiguities = self.rel_store.get_pending_ambiguities().await?;

        for ambiguity in pending_ambiguities {
            if let Some(ref todo_id) = ambiguity.todo_id {
                let question = Question {
                    id: todo_id.clone(),
                    priority: QuestionPriority::Medium,
                    category: QuestionCategory::EntityDisambiguation,
                    title: format!("关于{}的歧义需要澄清", ambiguity.entity_id),
                    detail: ambiguity.description,
                    context: Some(ambiguity.context_summary),
                    created_at: Utc::now(),
                    expires_at: Some(Utc::now() + chrono::Duration::days(3)),
                    related_contact_ids: ambiguity.related_contact_ids,
                    related_event_ids: ambiguity.related_event_ids,
                };
                actions.push(HeartbeatAction::AskUser(question));
            }
        }

        Ok(actions)
    }

    async fn analyze_contact_relations(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let mut actions = Vec::new();

        let contacts = self.rel_store.get_contacts_needing_profile(self.profile_max_updates_per_day).await?;

        for contact in contacts {
            if let Some(profile) = self.generate_contact_profile(&contact).await? {
                actions.push(HeartbeatAction::UpdateContactSummary {
                    contact_id: contact.id.clone(),
                    new_summary: serde_json::to_string(&profile).unwrap_or_default(),
                });
            }
        }

        Ok(actions)
    }

    async fn generate_contact_profile(
        &self,
        contact: &crate::core::ContactIndex,
    ) -> Result<Option<ContactProfile>, ShouxinError> {
        let can_update = self.can_update_profile(&contact.id).await?;
        if !can_update {
            debug!("Skipping profile update for {}: frequency limit", contact.name);
            return Ok(None);
        }

        debug!("Generating profile for contact: {}", contact.name);

        let events = self.rel_store.search_events_by_contact(
            &contact.id,
            contact.owner_type,
            &contact.owner_id
        ).await?;

        let events_summary: String = events.iter()
            .map(|e| format!("[{}] {}", e.time_range_start.format("%Y-%m-%d"), e.summary))
            .collect::<Vec<_>>()
            .join("\n");

        let contact_aliases: Vec<String> = serde_json::from_str(&contact.aliases).unwrap_or_default();
        let contact_tags: Vec<String> = serde_json::from_str(&contact.relationship_tags).unwrap_or_default();

        // 找出近期包含该联系人（或其别名）的对话原文片段
        //
        // 为什么需要：原来只把"事件摘要"喂给模型，而"老王是我的同事""小李是老王的搭档"
        // 这类**关系表述**往往在对话原文里、不在事件摘要里，模型只能回一句"未知"。
        // 结果就是画像里的 social_relations 永远空，联系人之间的关系无从建立。
        let mut names: Vec<String> = vec![contact.name.clone()];
        names.extend(contact_aliases.iter().cloned());
        let mut conversation_block = String::new();
        if let Ok(recent) = self
            .dict_store
            .get_recent_conversations_for_owner(contact.owner_type, &contact.owner_id, 60)
            .await
        {
            let mut picked = 0;
            for conv in recent.iter() {
                if picked >= 3 {
                    break;
                }
                if names.iter().any(|n| !n.is_empty() && conv.content.contains(n.as_str())) {
                    let snippet: String = conv.content.chars().take(300).collect();
                    conversation_block.push_str(&format!("- {}\n", snippet));
                    picked += 1;
                }
            }
        }

        // 没有证据就不生成：不占额度、不记尝试次数。
        //
        // 产品语义（用户明确要求）：某个人今天完全没被提到、也没有和他相关的事，
        // 这一天就不更新他——这是心跳该做的"该不该更新"的判断，
        // 而不是拿"每天 2 次"去硬卡。这里就是那个判断：
        // 既没有事件、也没有任何一段对话提到他 → 直接跳过。
        if events.is_empty() && conversation_block.trim().is_empty() {
            debug!("跳过 {}：没有任何事件/对话证据，本轮不更新", contact.name);
            return Ok(None);
        }

        // 记账放在**取证之后、调模型之前**：
        // 这样"没证据"的跳过不消耗额度；而真的调了模型（哪怕随后进程崩了）
        // 也算一次尝试，避免同一条输入让服务反复重启-崩溃。
        self.record_profile_update(&contact.id).await?;

        let prompt = format!(
            r#"根据以下信息生成联系人画像。

**输出格式（必须严格遵守）：**
- 只输出一个 JSON 对象，不要 markdown 代码块、不要解释文字。
- **JSON 的 key 必须保持英文原样**：personal_info / social_relations / major_events / objective_evaluation / potential_relations。
- **只有 value 使用中文**。绝对不要把 key 翻译成中文（例如不要输出 "职业"、"社会关系"）。
- social_relations 必须是**数组**，每个元素形如
  {{"relation_type":"同事","related_contact_name":"小李","confidence":0.8}}；
  无法确定的对象把 related_contact_name 省略，但 relation_type 必须给出具体关系（如 同事/上级/朋友/家人），
  确实未知时用 "未知"。

示例（仅示范结构与 key 名）：
{{"personal_info":{{"known_occupation":"后端工程师","known_age":"未知","known_interests":["分布式系统"]}},
"social_relations":[{{"relation_type":"同事","related_contact_name":"小李","confidence":0.8}}],
"major_events":[{{"summary":"参加架构评审会"}}],
"objective_evaluation":"工作认真，专注后端",
"potential_relations":[]}}

联系人姓名：{}
已知别名：{:?}
已知关系：{:?}
相关事件：
{}
相关对话原文片段（关系表述优先以此为准）：
{}

请生成JSON格式的联系人画像，包含：
1. personal_info: 个人信息（职业、年龄、兴趣等）
2. social_relations: 社会关系（与用户的关系类型）
3. major_events: 重大事件（最重要的事件）
4. objective_evaluation: 对该联系人的客观评价
5. potential_relations: 可能存在但未确认的关系"#,
            contact.name,
            contact_aliases,
            contact_tags,
            events_summary,
            if conversation_block.is_empty() {
                "（无）".to_string()
            } else {
                conversation_block
            }
        );

        let messages = vec![Message::user(&prompt).unwrap()];
        match tokio::time::timeout(
            Duration::from_secs(self.profile_timeout_secs),
            self.llm_client.chat(messages),
        )
        .await
        {
            Ok(Ok(response)) => {
                // P0-1: LLM可能返回markdown代码块包裹的JSON，需要提取
                let json_str = balance_json(&extract_json_from_response(&response));
                if let Ok(profile_json) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    let profile = ContactProfile {
                        contact_id: contact.id.clone(),
                        personal_info: PersonalInfo {
                            name: contact.name.clone(),
                            aliases: contact_aliases,
                            known_occupation: profile_json.get("personal_info")
                                .and_then(|p| p.get("known_occupation"))
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            known_age: profile_json.get("personal_info")
                                .and_then(|p| p.get("known_age"))
                                .and_then(|v| v.as_str())
                                .map(String::from),
                            known_interests: profile_json.get("personal_info")
                                .and_then(|p| p.get("known_interests"))
                                .and_then(|v| v.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default(),
                        },
                        // P0-2: 解析LLM返回的social_relations，兼容字符串数组和对象数组两种格式
                        // 兼容三种返回形态：
                        //   1) ["朋友", "同事"]
                        //   2) [{"relation_type":"同事","related_contact_name":"小李","confidence":0.8}]
                        //   3) {"与用户的关系类型":"同事"} / {"relationship_type":"同事"}
                        //      —— 小模型常把数组写成对象、甚至把 key 翻成中文，这里都兜住，
                        //      否则社会关系会被整段丢弃，relation_tags/related_contacts 永远是空。
                        social_relations: {
                            let value = profile_json.get("social_relations");
                            let items: Vec<serde_json::Value> = match value {
                                Some(serde_json::Value::Array(arr)) => arr.clone(),
                                Some(serde_json::Value::Object(_)) => vec![value.unwrap().clone()],
                                _ => Vec::new(),
                            };
                            items
                                .iter()
                                .filter_map(|v| {
                                    if let Some(s) = v.as_str() {
                                        let s = s.trim();
                                        if s.is_empty() {
                                            return None;
                                        }
                                        return Some(SocialRelation {
                                            relation_type: s.to_string(),
                                            related_contact_id: None,
                                            related_contact_name: None,
                                            confidence: 0.5,
                                        });
                                    }

                                    // 先找 relation_type / relationship_type，
                                    // 再退化到"该对象里第一个非空字符串值"（覆盖中文 key）
                                    let mut relation_type = v
                                        .get("relation_type")
                                        .or_else(|| v.get("relationship_type"))
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("")
                                        .trim()
                                        .to_string();
                                    if relation_type.is_empty() {
                                        if let Some(obj) = v.as_object() {
                                            for (_, val) in obj {
                                                if let Some(s) = val.as_str() {
                                                    let s = s.trim();
                                                    if !s.is_empty() {
                                                        relation_type = s.to_string();
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if relation_type.is_empty() {
                                        return None;
                                    }
                                    Some(SocialRelation {
                                        relation_type,
                                        related_contact_id: v
                                            .get("related_contact_id")
                                            .and_then(|x| x.as_str())
                                            .map(String::from),
                                        related_contact_name: v
                                            .get("related_contact_name")
                                            .and_then(|x| x.as_str())
                                            .map(String::from),
                                        confidence: v
                                            .get("confidence")
                                            .and_then(|x| x.as_f64())
                                            .map(|f| f as f32)
                                            .unwrap_or(0.5),
                                    })
                                })
                                .collect()
                        },
                        major_events: events.iter().take(10).map(|e| MajorEvent {
                            event_id: e.event_id.clone(),
                            summary: e.summary.clone(),
                            timestamp: e.time_range_start,
                            importance: e.importance.clone(),
                            sentiment: String::new(),
                        }).collect(),
                        objective_evaluation: profile_json.get("objective_evaluation")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        // P0-2: 解析LLM返回的potential_relations，兼容字符串数组和对象数组两种格式
                        // 格式1: ["可能认识李四"] → inferred_relation 字符串
                        // 格式2: [{"target_contact_name": "李四", "inferred_relation": "同事", "confidence": 0.6, "reasoning": "..."}]
                        potential_relations: profile_json.get("potential_relations")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| {
                                if let Some(s) = v.as_str() {
                                    let s = s.trim();
                                    if s.is_empty() { return None; }
                                    return Some(PotentialRelation {
                                        target_contact_id: String::new(),
                                        target_contact_name: String::new(),
                                        inferred_relation: s.to_string(),
                                        confidence: 0.3,
                                        reasoning: String::new(),
                                    });
                                }
                                let inferred_relation = v.get("inferred_relation")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .trim()
                                    .to_string();
                                if inferred_relation.is_empty() { return None; }
                                Some(PotentialRelation {
                                    target_contact_id: String::new(),
                                    target_contact_name: v.get("target_contact_name")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    inferred_relation,
                                    confidence: v.get("confidence")
                                        .and_then(|x| x.as_f64())
                                        .map(|f| f as f32)
                                        .unwrap_or(0.3),
                                    reasoning: v.get("reasoning")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                })
                            }).collect())
                            .unwrap_or_default(),
                        last_updated: Utc::now(),
                    };

                    return Ok(Some(profile));
                } else {
                    warn!("Failed to parse profile JSON for contact {}: response={}", contact.name, response.chars().take(200).collect::<String>());
                    // 尝试次数已在调用模型前记录，这里不再重复记（否则一次失败就用掉两次额度）
                }
            }
            Ok(Err(e)) => {
                warn!("Failed to generate profile for contact {}: {:?}", contact.id, e);
                // 同上：尝试次数已在调用前记录
            }
            Err(_elapsed) => {
                warn!(
                    "Profile generation timed out for contact {} after {}s",
                    contact.id, self.profile_timeout_secs
                );
                // P0-2: 超时也记录一次尝试，避免死循环
                self.record_profile_update(&contact.id).await?;
            }
        }

        Ok(None)
    }

    async fn can_update_profile(&self, contact_id: &str) -> Result<bool, ShouxinError> {
        let log = self.rel_store.get_profile_update_log(contact_id).await?;

        let today = Utc::now().format("%Y-%m-%d").to_string();
        let today_updates: u32 = log.iter()
            .filter(|l| l.date == today)
            .map(|l| l.update_count)
            .sum();

        debug!("can_update_profile check for {}: today_updates={}, max={}", contact_id, today_updates, self.profile_max_updates_per_day);

        if today_updates >= self.profile_max_updates_per_day {
            debug!("Profile update blocked for {}: daily limit reached", contact_id);
            return Ok(false);
        }

        if let Some(last_update) = log.first() {
            if last_update.date == today && last_update.update_count >= self.profile_max_updates_per_day {
                let hours_since = (Utc::now() - last_update.last_update).num_hours();
                if hours_since < self.profile_update_interval_hours {
                    debug!("Profile update blocked for {}: interval not elapsed ({}h < {}h)", contact_id, hours_since, PROFILE_UPDATE_INTERVAL_HOURS);
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    async fn record_profile_update(&self, contact_id: &str) -> Result<(), ShouxinError> {
        self.rel_store.record_profile_update(contact_id).await
    }

    async fn detect_knowledge_conflicts(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let recent_convs = self.dict_store
            .get_conversations_since_v2(Utc::now() - chrono::Duration::days(7), 30)
            .await?;

        if recent_convs.len() < 2 {
            debug!("detect_knowledge_conflicts: only {} conversations, need at least 2", recent_convs.len());
            return Ok(Vec::new());
        }

        debug!("detect_knowledge_conflicts: analyzing {} recent conversations", recent_convs.len());

        // 拼接时必须有**字符预算**。
        //
        // 原实现把最多 30 段对话全量拼进一个提示词：200 条压测语料下实测拼出
        // 47658 字符（约 1.5 万 token），远超 context=4096，
        // 结果每轮心跳都要白跑 30–90 秒，还看不到完整上下文。
        // 这里从**最新**往前取，累计到 3000 字符为止。
        const BUDGET_CHARS: usize = 3000;
        let mut picked: Vec<String> = Vec::new();
        let mut used = 0usize;
        for c in recent_convs.iter().rev() {
            let line = format!("[{}] {}", c.timestamp, c.content);
            let len = line.chars().count();
            if used + len > BUDGET_CHARS && !picked.is_empty() {
                break;
            }
            used += len;
            picked.push(line);
            if used >= BUDGET_CHARS {
                break;
            }
        }
        picked.reverse();
        let combined_text = picked.join("\n\n");

        let prompt = format!(
            r#"你是一个智能歧义检测助手。分析以下对话历史，判断是否存在需要用户澄清的歧义、矛盾或不确定信息。

**重要语言约束：所有输出内容（description、related_entities 等所有字段）必须使用中文，禁止使用英语、西班牙语或其他语言。**

要求：
1. 如果发现事实矛盾（用户先后说法相反或矛盾），必须报告。
2. 如果发现指代不清（如"小王"可能对应多个人），必须报告。
3. 如果发现关系不确定（如"可能是朋友"），必须报告。
4. 如果发现"买/不买"、"要/不要"、"去/不去"等对立表态，必须报告。
5. 如果不确定，保守处理，宁可报告也不要遗漏。

对话内容：
{}

请严格按以下JSON格式输出（仅输出JSON，不要其他内容）：
{{
  "has_ambiguity": true或false,
  "type": "FactualContradiction"或"PronounAmbiguity"或"RelationUncertain"或"SelfContradiction"（仅当has_ambiguity为true时）,
  "description": "详细描述歧义内容（仅当has_ambiguity为true时）",
  "related_entities": ["相关实体列表，字符串形式（仅当has_ambiguity为true时）"]
}}

如果has_ambiguity为false，所有其他字段都为空字符串或空数组。"#,
            combined_text
        );

        let messages = vec![Message::user(&prompt).unwrap()];
        let response = match tokio::time::timeout(Duration::from_secs(120), self.llm_client.chat(messages)).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                warn!("LLM call failed for ambiguity detection: {}", e);
                return Ok(Vec::new());
            }
            Err(_elapsed) => {
                warn!("LLM call timed out for ambiguity detection (120s)");
                return Ok(Vec::new());
            }
        };

        let cleaned = response.trim();
        let json_start = cleaned.find('{').unwrap_or(0);
        let json_end = cleaned.rfind('}').map(|p| p + 1).unwrap_or(cleaned.len());
        let json_str = &cleaned[json_start..json_end];

        #[derive(serde::Deserialize)]
        struct AmbiguityResult {
            #[serde(default)]
            has_ambiguity: bool,
            #[serde(default)]
            ambiguity_type: String,
            #[serde(default)]
            description: String,
            #[serde(default)]
            related_entities: Vec<String>,
        }

        let result: AmbiguityResult = match serde_json::from_str(json_str) {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to parse ambiguity detection response: {} | raw: {}", e, json_str);
                let has_buy_negate = recent_convs.iter().any(|c| {
                    c.content.contains("买") && (c.content.contains("不买") || c.content.contains("不要买"))
                });
                if has_buy_negate {
                    return Ok(vec![HeartbeatAction::AskUser(Question {
                        id: Uuid::new_v4().to_string(),
                        priority: QuestionPriority::High,
                        category: QuestionCategory::EntityDisambiguation,
                        title: "需要澄清的决定".to_string(),
                        detail: "检测到您对某个决定的态度存在矛盾，请确认您的最终决定。".to_string(),
                        context: Some(combined_text.chars().take(500).collect()),
                        created_at: Utc::now(),
                        expires_at: Some(Utc::now() + chrono::Duration::days(3)),
                        related_contact_ids: vec![],
                        related_event_ids: vec![],
                    })]);
                }
                return Ok(Vec::new());
            }
        };

        if !result.has_ambiguity {
            return Ok(Vec::new());
        }

        let ambiguity_type = if result.ambiguity_type.is_empty() {
            "SelfContradiction".to_string()
        } else {
            result.ambiguity_type
        };

        let description = if result.description.is_empty() {
            format!("检测到潜在矛盾或歧义，请澄清。")
        } else {
            result.description
        };

        let entity_ids: Vec<String> = result.related_entities.into_iter().take(5).collect();

        let owner_id = recent_convs.first().map(|c| c.owner_id.clone()).unwrap_or_else(|| "default_user".to_string());
        match self.rel_store.create_ambiguity_todo(
            &ambiguity_type,
            "contact",
            &entity_ids.join(","),
            &description,
            &format!("对话分析中发现：{}", description),
            crate::core::contact::OwnerType::User,
            &owner_id,
        ).await {
            Ok(_) => info!("Created ambiguity todo from conflict detection"),
            Err(e) => warn!("Failed to create ambiguity todo: {}", e),
        }

        Ok(vec![HeartbeatAction::AskUser(Question {
            id: Uuid::new_v4().to_string(),
            priority: QuestionPriority::High,
            category: if entity_ids.len() > 1 {
                QuestionCategory::EntityDisambiguation
            } else {
                QuestionCategory::ConflictResolution
            },
            title: format!("需要澄清：{}", ambiguity_type),
            detail: description,
            context: Some(combined_text.chars().take(500).collect()),
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + chrono::Duration::days(3)),
            related_contact_ids: entity_ids.clone(),
            related_event_ids: vec![],
        })])
    }

    async fn normalize_entities(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let mut actions = Vec::new();

        let merge_candidates = self.rel_store.find_contact_merge_candidates().await?;

        for candidate in merge_candidates {
            if candidate.confidence_score >= 0.8 {
                actions.push(HeartbeatAction::MergeEntities {
                    canonical_id: candidate.primary_contact_id,
                    alias_ids: vec![candidate.merged_contact_id],
                });
            } else {
                let question = Question {
                    id: Uuid::new_v4().to_string(),
                    priority: QuestionPriority::Medium,
                    category: QuestionCategory::ContactClarification,
                    title: format!("是否应该合并联系人?"),
                    detail: format!(
                        "系统检测到以下联系人可能是同一人:\n1. {} (别名: {:?})\n2. {} (别名: {:?})\n\
                         相似度: {:.0}%",
                        candidate.primary_contact_id,
                        candidate.shared_aliases,
                        candidate.merged_contact_id,
                        Vec::<String>::new(),
                        candidate.confidence_score * 100.0
                    ),
                    context: None,
                    created_at: Utc::now(),
                    expires_at: Some(Utc::now() + chrono::Duration::days(7)),
                    related_contact_ids: vec![candidate.primary_contact_id.clone(), candidate.merged_contact_id.clone()],
                    related_event_ids: Vec::new(),
                };
                actions.push(HeartbeatAction::AskUser(question));
            }
        }

        Ok(actions)
    }

    /// 检查近期 pending 事件之间的合并可能性
    /// 每次心跳最多检查 5 个事件，通过关键词预筛选减少 LLM 调用
    async fn check_event_merges(&self) -> Result<Vec<HeartbeatAction>, ShouxinError> {
        let events = self.rel_store.get_recent_pending_events_for_merge(5).await?;
        if events.len() < 2 {
            return Ok(Vec::new());
        }

        let mut actions = Vec::new();
        let mut checked_pairs = std::collections::HashSet::new();

        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                let pair_key = format!("{}:{}", events[i].event_id, events[j].event_id);
                if checked_pairs.contains(&pair_key) {
                    continue;
                }
                checked_pairs.insert(pair_key);

                // 关键词预筛选：至少 2 个共同关键词才调用 LLM
                let shared = self.extract_shared_keywords(&events[i].summary, &events[j].summary)?;
                if shared.len() < 2 {
                    continue;
                }

                // 时间差预筛选
                let time_diff = (events[i].time_range_start - events[j].time_range_start).num_hours().abs() as f64;
                if time_diff > EVENT_TIME_DIFF_THRESHOLD_HOURS {
                    continue;
                }

                if let Some(candidate) = self.check_event_merge(&events[i], &events[j]).await? {
                    let _ = self.rel_store.insert_pending_event_merge(
                        &candidate.event1_id,
                        Some(&candidate.event2_id),
                        &events[i].summary,
                        Some(&events[j].summary),
                        candidate.similarity_score,
                        events[i].owner_type,
                        &events[i].owner_id,
                    ).await;
                    info!("Detected event merge candidate: {} <-> {} (score: {:.2})",
                        candidate.event1_id, candidate.event2_id, candidate.similarity_score);
                }
            }
            // 更新事件的合并检查时间
            let _ = self.rel_store.update_event_merge_check_time(&events[i].event_id).await;
        }

        Ok(actions)
    }

    pub async fn check_event_merge(&self, event1: &crate::core::EventIndex, event2: &crate::core::EventIndex) -> Result<Option<EventMergeCandidate>, ShouxinError> {
        let time_diff = (event1.time_range_start - event2.time_range_start).num_hours().abs() as f64;

        if time_diff > EVENT_TIME_DIFF_THRESHOLD_HOURS {
            return Ok(None);
        }

        let shared_keywords = self.extract_shared_keywords(&event1.summary, &event2.summary)?;
        let keyword_similarity = if shared_keywords.len() >= 2 { 0.8 } else { 0.0 };

        let contact_ids1: Vec<String> = serde_json::from_str(&event1.contact_ids).unwrap_or_default();
        let contact_ids2: Vec<String> = serde_json::from_str(&event2.contact_ids).unwrap_or_default();

        let contact1_set: HashSet<_> = contact_ids1.iter().collect();
        let contact2_set: HashSet<_> = contact_ids2.iter().collect();

        let shared_contacts = contact1_set.intersection(&contact2_set).count();
        let total_contacts = contact1_set.union(&contact2_set).count();
        let contact_ratio = if total_contacts > 0 {
            shared_contacts as f32 / total_contacts as f32
        } else {
            0.0
        };

        let llm_similarity = self.llm_check_event_similarity(&event1.summary, &event2.summary).await?;

        let total_score = keyword_similarity * 0.3 + contact_ratio * 0.3 + llm_similarity * 0.4;

        if total_score >= MIN_SIMILARITY_SCORE && time_diff <= EVENT_TIME_DIFF_THRESHOLD_HOURS {
            return Ok(Some(EventMergeCandidate {
                event1_id: event1.event_id.clone(),
                event2_id: event2.event_id.clone(),
                time_diff_hours: time_diff,
                similarity_score: total_score,
                shared_keywords,
                shared_contact_ratio: contact_ratio,
                merge_reason: format!("事件相似度 {:.0}%, 时间差 {:.1} 小时, 共同联系人 {:.0}%",
                    total_score * 100.0, time_diff, contact_ratio * 100.0),
                is_auto_merge: total_score >= 0.85,
            }));
        }

        Ok(None)
    }

    async fn llm_check_event_similarity(&self, summary1: &str, summary2: &str) -> Result<f32, ShouxinError> {
        let prompt = format!(
            r#"判断以下两个事件描述是否描述的是同一件事：

事件1: {}
事件2: {}

回答格式：直接输出0到1之间的数字，表示相似度分数。例如：0.85
只输出数字，不要输出其他内容。"#,
            summary1, summary2
        );

        let messages = vec![Message::user(&prompt).unwrap()];
        match tokio::time::timeout(Duration::from_secs(30), self.llm_client.chat(messages)).await {
            Ok(Ok(response)) => {
                if let Ok(score) = response.trim().parse::<f32>() {
                    return Ok(score.clamp(0.0, 1.0));
                }
            }
            Ok(Err(e)) => {
                warn!("LLM similarity check failed: {:?}", e);
            }
            Err(_elapsed) => {
                warn!("LLM similarity check timed out (30s)");
            }
        }

        Ok(0.0)
    }

    fn extract_shared_keywords(&self, text1: &str, text2: &str) -> Result<Vec<String>, ShouxinError> {
        let action_verbs = ["做", "参加", "买", "去", "见", "吃", "开会", "完成", "学习", "工作"];
        let mut shared = Vec::new();

        for verb in action_verbs {
            if text1.contains(verb) && text2.contains(verb) {
                shared.push(verb.to_string());
            }
        }

        Ok(shared)
    }
}

struct ExtractedEntities {
    contact_names: Vec<String>,
    events: Vec<ExtractedEvent>,
    todo_updates: Vec<TodoUpdate>,
}

struct ExtractedEvent {
    summary: String,
    timestamp: DateTime<Utc>,
    contact_names: Vec<String>,
    conversation_id: String,
    /// LLM 提取的重要性分数 (0.0-1.0)，默认 0.5
    importance: f32,
}

struct TodoUpdate {
    original_text: String,
    status: String,
}
