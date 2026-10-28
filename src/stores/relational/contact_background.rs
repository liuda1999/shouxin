//! 联系人背景总结服务
//!
//! 实现懒惰触发的联系人背景动态总结

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::ShouxinError;
use crate::llm::OllamaLlmClient;
use crate::stores::dictionary::store::{ContactEventRecord, DictionaryStore};
use crate::traits::llm::LlmClient;

/// 总结刷新间隔（天）
const SUMMARY_REFRESH_DAYS: i64 = 3;

/// 历史事件查询数量限制
const EVENT_QUERY_LIMIT: usize = 20;

/// 最大总结大小（字节）
const MAX_SUMMARY_SIZE: usize = 1024;

/// 联系人背景总结结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactBackground {
    /// 职业
    pub profession: Option<String>,
    /// 关系网
    pub relationships: Vec<String>,
    /// 与用户关系
    pub user_relationship: Option<String>,
    /// 最近共同活动
    pub recent_activities: Vec<String>,
    /// 风评描述（行为模式和用户情感倾向）
    pub reputation: Option<String>,
    /// 总结生成时间
    pub summarized_at: DateTime<Utc>,
}

impl ContactBackground {
    pub fn new() -> Self {
        Self {
            profession: None,
            relationships: Vec::new(),
            user_relationship: None,
            recent_activities: Vec::new(),
            reputation: None,
            summarized_at: Utc::now(),
        }
    }

    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

impl Default for ContactBackground {
    fn default() -> Self {
        Self::new()
    }
}

/// 联系人背景总结服务
pub struct ContactBackgroundSummarizer {
    /// LLM 客户端
    llm_client: OllamaLlmClient,
    /// 字典库
    dictionary: Arc<DictionaryStore>,
}

impl ContactBackgroundSummarizer {
    pub fn new(llm_client: OllamaLlmClient, dictionary: Arc<DictionaryStore>) -> Self {
        Self {
            llm_client,
            dictionary,
        }
    }

    /// 生成背景总结
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一标识
    /// - `contact_name`: 联系人姓名
    /// - `current_background`: 当前背景总结（用于解析失败时保留旧值）
    ///
    /// # 返回
    /// JSON 格式的背景总结字符串
    ///
    /// # 说明
    /// 从字典库的 contact_event_history Tree 中读取该联系人的历史事件档案，
    /// 调用 LLM 生成结构化背景总结，包含风评描述
    pub async fn generate_background_summary(
        &self,
        contact_id: &str,
        contact_name: &str,
        current_background: Option<&str>,
    ) -> Result<String, ShouxinError> {
        // 从字典库获取该联系人的历史事件记录（最近 20 条）
        let events: Vec<crate::stores::dictionary::store::ContactEventRecord> = self
            .dictionary
            .get_contact_events(contact_id, Some(EVENT_QUERY_LIMIT))
            .await?;

        // 如果没有历史事件，返回空背景
        if events.is_empty() {
            let background = ContactBackground::new();
            return background
                .to_json_string()
                .map_err(|e| ShouxinError::SerializationError(e.to_string()));
        }

        // 构建事件摘要文本
        let events_text = self.format_events_for_summary(&events, contact_name);

        // 调用 LLM 生成总结
        let summary: String = self
            .call_llm_for_summary(&events_text, contact_name)
            .await?;

        // 解析 LLM 响应
        let background = self.parse_summary_response(&summary)?;

        // 截断背景信息以符合大小限制
        let background = self.truncate_background(background);

        // 序列化为 JSON，失败时保留旧值
        match background.to_json_string() {
            Ok(json) => Ok(json),
            Err(e) => {
                if let Some(old_bg) = current_background {
                    Ok(old_bg.to_string())
                } else {
                    Err(ShouxinError::SerializationError(e.to_string()))
                }
            }
        }
    }

    /// 检查是否需要刷新总结
    ///
    /// # 参数
    /// - `background_json`: 当前背景总结 JSON 字符串
    /// - `last_interaction_at`: 最后交互时间
    ///
    /// # 返回
    /// 是否需要刷新总结
    ///
    /// # 说明
    /// 如果距离上次总结超过 3 天且近期有新交互，则需要刷新
    pub fn needs_refresh(
        background_json: Option<&str>,
        last_interaction_at: DateTime<Utc>,
    ) -> bool {
        let now = Utc::now();

        // 检查是否有近期交互
        let has_recent_interaction = {
            let threshold = now - Duration::days(SUMMARY_REFRESH_DAYS);
            last_interaction_at > threshold
        };

        // 如果没有近期交互，不需要刷新
        if !has_recent_interaction {
            return false;
        }

        // 检查总结是否过期
        match background_json {
            Some(bg_str) if !bg_str.is_empty() => {
                if let Ok(bg) = ContactBackground::from_json_str(bg_str) {
                    let time_since_last = now - bg.summarized_at;
                    time_since_last > Duration::days(SUMMARY_REFRESH_DAYS)
                } else {
                    // 解析失败，需要刷新
                    true
                }
            }
            _ => true,
        }
    }

    /// 格式化事件记录为摘要文本
    ///
    /// # 参数
    /// - `events`: 历史事件记录列表
    /// - `contact_name`: 联系人姓名
    ///
    /// # 返回
    /// 格式化的事件摘要文本
    fn format_events_for_summary(
        &self,
        events: &[ContactEventRecord],
        contact_name: &str,
    ) -> String {
        let mut text = format!("联系人 {} 的历史事件记录：\n\n", contact_name);

        for (i, event) in events.iter().enumerate() {
            text.push_str(&format!(
                "{}. [{}] {} (重要性: {}, 情感倾向: {})\n",
                i + 1,
                event.event_time.format("%Y-%m-%d %H:%M"),
                event.event_summary,
                event.importance,
                event.user_sentiment
            ));
        }

        text
    }

    /// 调用 LLM 生成总结
    ///
    /// # 参数
    /// - `events_text`: 事件摘要文本
    /// - `contact_name`: 联系人姓名
    ///
    /// # 返回
    /// LLM 生成的总结文本
    async fn call_llm_for_summary(
        &self,
        events_text: &str,
        contact_name: &str,
    ) -> Result<String, ShouxinError> {
        let system_prompt = Self::background_summary_prompt(contact_name);

        let system_msg = crate::core::Message::system(&system_prompt)?;
        let user_msg = crate::core::Message::user(events_text)?;

        let response = self.llm_client.chat(vec![system_msg, user_msg]).await?;

        Ok(response)
    }

    /// 背景总结系统提示词
    ///
    /// # 参数
    /// - `contact_name`: 联系人姓名
    ///
    /// # 返回
    /// 系统提示词字符串
    fn background_summary_prompt(contact_name: &str) -> String {
        format!(
            r#"你是一个专业的联系人背景分析专家。请根据 {} 的历史事件记录生成结构化的背景总结。

**重要语言约束：所有输出内容（profession、relationships、user_relationship、recent_activities、reputation 等所有字段）必须使用中文，禁止使用英语、西班牙语或其他语言。**

要求：
1. 提取以下信息：
   - profession: 职业信息（如果提到）
   - relationships: 关系网（提到的人物关系）
   - user_relationship: 与用户的关系
   - recent_activities: 最近共同活动
   - reputation: 风评描述（分析事件中体现的联系人行为模式、用户情感倾向）

风评描述示例：
- "曾多次弄丢物品，用户对其可靠性评价较低"
- "工作认真负责，多次按时完成任务，用户信任度高"
- "经常迟到，用户对其时间观念有所不满"

输出格式：JSON 对象
{{
  "profession": "职业描述",
  "relationships": ["关系1", "关系2"],
  "user_relationship": "与用户的关系描述",
  "recent_activities": ["活动1", "活动2"],
  "reputation": "风评描述"
}}

注意：
- 只提取明确提到的信息，不要推断
- 如果某项信息没有提到，使用 null 或空数组
- 风评描述要基于事件记录中的情感倾向和行为模式
- 保持简洁，总结内容不超过 1KB

现在开始分析："#,
            contact_name
        )
    }

    /// 解析总结响应
    ///
    /// # 参数
    /// - `response`: LLM 响应文本
    ///
    /// # 返回
    /// 解析后的背景总结结构
    fn parse_summary_response(&self, response: &str) -> Result<ContactBackground, ShouxinError> {
        let mut json_str = response.trim();

        // 移除可能的代码块标记
        json_str = json_str
            .strip_prefix("```json")
            .or_else(|| json_str.strip_prefix("```"))
            .unwrap_or(json_str);

        json_str = json_str.strip_suffix("```").unwrap_or(json_str);

        let json_str = json_str.trim();

        // 空响应返回默认背景
        if json_str.is_empty() {
            return Ok(ContactBackground::new());
        }

        // 尝试提取 JSON 对象
        if let (Some(start_idx), Some(end_idx)) = (json_str.find('{'), json_str.rfind('}')) {
            if start_idx < end_idx {
                let possible_json = &json_str[start_idx..=end_idx];
                if let Ok(background) = serde_json::from_str::<ContactBackground>(possible_json) {
                    return Ok(background);
                }
            }
        }

        // 解析失败返回默认背景
        Ok(ContactBackground::new())
    }

    /// 截断背景信息以符合大小限制
    ///
    /// # 参数
    /// - `background`: 背景总结结构
    ///
    /// # 返回
    /// 截断后的背景总结结构
    ///
    /// # 说明
    /// 按优先级依次删除：最近活动 -> 关系网 -> 用户关系 -> 职业 -> 风评
    fn truncate_background(&self, mut background: ContactBackground) -> ContactBackground {
        for _ in 0..10 {
            // 检查大小是否符合限制
            if let Ok(json) = background.to_json_string() {
                if json.len() <= MAX_SUMMARY_SIZE {
                    break;
                }
            }

            // 按优先级删除字段
            if !background.recent_activities.is_empty() {
                background.recent_activities.pop();
                continue;
            }

            if !background.relationships.is_empty() {
                background.relationships.pop();
                continue;
            }

            if background.user_relationship.is_some() {
                background.user_relationship = None;
                continue;
            }

            if background.profession.is_some() {
                background.profession = None;
                continue;
            }

            if background.reputation.is_some() {
                background.reputation = None;
                continue;
            }

            break;
        }

        background
    }
}
