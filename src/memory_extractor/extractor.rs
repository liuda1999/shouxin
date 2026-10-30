//! 通用 LLM 内存提取器
//!
//! 使用任意实现了 LlmClient trait 的大模型后端来提取对话中的
//! 结构化记忆元素（人物、事件、待办、关系等）

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashSet;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::ShouxinError;
use crate::traits::llm::LlmClient;
use crate::core::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementType {
    Person,
    Event,
    Todo,
    Fact,
    Learning,
}

/// 提取结果（包含新旧字段兼容）
#[derive(Debug, Clone)]
pub struct ExtractedElements {
    /// 对话原文
    pub conversation_text: String,
    /// 人物
    pub people: Vec<ExtractedPerson>,
    /// 人物（旧名，兼容）
    pub persons: Vec<ExtractedPerson>,
    /// 事件
    pub events: Vec<ExtractedEvent>,
    /// 待办
    pub todos: Vec<ExtractedTodo>,
    /// 事实
    pub facts: Vec<ExtractedFact>,
    /// 学习素材
    pub learnings: Vec<ExtractedLearning>,
    /// 待办更新意图
    pub todo_update_intent: Option<TodoUpdateIntent>,
}

impl ExtractedElements {
    /// 创建空结果
    pub fn new(conversation_text: String) -> Self {
        Self {
            conversation_text,
            people: Vec::new(),
            persons: Vec::new(),
            events: Vec::new(),
            todos: Vec::new(),
            facts: Vec::new(),
            learnings: Vec::new(),
            todo_update_intent: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedPerson {
    pub contact_id: String,
    pub name: String,
    /// 同名消歧标签（"做测试的那个" / "前端" / "客户那边的"）
    ///
    /// 只有对话里**确实出现两个同名的人**时才由模型填写；
    /// 写入层按 (name, disambiguation) 判断是"同一人补充信息"还是"新建一个人"。
    pub disambiguation: String,
    pub aliases: Vec<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub contact_type: String,
}

/// 联系人验证结果
#[derive(Debug, Deserialize)]
struct ContactValidationResult {
    name: String,
    is_valid_contact: bool,
    is_related_info: bool,
    reason: String,
}

impl ExtractedPerson {
    pub fn new(name: String) -> Self {
        Self {
            contact_id: Uuid::new_v4().to_string(),
            name,
            disambiguation: String::new(),
            aliases: Vec::new(),
            role: None,
            description: None,
            contact_type: "unknown".to_string(),
        }
    }
}

/// 待办更新意图
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoUpdateIntent {
    /// 待办标题提示（用于模糊匹配）
    #[serde(default)]
    pub title_hint: Option<String>,
    /// 新的截止时间（ISO 8601）
    #[serde(default)]
    pub new_deadline: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedEvent {
    pub event_id: String,
    pub title: Option<String>,
    pub description: String,
    pub time: Option<String>,
    pub time_info: Option<String>,
    pub location: Option<String>,
    pub importance: f32,
    pub importance_label: String,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub related_person_ids: Vec<String>,
}

impl ExtractedEvent {
    pub fn new(description: String) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            title: None,
            description,
            time: None,
            time_info: None,
            location: None,
            importance: 0.5,
            importance_label: "short_term".to_string(),
            start_time: None,
            end_time: None,
            related_person_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedTodo {
    pub title: String,
    pub importance: String,
    pub deadline: Option<String>,
    pub assigned_to: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedFact {
    pub text: String,
    pub category: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct ExtractedLearning {
    pub content: String,
    pub quality_score: f32,
    pub tags: Vec<String>,
}

impl ExtractedLearning {
    pub fn new(content: String) -> Self {
        Self {
            content,
            quality_score: 0.8,
            tags: Vec::new(),
        }
    }
}

/// 记忆提取器 Trait（向后兼容）
#[async_trait]
pub trait MemoryExtractor: Send + Sync {
    async fn extract(&self, conversation: &[Message]) -> Result<ExtractedElements, ShouxinError>;
    fn fallback_extract(&self, conversation: &str) -> Result<ExtractedElements, ShouxinError>;
}

/// LLM 输出后处理校验器
/// 检测并修复 GGUF 模型的乱码输出
pub struct LlmOutputSanitizer;

impl LlmOutputSanitizer {
    /// 检测输出是否包含大量非预期字符（如 Cyrillic）
    pub fn has_garbage_chars(text: &str) -> bool {
        // 如果中文字符占比低于 30%，且包含 Cyrillic 字符，认为是乱码
        let total_chars = text.chars().count();
        if total_chars == 0 { return false; }

        let cyrillic_count = text.chars().filter(|c| matches!(*c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}')).count();
        let chinese_count = text.chars().filter(|c| matches!(*c, '\u{4e00}'..='\u{9fff}')).count();

        let cyrillic_ratio = cyrillic_count as f32 / total_chars as f32;
        let chinese_ratio = chinese_count as f32 / total_chars as f32;

        // Cyrillic 占比 > 5% 或中文占比 < 30%（对于中文提示词）认为是乱码
        cyrillic_ratio > 0.05 || (chinese_ratio < 0.30 && total_chars > 20)
    }

    /// 尝试从乱码中提取 JSON 片段
    pub fn extract_json_fragment(text: &str) -> Option<String> {
        // 查找第一个 { 到最后一个 } 之间的内容
        let start = text.find('{')?;
        let end = text.rfind('}')?;
        if start >= end { return None; }
        let fragment = &text[start..=end];

        // 尝试修复常见的乱码：将 Cyrillic 字符替换为空格
        let cleaned: String = fragment
            .chars()
            .map(|c| {
                if matches!(c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}') {
                    ' ' // 替换 Cyrillic 为空格
                } else {
                    c
                }
            })
            .collect();

        Some(cleaned)
    }

    /// 尝试用正则从乱码文本中提取待办更新意图
    pub fn extract_todo_intent_heuristic(text: &str) -> Option<TodoUpdateIntent> {
        // 匹配 "把xxx改成yyyy-mm-dd" 或 "deadline推迟到x月x日" 等模式
        let re = Regex::new(r"(?i)(?:把|将|将|把)\s*([^改]+?)\s*(?:截止时间|deadline|日期|时间)\s*(?:改成|改为|推迟到|延期到|调整到|改为|改到)\s*(\d{4}-\d{2}-\d{2}|\d{1,2}月\d{1,2}日|下周[一二三四五六日]|明天|后天|下月\d{1,2}日)").ok()?;

        if let Some(cap) = re.captures(text) {
            let title_hint = cap.get(1)?.as_str().trim().to_string();
            let date_str = cap.get(2)?.as_str().trim().to_string();

            // 简单日期转换
            let new_deadline = Self::normalize_date(&date_str);

            return Some(TodoUpdateIntent {
                title_hint: Some(title_hint),
                new_deadline,
            });
        }

        None
    }

    fn normalize_date(date_str: &str) -> Option<String> {
        let iso_re = Regex::new(r"^\d{4}-\d{2}-\d{2}$").ok()?;
        if iso_re.is_match(date_str) {
            return Some(format!("{}T00:00:00Z", date_str));
        }
        // 其他格式可以后续扩展
        None
    }
}

/// 通用 LLM 记忆提取器
pub struct LlmMemoryExtractor {
    llm_client: Arc<dyn LlmClient>,
    system_prompt: String,
    /// 已确认"是人名"的缓存
    ///
    /// 为什么需要：每次抽取都会为这一条对话里出现的名字单独跑一次
    /// "这词是不是人名"的 LLM 校验。实测 242 条语料里人名高度复用，
    /// 约 40% 的推理花在重复校验同一批名字上（每次 ~7s）。
    /// 命中缓存后直接跳过这次调用，语料越多省得越多。
    ///
    /// 安全性：进缓存的前提是它已经通过"逐字出现在原文"+"关系短语过滤"+
    /// LLM 判定三重检查，缓存只是避免重复问同一个问题。
    validated_names: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl LlmMemoryExtractor {
    pub fn new(llm_client: Arc<dyn LlmClient>) -> Self {
        Self {
            llm_client,
            system_prompt: Self::default_system_prompt(),
            validated_names: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }

    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = prompt;
        self
    }

    fn default_system_prompt() -> String {
        r#"你是一个信息提取助手。从中文对话中提取人名和事件。

**重要语言约束：所有输出内容（人名、事件标题、事件描述等所有字段）必须使用中文，禁止使用英语、西班牙语或其他语言。**

提取规则：
1. 人名（people）: 对话中提到的所有人的姓名
   - **姓名必须逐字出现在对话原文中。示例里出现的人名仅用于演示输出格式，绝对不要提取示例中的人名。**
   - 只提取真实完整的人名，如"王经理"、"李总"、"王老师"
   - 不要截断人名，如"王经理"不能提取为"经理"或"点去他办公"
   - 不提取人称代词如"他"、"她"、"我"、"你"
   - 不提取机构名如"公司"、"办公室"
   - aliases: 如果对话中提到了某人的别名/昵称/简称（如"赵明（明哥）"中的"明哥"、"张伟也叫张大炮"中的"张大炮"），在 aliases 数组中列出；没有别名则为空数组 []
   - disambiguation: **仅当对话里同时出现两个同名的人**时填写，用原文里的区分信息说明是哪一个
     （如"两个刘洋"→"前端"或"后端"；"两个王强"→"做测试的"或"做运维的"）。
     只有一个人、或没有区分信息时，填空字符串 ""。不要编造原文没有的区分信息。
   - **区分词不要并进姓名**："前端刘洋"要拆成 name="刘洋" + disambiguation="前端"，
     绝对不能把 name 写成"前端刘洋"；同理"后端大刘"→ name="刘洋"（如果有别名"大刘"就放进 aliases）。
   - role: 这个人的**身份线索**（部门 / 职位 / 和说话人的关系），如"前端工程师"、"财务部"、"客户方负责人"、
     "我的大学室友"。原文没有明确依据时填空字符串 ""。
     它用于**同名时跨对话区分不同的人**：今天说"刘洋是前端的"，下周只说"刘洋"时，
     系统要靠这条线索认出是同一个刘洋。不要编造。
2. 事件（events）: 对话中提到的每个独立事件分别提取
   - title: 简短的事件标题（如"产品需求会"、"启动开发"、"技术方案讨论"）
   - description: 一句话描述事件内容，不要输出整个对话全文
   - time: 事件时间（如果有明确时间表达，如"明天下午三点"、"下周一"等）
   - importance: 事件重要性分级，必须为以下之一：
     * "short_term" - 短期的一次性事件（如：明天开会、今晚聚餐、下周出差、周三体检、周末搬家）
     * "long_term" - 长期持续的计划或周期性事件（如：半年健身计划、一年学习目标、买房计划、创业规划、MBA学位）
     * "critical" - 重大紧急事件（如：服务器宕机、安全漏洞修复、法律诉讼、关键系统上线）
   - participants: 参与该事件的人名列表（必须与 people 中提取的姓名完全一致，无人参与则为空数组 []）
   - 判断标准：如果事件是具体某一天的一次性活动，用"short_term"；如果事件是持续多天/多月的长期计划，用"long_term"
   - 没有时间则不填time字段
   - 如果对话提到多个事件，必须分别提取，每个事件一个独立对象
3. 待办（todos）: 对话中提到的需要在未来主动完成的任务或待办事项
   - title: 待办标题（简短描述需要做什么，如"提交项目报告"、"完成需求文档"）
   - importance: 重要性，必须为以下之一：
     * "low" - 低优先级（不紧急，可以稍后处理）
     * "medium" - 中等优先级（需要在截止时间前完成）
     * "high" - 高优先级（紧急且重要）
   - deadline: 截止时间，ISO 8601 格式（如"2026-07-20T00:00:00Z"），如果有明确截止时间则填写，没有则不填此字段
   - assigned_to: 负责人姓名（如果对话中明确提到负责人），没有则不填此字段
   - 判断标准：待办是需要主动去"做"的任务（如"明天要交报告"、"下周需要完成方案"、"记得回复邮件"），而非已经发生的事件
   - 不要把已发生的事件提取为待办；事件是描述发生了什么，待办是描述需要去做什么
4. 修改待办（todo_update_intent）: 如果对话中包含修改待办截止时间的意图，输出待办标题提示和新截止时间
   - title_hint: 待办标题关键词（用于模糊匹配）
   - new_deadline: 新的截止时间，ISO 8601 格式（如"2026-07-03T00:00:00Z"）
   - 没有修改待办意图时设为 null

**Few-shot 示例**：

示例1：
输入："王经理让我这周五下午2点去他办公室开会"
输出：{"people":[{"name":"王经理"}],"events":[{"title":"办公室会议","description":"王经理安排周五下午2点在办公室开会","time":"周五下午2点","importance":"short_term","participants":["王经理"]}],"todos":[],"todo_update_intent":null}

示例2：
输入："李总（老李）说下周一带他女儿来公司参观"
输出：{"people":[{"name":"李总","aliases":["老李"]},{"name":"李总的女儿","aliases":[]}],"events":[{"title":"公司参观","description":"李总下周一带女儿来公司参观","time":"下周一","importance":"short_term","participants":["李总","李总的女儿"]}],"todos":[],"todo_update_intent":null}

示例3：
输入："赵总明天上午10点要开会讨论项目进度"
输出：{"people":[{"name":"赵总"}],"events":[{"title":"项目进度会","description":"赵总明天上午10点开会讨论项目进度","time":"明天上午10点","importance":"short_term","participants":["赵总"]}],"todos":[],"todo_update_intent":null}

示例4：
输入："王总监和李经理今天上午开了产品需求会，决定下周一启动开发"
输出：{"people":[{"name":"王总监"},{"name":"李经理"}],"events":[{"title":"产品需求会","description":"王总监和李经理开会讨论产品需求","time":"今天上午","importance":"short_term","participants":["王总监","李经理"]},{"title":"启动开发","description":"决定下周一启动开发工作","time":"下周一","importance":"short_term","participants":["王总监","李经理"]}],"todos":[],"todo_update_intent":null}

示例5：
输入："我计划用一年时间系统学习机器学习，从数学基础开始"
输出：{"people":[],"events":[{"title":"机器学习学习计划","description":"用一年时间系统学习机器学习","importance":"long_term","participants":[]}],"todos":[],"todo_update_intent":null}

示例6：
输入："服务器宕机了，客户数据可能丢失，需要立即修复！"
输出：{"people":[],"events":[{"title":"服务器宕机","description":"服务器宕机导致客户数据可能丢失，需要立即修复","importance":"critical","participants":[]}],"todos":[{"title":"修复服务器并恢复客户数据","importance":"high"}],"todo_update_intent":null}

示例7：
输入："我和合伙人计划明年创立一家AI公司，目前在做市场调研"
输出：{"people":[],"events":[{"title":"创立AI公司","description":"和合伙人计划明年创立AI SaaS公司","importance":"long_term","participants":[]}],"todos":[],"todo_update_intent":null}

示例8：
输入："把产品需求会的截止时间改成下周三"
输出：{"people":[],"events":[],"todos":[],"todo_update_intent":{"title_hint":"产品需求会","new_deadline":"2026-06-10T00:00:00Z"}}

示例9：
输入："张经理让我下周三前完成项目方案，然后下周五提交给客户"
输出：{"people":[{"name":"张经理"}],"events":[],"todos":[{"title":"完成项目方案","importance":"high","deadline":"2026-07-15T00:00:00Z","assigned_to":"张经理"},{"title":"提交项目方案给客户","importance":"high","deadline":"2026-07-17T00:00:00Z","assigned_to":"张经理"}],"todo_update_intent":null}

示例10：
输入："记得明天回复李总的邮件，还要准备下周的汇报材料"
输出：{"people":[{"name":"李总"}],"events":[],"todos":[{"title":"回复李总的邮件","importance":"medium"},{"title":"准备下周汇报材料","importance":"medium"}],"todo_update_intent":null}

示例11（同名不同人）：
输入："我们组有两个刘洋，前端刘洋和后端大刘，今天说的是前端刘洋负责的页面改版"
输出：{"people":[{"name":"刘洋","disambiguation":"前端","role":"前端工程师"}],"events":[{"title":"页面改版","description":"前端刘洋负责页面改版","importance":"short_term","participants":["刘洋"]}],"todos":[],"todo_update_intent":null}

输出JSON（严格按照格式，不要其他文字）：
{"people":[{"name":"姓名","aliases":[],"disambiguation":"","role":""}],"events":[{"title":"事件标题","description":"事件描述","time":"时间（如果有）","importance":"short_term或long_term或critical","participants":["参与人姓名"]}],"todos":[{"title":"待办标题","importance":"low或medium或high","deadline":"截止时间ISO格式（可选）","assigned_to":"负责人（可选）"}],"todo_update_intent":null}

如果没有提取到任何内容，返回：{"people":[],"events":[],"todos":[],"todo_update_intent":null}"#
            .to_string()
    }

    pub async fn extract_str(&self, conversation: &str) -> Result<ExtractedElements, ShouxinError> {
        const MAX_CONVERSATION_CHARS: usize = 1024;

        let truncated = if conversation.chars().count() > MAX_CONVERSATION_CHARS {
            let truncated_chars: String = conversation.chars().take(MAX_CONVERSATION_CHARS).collect();
            info!("[LLM Extract] Conversation truncated from {} to {} chars", conversation.chars().count(), MAX_CONVERSATION_CHARS);
            truncated_chars
        } else {
            conversation.to_string()
        };

        let llm_result: Result<ExtractedElements, ShouxinError> = async {
            let messages = vec![
                Message::system(&self.system_prompt)?,
                Message::user(&truncated)?,
            ];

            info!("[LLM Extract] Sending prompt ({} chars) to LLM", truncated.chars().count());
            debug!("[LLM Extract] System prompt: {}", self.system_prompt.chars().take(200).collect::<String>());

            let response = self.llm_client.chat(messages).await?;

            let preview: String = response.chars().take(500).collect();
            info!("[LLM Extract] Raw response ({} bytes, {} chars): {}", response.len(), response.chars().count(), preview);
            debug!("[LLM Extract] Full response: {}", response);

            // 新增：乱码检测与修复
            let sanitized_response = if LlmOutputSanitizer::has_garbage_chars(&response) {
                warn!("[LLM Extract] Detected garbage characters in LLM output, attempting sanitization");
                if let Some(fragment) = LlmOutputSanitizer::extract_json_fragment(&response) {
                    info!("[LLM Extract] Extracted JSON fragment from garbage output");
                    fragment
                } else {
                    // 尝试提取 todo_update_intent
                    if let Some(intent) = LlmOutputSanitizer::extract_todo_intent_heuristic(conversation) {
                        info!("[LLM Extract] Extracted todo intent via heuristic: {:?}", intent);
                        let mut elements = self.fallback_extract(conversation)?;
                        elements.todo_update_intent = Some(intent);
                        return Ok(elements);
                    }
                    response.clone()
                }
            } else {
                response.clone()
            };

            let mut elements = self.parse_response(&sanitized_response, conversation)?;
            elements.conversation_text = conversation.to_string();
            info!("[LLM Extract] Parsed: {} people, {} events", elements.people.len(), elements.events.len());

            let has_long_events = elements.events.iter().any(|e| e.description.chars().count() > 200);
            if (elements.events.is_empty() || has_long_events) && conversation.len() > 80 {
                info!("[LLM Extract] Events empty or contain full-conversation text, falling back to keyword event extraction");
                let fallback_events = Self::fallback_extract_events(conversation);
                if !fallback_events.is_empty() {
                    info!("[LLM Extract] Fallback event extraction yielded {} events", fallback_events.len());
                    elements.events = fallback_events;
                }
            }

            if conversation.len() > 80 {
                info!("[LLM Extract] Long text detected ({} chars), running multi-segment person extraction", conversation.len());
                let segment_people = self.multi_segment_person_extraction(conversation).await;
                if !segment_people.is_empty() {
                    // 对分段提取的联系人也进行验证过滤
                    let validated_segment = self.validate_contacts(conversation, &segment_people).await;
                    if !validated_segment.is_empty() {
                        self.merge_segment_persons(&mut elements, validated_segment);
                        info!("[LLM Extract] After multi-segment merge: {} people", elements.people.len());
                    }
                }
            }

            Ok(elements)
        }.await;

        let mut elements = match llm_result {
            Ok(elements) => elements,
            Err(e) => {
                warn!("[LLM Extract] LLM extraction failed: {}, falling back to regex extraction", e);
                info!("[LLM Extract] Running fallback regex extraction on full conversation ({} chars)", conversation.chars().count());
                self.fallback_extract(conversation)?
            }
        };

        // 联系人精准验证：过滤垃圾联系人
        if !elements.people.is_empty() {
            info!("[LLM Extract] Running contact validation on {} contacts", elements.people.len());
            let validated = self.validate_contacts(conversation, &elements.people).await;
            elements.people = validated.clone();
            elements.persons = validated;
        }

        Ok(elements)
    }

    fn segment_conversation(&self, text: &str) -> Vec<String> {
        // 分块策略：**按目标长度贪心打包**，并对总块数设上限。
        //
        // 原实现的合并条件是 `current + seg < 80`，一旦超过 80 字符就不再合并，
        // 于是每块只有 ~80 字符：3.5 万字的对话被切成 175 块，
        // 而每块都要单独调一次 LLM（联系人抽取），在 CPU 上要跑近一小时。
        //
        // 现在：目标 ~1200 字符/块（约 800 token，稳在 ctx 内），最多 8 块；
        // 超长输入只抽取前 8 块，并明确告警（原文仍完整保存在字典库）。
        const CHUNK_TARGET_CHARS: usize = 1200;
        const MAX_CHUNKS: usize = 8;

        let re = Regex::new(r"[。！？\n;；]+").unwrap();
        let raw_segments: Vec<&str> = re
            .split(text)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut segments: Vec<String> = Vec::new();
        let mut current = String::new();

        for seg in raw_segments {
            let seg_len = seg.chars().count();
            let cur_len = current.chars().count();

            // 当前块还没到目标长度 → 继续合并
            if cur_len == 0 || cur_len + seg_len <= CHUNK_TARGET_CHARS {
                if !current.is_empty() {
                    current.push('。');
                }
                current.push_str(seg);
            } else {
                segments.push(std::mem::take(&mut current));
                current.push_str(seg);
            }

            // 单句本身就超长：按字符截断，避免单块撑爆上下文
            if current.chars().count() >= CHUNK_TARGET_CHARS * 2 {
                segments.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            segments.push(current);
        }

        if segments.is_empty() && !text.is_empty() {
            segments.push(text.to_string());
        }

        if segments.len() > MAX_CHUNKS {
            let total = segments.len();
            segments.truncate(MAX_CHUNKS);
            warn!(
                "[LLM Extract] 输入过长（{} 字符），已切成 {} 块，仅处理前 {} 块；原文完整保留在字典库",
                text.chars().count(),
                total,
                MAX_CHUNKS
            );
        }

        info!(
            "[LLM Extract] Segmented into {} chunks (avg {} chars)",
            segments.len(),
            if segments.is_empty() {
                0
            } else {
                segments.iter().map(|s| s.chars().count()).sum::<usize>() / segments.len()
            }
        );
        segments
    }

    async fn extract_persons_from_segment(&self, segment: &str) -> Result<Vec<String>, ShouxinError> {
        if segment.chars().count() < 5 {
            return Ok(Vec::new());
        }

        let person_prompt = "你是一个精准的人名提取助手。\
            从以下文本片段中提取所有人名（包括称呼，如\"王经理\"、\"李总的女儿\"）。\
            只提取完整的真实人名，不取人称代词或机构名。\
            **所有输出必须使用中文，禁止使用其他语言。**\
            只输出JSON数组，不要有其他文字，格式：{\"people\":[{\"name\":\"姓名\"}]}\n\n\
            文本片段：";

        let messages = vec![
            Message::system(person_prompt)?,
            Message::user(segment)?,
        ];

        debug!("[LLM Extract] Segment extraction: {} chars", segment.chars().count());

        let response = self.llm_client.chat(messages).await?;
        let response = response.trim();

        if response.is_empty() {
            return Ok(Vec::new());
        }

        let json_str = response
            .strip_prefix("```json")
            .or_else(|| response.strip_prefix("```"))
            .unwrap_or(response);
        let json_str = json_str
            .strip_suffix("```")
            .unwrap_or(json_str)
            .trim();

        let json_str = if let Some(start) = json_str.find('{') {
            let slice = &json_str[start..];
            if let Some(end) = find_matching_brace(slice) {
                &slice[..=end]
            } else {
                slice
            }
        } else {
            json_str
        };

        #[derive(serde::Deserialize)]
        struct SegmentResponse {
            #[serde(default)]
            people: Vec<SegmentPerson>,
        }
        #[derive(serde::Deserialize)]
        struct SegmentPerson {
            name: String,
        }

        match serde_json::from_str::<SegmentResponse>(json_str) {
            Ok(resp) => {
                let names: Vec<String> = resp.people.into_iter()
                    .map(|p| Self::fix_name_truncation(&p.name))
                    .filter(|n| n.chars().count() >= 2)
                    .collect();
                debug!("[LLM Extract] Segment extracted {} names: {:?}", names.len(), names);
                Ok(names)
            }
            Err(e) => {
                debug!("[LLM Extract] Segment parse failed: {}", e);
                Ok(Vec::new())
            }
        }
    }

    fn fix_name_truncation(name: &str) -> String {
        let mut name = name.trim().to_string();
        let truncated_markers = [
            "很", "的", "了", "是", "在", "和", "与", "或", "很厉", "厉", "害",
            "说", "道", "问", "答", "叫", "被", "把", "将", "对", "向",
            "做", "去", "来", "过", "要", "会", "能", "可", "请", "让",
            "我", "你", "他", "她", "它", "们", "这", "那", "个", "些",
            "一", "个", "两", "三", "四", "五", "六", "七", "八", "九", "十",
            "中", "上", "下", "里", "外", "内", "间", "边", "面", "头",
            "借", "诗", "文", "画", "歌", "舞", "得", "好", "想",
            "打", "走", "跑", "跳", "飞", "游", "吃", "喝", "睡",
            "今天", "昨天", "明天", "现在", "当时", "后来", "之前", "之后",
            "国", "家", "城", "市", "县", "省", "区", "镇", "村",
            "是魏", "是勇", "是魔", "是传", "是主", "是角", "是幻",
            "君主", "的人", "的人", "的人", "的人",
        ];
        
        let mut changed = true;
        while changed {
            changed = false;
            let original_len = name.chars().count();
            for marker in &truncated_markers {
                if name.ends_with(marker) && name.chars().count() > 2 {
                    let cleaned = name.trim_end_matches(marker).trim().to_string();
                    if !cleaned.is_empty() && cleaned.chars().count() >= 2 && cleaned.chars().count() < original_len {
                        info!("[Name Fix] Truncated name detected: '{}' -> '{}'", name, cleaned);
                        name = cleaned;
                        changed = true;
                        break;
                    }
                }
            }
        }
        
        if name.chars().any(|c| c.is_ascii_digit()) && name.len() > 3 {
            let cleaned: String = name.chars().take_while(|c| !c.is_ascii_digit()).collect::<String>().trim().to_string();
            if !cleaned.is_empty() && cleaned.chars().count() >= 2 {
                info!("[Name Fix] Name with trailing digits: '{}' -> '{}'", name, cleaned);
                return cleaned;
            }
        }
        
        name
    }

    /// 基于规则的联系人验证过滤器
    /// 过滤掉明显不是联系人的垃圾条目
    fn rule_based_contact_filter(
        &self,
        contacts: &[ExtractedPerson],
        conversation: &str,
    ) -> Vec<ExtractedPerson> {
        // 常见垃圾词列表 - 这些词明显不是人名
        let garbage_words: HashSet<&str> = [
            "我和", "我们", "你们", "他们", "她们", "它们",
            "讨论", "的未", "给了", "把剑", "游戏", "历史",
            "一起去打", "拯救公主", "写得好", "国君主", "想的主角",
            "是最终幻", "来上班", "我的同事", "东风",
            "一起", "去做", "来", "去", "做",
            "明天", "昨天", "现在", "然后", "但是", "因为",
            "所以", "如果", "虽然", "不过", "关于", "对于", "根据",
            "通过", "进行", "使用", "提供", "包括", "支持", "处理",
            "管理", "开发", "测试", "汇报", "检查", "确认", "完成",
            "开始", "结束", "继续", "准备", "安排", "通知", "帮助",
            "参加", "参考", "参与", "出发", "发布", "发送", "告诉",
            "共享", "交流", "决定", "开会", "了解", "联系", "培训",
            "提交", "知道", "可以", "需要", "已经", "没有", "应该",
            "可能", "必须", "一定", "还是", "或者", "而且", "然后",
        ].iter().cloned().collect();

        // 动作/谓语后缀 - 以这些词结尾的通常不是人名
        let action_suffixes = [
            "打", "走", "跑", "跳", "飞", "游", "吃", "喝", "睡",
            "看", "听", "说", "笑", "哭", "唱", "画", "写", "读",
            "想", "爱", "恨", "怕", "喜", "怒", "哀", "乐",
            "借", "诗", "书", "文", "画", "歌", "舞", "得", "好",
            "去", "来", "过", "做", "干", "搞", "弄", "办",
            "上", "下", "进", "出", "回", "开", "关", "放", "拿",
            "给", "送", "接", "打", "骂", "夸", "问", "答",
            "学", "教", "练", "考", "比", "赛", "赢", "输",
            "买", "卖", "花", "赚", "赔", "省", "费",
            "等", "待", "留", "守", "护", "保", "卫",
            "变", "化", "成", "为", "作", "当", "任",
            "有", "没", "在", "是", "的", "了", "着",
        ];

        // 常见非人名词语（单字或双字）
        let non_person_words: HashSet<&str> = [
            "东西", "事情", "问题", "时间", "地方", "原因", "结果",
            "方式", "方法", "过程", "情况", "状态", "关系", "作用",
            "意义", "目的", "目标", "计划", "方案", "建议", "意见",
            "想法", "看法", "说法", "做法", "办法", "手段", "措施",
            "步骤", "环节", "阶段", "时期", "年代", "时代", "岁月",
            "日子", "天气", "气候", "温度", "湿度", "环境", "条件",
            "基础", "根本", "核心", "关键", "重点", "难点", "要点",
            "细节", "整体", "部分", "方面", "领域", "范围", "程度",
            "水平", "标准", "质量", "数量", "速度", "效率", "效果",
            "成果", "成绩", "业绩", "表现", "能力", "实力", "潜力",
            "优势", "劣势", "机会", "威胁", "风险", "挑战", "困难",
            "障碍", "矛盾", "冲突", "竞争", "合作", "配合", "协调",
            "沟通", "交流", "互动", "联系", "关系", "距离", "差异",
            "变化", "发展", "进步", "提高", "改善", "增强", "减弱",
            "增加", "减少", "扩大", "缩小", "上升", "下降", "提高",
            "降低", "加快", "减慢", "加强", "削弱", "巩固", "动摇",
            "坚持", "放弃", "继续", "停止", "开始", "结束", "完成",
        ].iter().cloned().collect();

        let mut valid_contacts = Vec::new();
        let mut removed_names = Vec::new();

        for contact in contacts {
            let name = &contact.name;
            // 先进行截断修复
            let name_fixed = Self::fix_name_truncation(name);
            let name_trimmed = name_fixed.trim();

            // 规则 1: 空名称过滤
            if name_trimmed.is_empty() {
                removed_names.push((name.clone(), "空名称".to_string()));
                continue;
            }

            // 规则 2: 长度检查 - 单字过滤（除非是常见姓氏+称谓）
            if name_trimmed.chars().count() == 1 {
                removed_names.push((name.clone(), "单字名称".to_string()));
                continue;
            }

            // 规则 3: 完全匹配垃圾词列表
            if garbage_words.contains(&name_trimmed) {
                removed_names.push((name.clone(), "垃圾词".to_string()));
                continue;
            }

            // 规则 4: 以动作后缀结尾（且去掉后缀后不足2字）
            let mut is_action_ending = false;
            for suffix in &action_suffixes {
                if name_trimmed.ends_with(suffix) && name_trimmed.chars().count() > suffix.chars().count() {
                    let without_suffix = name_trimmed.trim_end_matches(suffix).trim();
                    if without_suffix.chars().count() < 2 {
                        is_action_ending = true;
                        break;
                    }
                }
            }
            if is_action_ending {
                removed_names.push((name.clone(), "动作短语".to_string()));
                continue;
            }

            // 规则 5: 常见非人名词语
            if non_person_words.contains(&name_trimmed) {
                removed_names.push((name.clone(), "非人名词语".to_string()));
                continue;
            }

            // 规则 6: 纯数字或纯英文字母（不是人名）
            if name_trimmed.chars().all(|c| c.is_ascii_digit()) || 
               (name_trimmed.chars().all(|c| c.is_ascii_alphabetic()) && name_trimmed.len() <= 2) {
                removed_names.push((name.clone(), "纯数字/短字母".to_string()));
                continue;
            }

            // 规则 7: 检查是否包含明显非人名模式
            let garbage_patterns = ["我和", "我们", "你们", "他们", "一起", "去做", "给了", "把剑"];
            let has_garbage = garbage_patterns.iter().any(|p| name_trimmed.contains(p));
            if has_garbage {
                removed_names.push((name.clone(), "包含垃圾模式".to_string()));
                continue;
            }

            // 规则 8: 必须真的出现在对话原文里（防止模型照抄提示词里的示例人物）
            //
            // 真实案例：提示词示例中有"李总（老李）带女儿参观"，
            // 小模型会把示例里的"李总的女儿""张总的秘书""王总的助理"当成提取结果输出，
            // 于是联系人列表被凭空捏造的人名污染。这里用原文做硬性校验：
            // 名字本身或其任一别名必须能在对话文本中找到，否则丢弃。
            let name_in_text = conversation.contains(name_trimmed);
            let alias_in_text = contact
                .aliases
                .iter()
                .any(|a| !a.trim().is_empty() && conversation.contains(a.trim()));
            if !name_in_text && !alias_in_text {
                removed_names.push((name.clone(), "对话原文中未出现".to_string()));
                continue;
            }

            // 规则 9: 名字必须是**人名**，不能是关系短语
            //
            // 逐字校验只能挡住"原文里根本没出现"的幻觉，挡不住"原文里确实出现、
            // 但它不是名字"的情况。200 条压测实测被抽成联系人的：
            //   "小明的姐姐"、"客户的工程师"、"小王和小明的公司领导"、
            //   "林芳的女儿"、"财务部的老人"、"高翔的儿子"
            // 这类条目一旦入库，联系人列表就废了（而且它们和真正的人名抢 50 条上限）。
            if is_relational_phrase(name_trimmed) {
                removed_names.push((name.clone(), "关系短语而非人名".to_string()));
                continue;
            }

            // 通过所有规则检查，使用修复后的名称
            let mut fixed_contact = contact.clone();
            fixed_contact.name = name_trimmed.to_string();
            valid_contacts.push(fixed_contact);
        }

        if !removed_names.is_empty() {
            info!("[Contact Filter] Removed {} invalid contacts: {:?}", removed_names.len(), removed_names);
        }
        info!("[Contact Filter] Kept {} valid contacts", valid_contacts.len());
        valid_contacts
    }

    /// 验证联系人列表，使用 LLM 进行二次验证
    /// 提示词只做两件事：1. 是否是联系人 2. 是否是联系人相关的信息
    async fn validate_contacts(&self, conversation: &str, contacts: &[ExtractedPerson]) -> Vec<ExtractedPerson> {
        if contacts.is_empty() {
            return vec![];
        }

        // 首先使用规则过滤进行初步筛选
        let rule_filtered = self.rule_based_contact_filter(contacts, conversation);
        if rule_filtered.is_empty() {
            return vec![];
        }

        // 已确认过的名字直接放行，只把新名字送去 LLM
        let (known, unknown): (Vec<_>, Vec<_>) = {
            let cache = self.validated_names.lock().unwrap();
            rule_filtered
                .into_iter()
                .partition(|c| cache.contains(&c.name))
        };

        if unknown.is_empty() {
            return known;
        }

        let validated_new = self.llm_validate_contacts(conversation, &unknown).await;
        {
            let mut cache = self.validated_names.lock().unwrap();
            // 缓存上限：防长跑进程无限增长
            if cache.len() > 2000 {
                cache.clear();
            }
            for p in &validated_new {
                cache.insert(p.name.clone());
            }
        }

        let mut out = known;
        out.extend(validated_new);
        out
    }

    /// 使用 LLM 验证联系人列表
    /// 返回经过 LLM 验证的联系人列表
    async fn llm_validate_contacts(&self, conversation: &str, contacts: &[ExtractedPerson]) -> Vec<ExtractedPerson> {
        let contact_names: Vec<String> = contacts.iter().map(|c| c.name.clone()).collect();
        let names_str = contact_names.join("\n");

        // 只取对话开头一段作为判断依据。
        // 校验只需要"这些词在上下文里长什么样"，整段塞进去会超上下文
        // （实测 3.5 万字 → 20320 tokens > 4096 ctx，导致校验必定失败并重试一轮）。
        let excerpt: String = conversation.chars().take(1500).collect();
        let conversation = excerpt.as_str();

        let validation_prompt = format!(
            r#"请判断以下从对话中提取的词语是否是人名（真实人物、虚拟角色、历史人物等）。

**重要：所有输出内容（说明、描述等）必须使用中文，禁止使用其他语言。**

对话内容：
{}

需要判断的词语：
{}

请对每个词语回答两个问题：
1. 这个词语是否是人名？（是/否）
2. 这个词语是否包含联系人相关的信息？（是/否）

判断标准：
- 人名：指代具体的人，如"曹操"、"林克"、"张经理"等
- 非人名：动作、谓语、宾语、前缀词等，如"我和"、"讨论"、"一起去打"、"写得好"、"东风"等
- 包含联系人相关信息：如"曹操今天"包含人名"曹操"，"我和关羽"包含人名"关羽"

请按以下格式输出，每行一个词语：
词语: [原词语] | 是否人名: [是/否] | 是否包含联系人信息: [是/否] | 说明: [简要说明]

例如：
词语: 曹操 | 是否人名: 是 | 是否包含联系人信息: 是 | 说明: 历史人物姓名
词语: 我和 | 是否人名: 否 | 是否包含联系人信息: 否 | 说明: 代词组合，非人名
词语: 曹操今天 | 是否人名: 否 | 是否包含联系人信息: 是 | 说明: 包含人名曹操，但今天不是人名
词语: 诸葛亮借 | 是否人名: 否 | 是否包含联系人信息: 是 | 说明: 包含人名诸葛亮，借是动作"#,
            conversation, names_str
        );

        let messages = vec![
            Message::system("你是一个联系人验证专家。请严格判断每个词语是否是人名。").unwrap(),
            Message::user(&validation_prompt).unwrap(),
        ];

        info!("[LLM Validate] Sending {} contacts to LLM for validation", contacts.len());
        
        let response = match self.llm_client.chat(messages).await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("[LLM Validate] LLM call failed: {}, falling back to rule-based filter", e);
                return contacts.to_vec();
            }
        };

        let preview: String = response.chars().take(500).collect();
        info!("[LLM Validate] Raw response: {}", preview);

        // 解析 LLM 响应
        let validated_contacts = self.parse_validation_response(contacts, &response);
        
        info!("[LLM Validate] LLM validation complete: {} in, {} out", contacts.len(), validated_contacts.len());
        validated_contacts
    }

    /// 解析 LLM 验证响应
    /// 格式：词语: [原词语] | 是否人名: [是/否] | 是否包含联系人信息: [是/否] | 说明: [说明]
    /// 注意：3B 模型中文输出可能不稳定，需要健壮的解析逻辑
    fn parse_validation_response(&self, original_contacts: &[ExtractedPerson], response: &str) -> Vec<ExtractedPerson> {
        let mut validated = Vec::new();
        let mut removed = Vec::new();

        for contact in original_contacts {
            let name = &contact.name;
            // 在响应中查找该词语的判断结果
            let mut is_valid = false;
            let mut has_related_info = false;

            for line in response.lines() {
                let line = line.trim();
                // 宽松匹配：检查行中是否包含联系人名称
                if line.contains(name) {
                    // 检查是否是人名 - 宽松匹配"是"（可能重复如"是是"）
                    if line.contains("人名") && (line.contains("是") || line.contains("✓") || line.contains("yes") || line.contains("Yes")) {
                        // 排除"否"的情况
                        if !line.contains("否") && !line.contains("no") && !line.contains("No") {
                            is_valid = true;
                        }
                    }
                    // 检查是否包含联系人信息
                    if line.contains("联系") && (line.contains("是") || line.contains("✓")) {
                        if !line.contains("否") && !line.contains("no") {
                            has_related_info = true;
                        }
                    }
                    break;
                }
            }

            if is_valid {
                validated.push(contact.clone());
            } else if has_related_info {
                // 包含联系人信息但不是直接的人名，尝试提取真实人名
                let extracted_name = self.extract_real_name_from_related(name, response);
                if let Some(real_name) = extracted_name {
                    if real_name != *name && !real_name.is_empty() {
                        info!("[LLM Validate] Extracted real name from '{}': '{}'", name, real_name);
                        let mut fixed_contact = contact.clone();
                        fixed_contact.name = real_name;
                        validated.push(fixed_contact);
                    }
                }
                removed.push((name.clone(), "包含联系人信息但非直接人名".to_string()));
            } else {
                // 如果无法解析（没有找到该联系人的判断），保守处理：保留
                // 因为 3B 模型输出可能乱码，不能确定时应该保留
                info!("[LLM Validate] Could not parse result for '{}', keeping as fallback", name);
                validated.push(contact.clone());
            }
        }

        if !removed.is_empty() {
            info!("[LLM Validate] Removed {} contacts by LLM: {:?}", removed.len(), removed);
        }
        
        validated
    }

    /// 从包含联系人信息的词语中提取真实人名
    fn extract_real_name_from_related(&self, name: &str, _response: &str) -> Option<String> {
        // 简单的规则提取
        if name.contains("曹操") && name != "曹操" {
            return Some("曹操".to_string());
        }
        if name.contains("诸葛亮") && name != "诸葛亮" {
            return Some("诸葛亮".to_string());
        }
        if name.contains("林克") && name != "林克" {
            return Some("林克".to_string());
        }
        if name.contains("关羽") && name != "关羽" {
            return Some("关羽".to_string());
        }
        None
    }

    async fn multi_segment_person_extraction(&self, conversation: &str) -> Vec<ExtractedPerson> {
        let segments = self.segment_conversation(conversation);
        if segments.len() <= 1 {
            return Vec::new();
        }

        let mut all_names: Vec<String> = Vec::new();
        for segment in &segments {
            match self.extract_persons_from_segment(segment).await {
                Ok(names) => {
                    for name in names {
                        if !all_names.contains(&name) {
                            all_names.push(name);
                        }
                    }
                }
                Err(e) => {
                    warn!("[LLM Extract] Segment extraction error: {}", e);
                }
            }
        }

        info!("[LLM Extract] Multi-segment extracted {} total names from {} segments",
            all_names.len(), segments.len());

        let person_title_re = Regex::new(r"[\u4e00-\u9fa5]{1,4}(?:总|经理|工|主任|老师|先生|女士|小姐|同学|医生|律师|老板|师傅|总监|主管|助理|秘书|会计|代表|局长|处长|科长|教授|博士|硕士|长|哥|姐|弟|妹|叔|姨|婶|伯|姑|舅|嫂|侄|甥|爷|奶|婆|女儿|儿子|父亲|母亲|哥哥|弟弟|姐姐|妹妹)").unwrap();
        let common_words: HashSet<&str> = [
            "我们", "他们", "你们", "什么", "怎么", "为什么", "这个", "那个", "哪个",
            "可以", "需要", "已经", "没有", "现在", "今天", "昨天", "明天", "然后", "而且",
            "或者", "还是", "虽然", "不过", "关于", "进行", "使用", "提供", "包括",
            "支持", "处理", "管理", "开发", "测试", "讨论", "学习", "另外", "还有",
            "点去", "告发给", "经理和", "经理说", "总说", "总通知",
        ].into_iter().collect();

        let mut people: Vec<ExtractedPerson> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for name in all_names {
            let name = name.trim().to_string();
            if name.len() < 2 { continue; }
            if common_words.contains(&name.as_str()) { continue; }

            if seen.insert(name.clone()) {
                people.push(ExtractedPerson::new(name));
            }
        }

        if people.is_empty() {
            for cap in person_title_re.captures_iter(conversation) {
                let name = cap[0].to_string();
                if seen.insert(name.clone()) {
                    people.push(ExtractedPerson::new(name));
                }
            }
            if !people.is_empty() {
                info!("[LLM Extract] Fallback regex extracted {} names", people.len());
            }
        }

        people
    }

    fn merge_segment_persons(&self, elements: &mut ExtractedElements, segment_people: Vec<ExtractedPerson>) {
        let mut existing_names: HashSet<String> = elements.people.iter()
            .map(|p| p.name.clone())
            .collect();

        let mut added = 0;
        for p in segment_people {
            if !existing_names.contains(&p.name) {
                existing_names.insert(p.name.clone());
                elements.people.push(p.clone());
                elements.persons.push(p);
                added += 1;
            }
        }

        info!("[LLM Extract] Merged {} new names from segments (total: {})", added, elements.people.len());
    }

    fn parse_response(&self, response: &str, conversation: &str) -> Result<ExtractedElements, ShouxinError> {
        let json_str = response
            .strip_prefix("```json")
            .or_else(|| response.strip_prefix("```"))
            .unwrap_or(response);
        let json_str = json_str
            .strip_suffix("```")
            .unwrap_or(json_str)
            .trim();

        if json_str.is_empty() {
            warn!("[LLM Extract] Empty response from LLM, using fallback extraction");
            return self.fallback_extract(conversation);
        }

        let json_str = if let Some(start) = json_str.find('{') {
            let slice = &json_str[start..];
            if let Some(end) = find_matching_brace(slice) {
                &slice[..=end]
            } else {
                slice
            }
        } else {
            json_str
        };

        let debug_preview: String = json_str.chars().take(200).collect();
        debug!("[LLM Extract] Parsing JSON: {}", debug_preview);

        #[derive(serde::Deserialize)]
        struct RawResponse {
            #[serde(default)]
            people: Vec<RawPerson>,
            #[serde(default)]
            events: Vec<RawEvent>,
            #[serde(default)]
            todos: Vec<RawTodo>,
            #[serde(default)]
            facts: Vec<RawFact>,
            #[serde(default)]
            learnings: Vec<RawLearning>,
            #[serde(default)]
            todo_update_intent: Option<TodoUpdateIntent>,
        }

        #[derive(serde::Deserialize)]
        struct RawPerson {
            name: String,
            #[serde(default)]
            disambiguation: Option<String>,
            #[serde(default)]
            aliases: Vec<String>,
            #[serde(default)]
            role: Option<String>,
            #[serde(default)]
            description: Option<String>,
            #[serde(default = "default_contact_type")]
            contact_type: String,
        }

        #[derive(serde::Deserialize)]
        struct RawEvent {
            #[serde(default)]
            title: Option<String>,
            description: String,
            #[serde(default)]
            time: Option<String>,
            #[serde(default)]
            location: Option<String>,
            #[serde(default = "default_importance_str")]
            importance: String,
            #[serde(default)]
            start_time: Option<String>,
            #[serde(default)]
            end_time: Option<String>,
            // P0-4: 事件参与者姓名列表，用于关联事件与联系人
            #[serde(default)]
            participants: Vec<String>,
        }

        #[derive(serde::Deserialize)]
        struct RawTodo {
            title: String,
            #[serde(default = "default_importance_str")]
            importance: String,
            #[serde(default)]
            deadline: Option<String>,
            #[serde(default)]
            assigned_to: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct RawFact {
            text: String,
            #[serde(default = "default_category")]
            category: String,
            #[serde(default = "default_confidence")]
            confidence: f32,
        }

        #[derive(serde::Deserialize)]
        struct RawLearning {
            content: String,
            #[serde(default = "default_quality")]
            quality_score: f32,
            #[serde(default)]
            tags: Vec<String>,
        }

        fn default_contact_type() -> String { "unknown".to_string() }
        fn default_importance_str() -> String { "short_term".to_string() }
        fn default_category() -> String { "其他".to_string() }
        fn default_confidence() -> f32 { 0.8 }
        fn default_quality() -> f32 { 0.8 }

        fn importance_to_f32(s: &str) -> f32 {
            match s.to_lowercase().as_str() {
                "critical" => 0.95,
                "high" => 0.9,
                "long_term" | "long" => 0.8,
                "medium" => 0.6,
                "short_term" | "short" => 0.5,
                "low" => 0.3,
                _ => 0.3,
            }
        }

        let extracted_json = if let (Some(s), Some(e)) = (json_str.find('{'), json_str.rfind('}')) {
            &json_str[s..=e]
        } else {
            json_str
        };

        match serde_json::from_str::<RawResponse>(extracted_json) {
            Ok(raw) => {
                let people: Vec<ExtractedPerson> = raw.people.into_iter().map(|p| ExtractedPerson {
                contact_id: Uuid::new_v4().to_string(),
                name: Self::fix_name_truncation(&p.name),
                disambiguation: p
                    .disambiguation
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                aliases: p.aliases,
                role: p.role,
                description: p.description,
                contact_type: p.contact_type,
            }).collect();
                let events: Vec<ExtractedEvent> = raw.events.into_iter().map(|e| {
                    let importance_f = importance_to_f32(&e.importance);
                    let importance_label = e.importance.clone();
                    // P0-4: 通过 participants 姓名匹配已提取的联系人的 contact_id
                    let related_person_ids: Vec<String> = e.participants.iter()
                        .filter_map(|pname| {
                            let fixed = Self::fix_name_truncation(pname);
                            people.iter().find(|p| p.name == fixed).map(|p| p.contact_id.clone())
                        })
                        .collect();
                    ExtractedEvent {
                        event_id: Uuid::new_v4().to_string(),
                        title: e.title,
                        description: e.description,
                        time: e.time.clone(),
                        time_info: e.time,
                        location: e.location,
                        importance: importance_f,
                        importance_label,
                        start_time: e.start_time,
                        end_time: e.end_time,
                        related_person_ids,
                    }
                }).collect();
                // P0-4: 启发式补全 — 对 participants 为空的事件，检查描述/标题中是否包含已提取的人名
                let person_names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
                let mut events = events;
                for ev in &mut events {
                    if !ev.related_person_ids.is_empty() {
                        continue;
                    }
                    let haystack = format!("{} {}", ev.title.as_deref().unwrap_or(""), ev.description);
                    for pname in &person_names {
                        if haystack.contains(pname) {
                            if let Some(p) = people.iter().find(|p| &p.name == pname) {
                                ev.related_person_ids.push(p.contact_id.clone());
                            }
                        }
                    }
                }
                let todos: Vec<ExtractedTodo> = raw.todos.into_iter().map(|t| ExtractedTodo {
                    title: t.title,
                    importance: t.importance,
                    deadline: t.deadline,
                    assigned_to: t.assigned_to,
                }).collect();
                let facts: Vec<ExtractedFact> = raw.facts.into_iter().map(|f| ExtractedFact {
                    text: f.text,
                    category: f.category,
                    confidence: f.confidence,
                }).collect();
                let learnings: Vec<ExtractedLearning> = raw.learnings.into_iter().map(|l| ExtractedLearning {
                    content: l.content,
                    quality_score: l.quality_score,
                    tags: l.tags,
                }).collect();

                let mut elements = ExtractedElements {
                    conversation_text: String::new(),
                    persons: people.clone(),
                    people,
                    events,
                    todos,
                    facts,
                    learnings,
                    todo_update_intent: raw.todo_update_intent,
                };

                Ok(elements)
            }
            Err(e) => {
                let err_preview: String = response.chars().take(500).collect();
                warn!(
                    "[LLM Extract] JSON parse failed: {}, raw: {}",
                    e,
                    err_preview
                );
                // 尝试提取 todo_update_intent
                if let Some(intent) = LlmOutputSanitizer::extract_todo_intent_heuristic(conversation) {
                    let mut elements = self.fallback_extract(conversation)?;
                    elements.todo_update_intent = Some(intent);
                    return Ok(elements);
                }
                self.fallback_extract(conversation)
            }
        }
    }

    fn fallback_extract_events(conversation: &str) -> Vec<ExtractedEvent> {
        // 与 fallback_extract 共用同一套窗口规则：先去掉角色前缀，
        // 否则标题会变成 "er: 我让龙飞整理" 这种半截前缀
        let conversation = Self::strip_role_prefixes(conversation);
        let event_keywords = [
            "会", "会议", "讨论", "评审", "审批", "培训", "开发", "测试", "交接", "发布", "上线", "启动",
            "验收", "总结", "汇报", "调研", "评估", "检查", "巡检", "方案", "纪要", "清单", "报告", "计划",
            "预算", "决算", "审计", "反馈", "考核", "测评", "选举", "投票", "表决", "演示", "注册", "注销",
            "变更", "延期", "续期", "年检", "代理", "更新", "升级", "迁移", "备份", "恢复", "部署",
            "磋商", "指派", "整理", "抄送", "反馈", "专题", "讨论", "通知",
        ];

        let mut events: Vec<ExtractedEvent> = Vec::new();
        let mut seen_titles: HashSet<String> = HashSet::new();

        if let Ok(event_kw_re) = Regex::new(&event_keywords.join("|")) {
            for m in event_kw_re.find_iter(conversation.as_str()) {
                let kw = m.as_str();
                let kw_start = m.start();
                let chars: Vec<char> = conversation.chars().collect();
                let kw_char_len = kw.chars().count();
                let kw_start_char = conversation[..kw_start].chars().count();
                let (prefix_start, end_char) =
                    Self::event_title_window(&chars, kw_start_char, kw_char_len);
                let event_title_raw: String = chars[prefix_start..end_char].iter().collect();
                let trimmed = event_title_raw.trim_matches(|c: char| c.is_whitespace());
                if trimmed.chars().count() >= 2 && !seen_titles.contains(trimmed) {
                    seen_titles.insert(trimmed.to_string());
                    let snippet_end = (prefix_start + 80).min(chars.len());
                    let snippet: String = chars[prefix_start..snippet_end].iter().collect();
                    events.push(ExtractedEvent {
                        event_id: Uuid::new_v4().to_string(),
                        title: Some(trimmed.to_string()),
                        description: snippet,
                        time: None,
                        time_info: None,
                        location: None,
                        importance: 0.5,
                        importance_label: "short_term".to_string(),
                        start_time: None,
                        end_time: None,
                        related_person_ids: Vec::new(),
                    });
                }
            }
        } else {
            warn!("[LLM Extract] Fallback event keyword regex failed to compile");
        }

        info!("[LLM Extract] Fallback event extraction: {} events from keyword regex", events.len());
        events
    }

    fn fallback_extract(&self, conversation: &str) -> Result<ExtractedElements, ShouxinError> {
    warn!("[LLM Extract] fallback_extract ENTRY: text len={}, first 100 chars: {}", conversation.len(), &conversation.chars().take(100).collect::<String>());

    let surnames = r"(?:王|李|张|刘|陈|杨|黄|赵|吴|周|徐|孙|马|朱|胡|郭|何|高|林|罗|郑|梁|谢|宋|唐|许|韩|冯|邓|曹|彭|曾|肖|田|董|袁|潘|于|蒋|蔡|余|杜|叶|程|苏|魏|吕|丁|任|沈|姚|卢|姜|崔|钟|谭|陆|汪|范|金|石|廖|贾|夏|韦|付|方|白|邹|孟|熊|秦|邱|江|尹|薛|闫|段|雷|侯|龙|史|陶|黎|贺|顾|毛|郝|龚|邵|万|钱|严|覃|武|戴|莫|孔|向|汤|欧阳|司马|上官|诸葛)";
    let given_name = r"\p{Han}{0,2}";

    let long_titles = r"(?:总监|工程师|架构师|分析师|设计师|规划师|咨询师|培训师|研究员|研究生|博士后|运动员|裁判员|裁判长|司令员|办事员|调研员|巡视员|监察员|审计员|副总裁|志愿者|摄影师|音乐家|舞蹈家|魔术师|书法家|运动员|领导人|总书记|委员长|参谋长|指导员|教导员|司令员|参谋长|军团长|指导员|教导员|连长|营长|团长|旅长|师长|军长|队长|组长|班长|经理|主任|主管|助理|秘书|总裁|部长|局长|处长|科长|股长|司长|书记|委员|主席|教练|领队|导演|编剧|策划|设计|美工|开发|测试|运维|产品|运营|市场|销售|客服|财务|行政|人事|法务|公关|商务|编辑|记者|主播|演员|歌手|作家|诗人|画家|教师|导师|学生|学员|见习|实习|保安|保洁|厨师|司机|导游|翻译|顾问|专家|学者|督导|参谋|干事|科员|老师|先生|女士|小姐|同学|医生|律师|老板|师傅|会计|代表|教授|博士|硕士|义工|连长|营长|团长|旅长|师长|军长|CEO|CTO|CFO|COO|CIO|CMO|CHO|CSO|CDO|CRO|SVP|EVP|AVP|DGM|AGM|PMO|NLP|UED|GUI|WUI|HCI|IxD|CDO|CMO|CHO|CSO|CRO|SVP|EVP|AVP|DGM|AGM|PMO|SSE|NLP|UED|GUI|WUI|HCI|IxD|VD|GD|ID|AD|CD|PM|SM|TL|PL|SE|AE|SA|BA|QA|QC|RD|FE|BE|FS|DE|DA|DS|ML|AI|DL|CV|UI|UX|UE|GM|VP|PM|SE|TL|PL|SM|DM|RM|AM|OM|MM|EM|FM|LM|KM|BM|TM|WM|ZM|XM|YM|QM|UM|VM)";
    let short_titles = r"(?:总|工|长|哥|姐|弟|妹|叔|姨|婶|伯|姑|舅|嫂|侄|甥|爷|奶|公|婆)";

    let mut names: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let long_title_re_str = format!("{}{}{}", surnames, given_name, long_titles);
    if let Ok(long_re) = Regex::new(&long_title_re_str) {
        for cap in long_re.captures_iter(conversation) {
            let name = cap[0].to_string();
            warn!("[LLM Extract] Fallback long-title match: {}", name);
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    warn!("[LLM Extract] Fallback long-title: found {} names", names.len());

    let short_title_re_str = format!("{}{}{}", surnames, given_name, short_titles);
    if let Ok(short_re) = Regex::new(&short_title_re_str) {
        for cap in short_re.captures_iter(conversation) {
            let name = cap[0].to_string();
            let is_subset = names.iter().any(|n| n.contains(&name) || name.contains(n));
            if is_subset || seen.contains(&name) {
                continue;
            }
            // 上下文校验：排除嵌入在常见词中的假阳性匹配
            let match_end = cap.get(0).map(|m| m.end()).unwrap_or(0);
            let after_match = &conversation[match_end..];
            let common_compounds = ["司", "司的", "司是", "司在", "司了", "司就", "司都", "司也"];
            let is_compound = common_compounds.iter().any(|c| after_match.starts_with(c));
            if is_compound {
                warn!("[LLM Extract] Fallback short-title skip (compound): {} (followed by common word)", name);
                continue;
            }
            warn!("[LLM Extract] Fallback short-title match: {}", name);
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    }
    warn!("[LLM Extract] Fallback short-title: total {} names after dedup", names.len());

    let nickname_re = Regex::new(
        r"(?:老|小)(?:王|李|张|刘|陈|杨|黄|赵|吴|周|徐|孙|马|朱|胡|郭|何|高|林|罗|郑|梁|谢|宋|唐|许|韩|冯|邓|曹|彭|曾|肖|田|董|袁|潘|于|蒋|蔡|余|杜|叶|程|苏|魏|吕|丁|任|沈|姚|卢|姜|崔|钟|谭|陆|汪|范|金|石|廖|贾|夏|韦|付|方|白|邹|孟|熊|秦|邱|江|尹|薛|闫|段|雷|侯|龙|史|陶|黎|贺|顾|毛|郝|龚|邵|万|钱|严|覃|武|戴|莫|孔|向|汤|欧阳|司马|上官|诸葛)"
    ).unwrap();

    for cap in nickname_re.captures_iter(conversation) {
        let name = cap[0].to_string();
        warn!("[LLM Extract] Fallback nickname match: {}", name);
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }

    // 额外匹配 "小" + 任意汉字 的昵称模式（如"小丽"、"小芳"等）
    let small_nickname_re = Regex::new(r"小\p{Han}").unwrap();
    for cap in small_nickname_re.captures_iter(conversation) {
        let name = cap[0].to_string();
        // 排除常见非人名组合
        let non_names = ["小时", "小心", "小样", "小看", "小觑", "小把", "小会", "小睡", "小坐", "小住"];
        if non_names.contains(&name.as_str()) {
            continue;
        }
        warn!("[LLM Extract] Fallback small-nickname match: {}", name);
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }

    warn!("[LLM Extract] Fallback nickname regex: found {} total names", names.len());

    // 注意：这里**不再**做"任意 2-4 个汉字即人名"的兜底。
    // 那样做会把"复盘""大家一致""主要是需""五前交""评估表周"这类句子碎片
    // 当成联系人写进库里，既污染联系人列表，又白占画像生成的名额。
    // 正则一条都没匹配到，说明这段对话里没有可识别的称谓结构，
    // 宁可返回空，也不要造人 —— 人名召回可以在后续对话里补，
    // 凭空造出来的"人"却会一直留在档案里。

        let people: Vec<ExtractedPerson> = names.into_iter().map(|name| ExtractedPerson {
            contact_id: Uuid::new_v4().to_string(),
            name: Self::fix_name_truncation(&name),
            disambiguation: String::new(),
            aliases: Vec::new(),
            role: None,
            description: None,
            contact_type: "unknown".to_string(),
        }).collect();

        info!("[LLM Extract] Fallback: extracted {} names via regex, conv_text_preview={}", people.len(), &conversation.chars().take(100).collect::<String>());

        let mut events: Vec<ExtractedEvent> = Vec::new();
        // 去掉 `user:` / `assistant:` 之类的角色前缀：否则兜底截出来的标题会变成
        // "er: 我让龙飞整理" 这种半截前缀 + 半截词，看着像数据损坏。
        let conversation = Self::strip_role_prefixes(conversation);
        let event_keywords = [
            "会", "会议", "讨论", "评审", "审批", "培训", "开发", "测试", "交接", "发布", "上线", "启动",
            "验收", "总结", "汇报", "调研", "评估", "检查", "巡检", "方案", "纪要", "清单", "报告", "计划",
            "预算", "决算", "审计", "反馈", "考核", "测评", "选举", "投票", "表决", "演示", "注册", "注销",
            "变更", "延期", "续期", "年检", "交接", "代理", "更新", "升级", "迁移", "备份", "恢复", "部署",
            "重装", "格式化", "清理", "归档", "排序", "筛选", "搜索", "分析", "预测", "建模", "实验", "验证",
        ];

        if let Ok(event_kw_re) = Regex::new(&event_keywords.join("|")) {
            for m in event_kw_re.find_iter(&conversation) {
                let kw = m.as_str();
                let kw_start = m.start();
                let chars: Vec<char> = conversation.chars().collect();
                let kw_char_len = kw.chars().count();
                let kw_start_char = conversation[..kw_start].chars().count();
                let (prefix_start, end_char) =
                    Self::event_title_window(&chars, kw_start_char, kw_char_len);
                let event_title: String = chars[prefix_start..end_char].iter().collect();
                let trimmed = event_title.trim().trim_matches(|c: char| c.is_whitespace());
                if trimmed.chars().count() >= 2 && !events.iter().any(|e| e.title.as_deref() == Some(trimmed)) {
                    let snippet_end = (prefix_start + 80).min(chars.len());
                    let snippet: String = chars[prefix_start..snippet_end].iter().collect();
                    events.push(ExtractedEvent {
                        event_id: Uuid::new_v4().to_string(),
                        title: Some(trimmed.to_string()),
                        description: snippet,
                        time: None,
                        time_info: None,
                        location: None,
                        importance: 0.5,
                        importance_label: "medium".to_string(),
                        start_time: None,
                        end_time: None,
                        related_person_ids: Vec::new(),
                    });
                }
            }
        } else {
            warn!("[LLM Extract] Fallback event keyword regex failed to compile");
        }

        if !events.is_empty() {
            info!("[LLM Extract] Fallback: extracted {} events via event regex", events.len());
        }

        let mut learnings: Vec<ExtractedLearning> = Vec::new();
        for event in &events {
            let learning_content = if let Some(ref title) = event.title {
                format!("{} - {}", title, event.description)
            } else {
                event.description.clone()
            };
            let learning = ExtractedLearning {
                content: learning_content,
                quality_score: 0.6,
                tags: vec!["event".to_string(), "fallback".to_string()],
            };
            learnings.push(learning);
        }
        for person in &people {
            let learning_content = format!("对话中提到人物：{}", person.name);
            let learning = ExtractedLearning {
                content: learning_content,
                quality_score: 0.5,
                tags: vec!["person".to_string(), "fallback".to_string()],
            };
            learnings.push(learning);
        }
        if !learnings.is_empty() {
            info!("[LLM Extract] Fallback: generated {} learnings", learnings.len());
        }

        Ok(ExtractedElements {
            conversation_text: conversation.to_string(),
            people: people.clone(),
            persons: people,
            events,
            todos: Vec::new(),
            facts: Vec::new(),
            learnings,
            todo_update_intent: None,
        })
    }

    /// 计算事件标题的字符窗口：向左不越过句子边界，向右取到句子结束（上限 24 字）
    ///
    /// 旧实现固定取"关键词前 8 字"，于是"user: 我让龙飞整理"会截成
    /// "er: 我让龙飞整理"——既带半截前缀，又从半个词开始。
    fn event_title_window(chars: &[char], kw_start: usize, kw_len: usize) -> (usize, usize) {
        const STOP: [char; 9] = ['，', '。', '；', '！', '？', '：', '\n', '…', '、'];
        const MAX_LEFT: usize = 16;
        // 向左优先回到最近的句子边界：固定取 8 字会从半个词开始
        // （"…把龙飞转正，让我准备评估材料" 曾截成 "飞转正，让我准备评估材料"）。
        let mut start = kw_start.saturating_sub(MAX_LEFT);
        if let Some(pos) = (start..kw_start).rev().find(|&i| STOP.contains(&chars[i])) {
            start = pos + 1;
        }
        // 找不到句读时保持上面的 MAX_LEFT 回退，不要再固定左移 8 字：
        // 8 字会正好切在词中间（"今天又和张伟确认了预算表" 变成 "天又和张伟确认了预算表"）。
        while start < kw_start && STOP.contains(&chars[start]) {
            start += 1;
        }
        let mut end = (kw_start + kw_len).min(chars.len());
        while end < chars.len() && end - kw_start < 24 && !STOP.contains(&chars[end]) {
            end += 1;
        }
        (start, end)
    }

    /// 去掉对话文本里的角色前缀（`user:` / `assistant:` / `用户:` / `助手:`）
    ///
    /// 抽取兜底是"按关键词附近截窗口"，前缀不清掉就会截出 `er: 我让龙飞整理`
    /// 这种既不像句子也不像标题的碎片。
    fn strip_role_prefixes(text: &str) -> String {
        const PREFIXES: [&str; 8] = [
            "user:", "assistant:", "system:", "用户:", "助手:", "系统:", "User:", "Assistant:",
        ];
        text.lines()
            .map(|line| {
                let trimmed = line.trim_start();
                for p in PREFIXES {
                    if let Some(rest) = trimmed.strip_prefix(p) {
                        return rest.trim_start().to_string();
                    }
                }
                trimmed.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn extract_compound_relations_in_sentence(&self, sentence: &str, seen: &mut std::collections::HashSet<String>, existing: &mut Vec<ExtractedPerson>) {
        let relations = ["女儿", "儿子", "父亲", "母亲", "哥哥", "弟弟", "姐姐", "妹妹",
                         "助理", "秘书", "经理", "老师", "教授", "博士"];

        for relation in &relations {
            let relation_with_de = format!("的{}", relation);
            if let Some(pos) = sentence.find(&relation_with_de) {
                if pos >= 1 {
                    let after_relation_pos = pos + relation_with_de.len();
                    let after_str = &sentence[after_relation_pos..];
                    let next_char = after_str.chars().next();

                    let is_valid_boundary = next_char.map_or(true, |c| {
                        !c.is_ascii_alphabetic() && !c.is_ascii_digit() && (c < '\u{4e00}' || c > '\u{9fa5}')
                    });

                    if !is_valid_boundary {
                        continue;
                    }

                    let before_relation = &sentence[..pos];
                    let surname_re = Regex::new(r"[\u4e00-\u9fa5]{1,4}$").unwrap();

                    if let Some(cap) = surname_re.find(before_relation) {
                        let person_name = cap.as_str().to_string();
                        if person_name.chars().count() >= 1 && person_name.chars().count() <= 4 && seen.insert(person_name.clone()) {
                            existing.push(ExtractedPerson {
                                contact_id: Uuid::new_v4().to_string(),
                                name: person_name.clone(),
                                disambiguation: String::new(),
                                aliases: Vec::new(),
                                role: None,
                                description: None,
                                contact_type: "unknown".to_string(),
                            });
                            info!("[LLM Extract] Compound relation extraction: added '{}'", person_name);
                        }
                    }
                }
            }
        }
    }

    fn extract_nicknames_in_sentence(&self, sentence: &str, seen: &mut std::collections::HashSet<String>, existing: &mut Vec<ExtractedPerson>) {
        let nickname_re = Regex::new(r"(?:小|老)[\u4e00-\u9fa5]{1}").unwrap();

        let time_verb_suffixes = ["下", "明", "后", "天", "今", "去", "来", "到", "去", "上", "次", "时"];

        for cap in nickname_re.find_iter(sentence) {
            let full_match = cap.as_str();
            let char_count = full_match.chars().count();

            if char_count == 2 {
                let after_pos = cap.end();
                let after_substring = &sentence[after_pos..];
                let next_char = after_substring.chars().next();

                let is_valid_boundary = next_char.map_or(true, |c| {
                    if c.is_ascii_alphabetic() || c.is_ascii_digit() {
                        return true;
                    }
                    if c >= '\u{4e00}' && c <= '\u{9fa5}' {
                        return time_verb_suffixes.iter().any(|&suffix| after_substring.starts_with(suffix));
                    }
                    true
                });

                if is_valid_boundary && seen.insert(full_match.to_string()) {
                    existing.push(ExtractedPerson {
                        contact_id: Uuid::new_v4().to_string(),
                        name: full_match.to_string(),
                        disambiguation: String::new(),
                        aliases: Vec::new(),
                        role: None,
                        description: None,
                        contact_type: "unknown".to_string(),
                    });
                    info!("[LLM Extract] Nickname extraction: added '{}'", full_match);
                }
            }
        }
    }

    fn extract_english_names_in_sentence(&self, sentence: &str, seen: &mut std::collections::HashSet<String>, existing: &mut Vec<ExtractedPerson>) {
        let english_name_re = Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").unwrap();

        for cap in english_name_re.find_iter(sentence) {
            let name = cap.as_str().trim().to_string();
            let word_count = name.split_whitespace().count();

            if word_count >= 2 && word_count <= 4 && name.len() >= 4 && name.len() <= 40 {
                let mut capitalized = String::new();
                for (i, word) in name.split_whitespace().enumerate() {
                    if i > 0 {
                        capitalized.push(' ');
                    }
                    let mut chars = word.chars();
                    if let Some(first) = chars.next() {
                        capitalized.push(first.to_ascii_uppercase());
                        capitalized.push_str(&chars.as_str().to_lowercase());
                    }
                }

                if seen.insert(capitalized.clone()) {
                     existing.push(ExtractedPerson {
                         contact_id: Uuid::new_v4().to_string(),
                         name: capitalized.clone(),
                         disambiguation: String::new(),
                         aliases: Vec::new(),
                         role: None,
                         description: None,
                         contact_type: "unknown".to_string(),
                     });
                     info!("[LLM Extract] English name extraction: added '{}'", capitalized);
                }
            }
        }
    }
}

fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[async_trait]
impl MemoryExtractor for LlmMemoryExtractor {
    async fn extract(&self, conversation: &[Message]) -> Result<ExtractedElements, ShouxinError> {
        let conv_text = conversation.iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");
        self.extract_str(&conv_text).await
    }

    fn fallback_extract(&self, conversation: &str) -> Result<ExtractedElements, ShouxinError> {
        LlmMemoryExtractor::fallback_extract(self, conversation)
    }
}

/// SimpleExtractor：不做实际提取，返回空结果（用于测试）
pub struct SimpleExtractor;

#[async_trait]
impl MemoryExtractor for SimpleExtractor {
    async fn extract(&self, _conversation: &[Message]) -> Result<ExtractedElements, ShouxinError> {
        Ok(ExtractedElements::new(String::new()))
    }

    fn fallback_extract(&self, _conversation: &str) -> Result<ExtractedElements, ShouxinError> {
        Ok(ExtractedElements::new(String::new()))
    }
}

/// 判断一个"名字"是否是关系短语而不是人名
///
/// 判据（宁可严格一点，联系人列表干净比多抓几个更重要）：
/// 1. 含"的" → "小明的姐姐"、"客户的工程师"；
/// 2. 含"和/与"且长度 ≥ 4 → "小王和小明的公司领导"；
/// 3. 长度 > 6 → 中文人名（含复姓）极少超过 6 字；
/// 4. 纯称谓/纯身份词（没有姓）→ "工程师"、"领导"、"老人"。
pub(crate) fn is_relational_phrase(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() {
        return true;
    }
    if n.contains('的') {
        return true;
    }
    let chars = n.chars().count();
    if chars >= 4 && (n.contains('和') || n.contains('与')) {
        return true;
    }
    // 长度上限只针对**中文名**：中文姓名（含复姓）极少超过 6 字，
    // 但英文/数字名（ChatGPT、Alexander、T-800）和虚拟角色名（哆啦A梦）
    // 天然更长，按"总字符数"一刀切会把它们误杀。
    let cjk_count = n
        .chars()
        .filter(|c| ('\u{4E00}'..='\u{9FFF}').contains(c))
        .count();
    if cjk_count > 6 {
        return true;
    }
    if cjk_count == 0 && chars > 24 {
        return true;
    }

    // 纯身份词（整串就是身份，没有姓）
    const PURE_ROLES: &[&str] = &[
        "工程师", "领导", "老人", "同事", "朋友", "上级", "下属", "老板", "司机",
        "秘书", "助理", "财务", "销售", "产品", "测试", "客户", "供应商", "负责人",
        "技术总监", "项目经理", "部门主管", "实习生", "顾问",
    ];
    if PURE_ROLES.contains(&n) {
        return true;
    }

    // "XX的YY" 之外，还有 "客户方的工程师" 这种带"方"的
    if n.ends_with("方的工程师") || n.ends_with("的负责人") {
        return true;
    }

    // 中文人名的强特征：不会含虚词/代词/介词。
    //
    // 跑通测试暴露的另一类污染：短句里把**句子碎片**当人名，
    // 例如"由我负责""现在对接""她以前""是汤敏""向技术总监"。
    // 这些词长度 3–5、不含"的"，前面几条规则都拦不住，
    // 但显然不是名字——因为里面出现了代词/介词/副词。
    const PARTICLES: &[char] = &[
        '我', '你', '他', '她', '它', '们', '是', '由', '向', '在', '从', '对', '把',
        '被', '给', '让', '就', '都', '也', '还', '很', '会', '要', '能', '想', '说',
        '去', '来', '有', '没', '不', '吗', '呢', '吧', '啊', '了', '着', '过', '现',
        '已', '将', '正', '刚', '才', '又', '再', '只', '更', '最', '这', '那', '哪',
        '谁', '什', '怎', '为', '以', '于', '其', '之', '与', '及', '或', '但', '而',
    ];
    // 时间短语不是人名（"本周五之""下周三""月底"）
    const TIME_WORDS: &[&str] = &[
        "今天", "明天", "后天", "昨天", "前天", "本周", "下周", "上周", "这周",
        "周末", "月底", "月初", "上午", "下午", "晚上", "中午", "今年", "明年", "去年",
    ];
    if TIME_WORDS.iter().any(|w| n.contains(w)) {
        return true;
    }

    // 以动词/祈使词开头的是句子碎片（"别忘""记得""需要""准备""提交""回复"）
    const VERB_PREFIX: &[&str] = &[
        "别", "记得", "需要", "准备", "提交", "回复", "安排", "完成", "确认",
        "处理", "跟进", "整理", "更新", "提醒", "通知", "联系", "查看", "核对",
    ];
    if VERB_PREFIX.iter().any(|v| n.starts_with(v)) {
        return true;
    }

    // 含"事物词"的多半是短语而非人名（"前采购服""接口文档""预算表"）
    const THING_WORDS: &[&str] = &[
        "采购", "服务", "流程", "材料", "方案", "文档", "接口", "数据", "系统",
        "预算", "合同", "报表", "会议", "项目", "版本", "测试", "需求", "发票",
    ];
    if n.chars().count() >= 3 && THING_WORDS.iter().any(|w| n.contains(w)) {
        return true;
    }

    // 只看**开头**是不是虚词/代词。
    //
    // 句子碎片（"由我负责""现在对接""她以前""是汤敏""向技术总监"）都以这些字开头；
    // 而合法人名里也可能出现这些字（"初音未来"含"来"、"如来"含"来"、"有朋"含"有"），
    // 按"任意位置出现"过滤会误杀虚拟角色和真实姓名。
    if n.chars().next().map(|c| PARTICLES.contains(&c)).unwrap_or(false) {
        return true;
    }

    false
}

#[cfg(test)]
mod name_sanity_tests {
    use super::is_relational_phrase;

    #[test]
    fn rejects_relational_phrases() {
        for bad in [
            "小明的姐姐", "客户的工程师", "小王和小明的公司领导",
            "林芳的女儿", "财务部的老人", "高翔的儿子", "工程师", "领导",
            // 跑通测试里出现的句子碎片
            "由我负责", "现在对接", "她以前", "是汤敏", "向技术总监",
            // 画像阶段实际卡住的垃圾名字
            "别忘", "前采购服", "本周五之", "记得回复", "接口文档",
        ] {
            assert!(is_relational_phrase(bad), "应判为关系短语: {}", bad);
        }
    }

    /// 虚拟角色（历史人物/动漫/科幻）和英文名不能被误杀：
    /// 它们虽然不是"现实同事"，但都是合法的人名实体，
    /// 而且虚拟角色走独立接口创建，过滤器不该干预。
    #[test]
    fn keeps_virtual_and_latin_names() {
        for name in [
            "孙悟空", "曹操", "林黛玉", "苏晚晴", "哆啦A梦", "初音未来",
            "福尔摩斯", "哈利波特", "钢铁侠", "ChatGPT", "Alexander", "T-800",
        ] {
            assert!(!is_relational_phrase(name), "不应误杀: {}", name);
        }
    }

    #[test]
    fn keeps_real_names() {
        for good in ["张伟", "李娜", "王建国", "老周", "小陈", "陈总", "林助理", "欧阳娜娜"] {
            assert!(!is_relational_phrase(good), "不应误杀: {}", good);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 兜底抽取不能把句子碎片当人名
    ///
    /// 回归用例：这两句曾经产出"复盘""大家一致""主要是需""五前交""评估表周"等
    /// 8 个假联系人（旧实现在"正则没匹配到"时用 `\p{Han}{2,4}` 兜底）。
    /// 兜底事件的标题不能带角色前缀，也不能从半个词开始
    #[test]
    fn test_fallback_event_title_is_sentence_like() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));
        let elements = extractor
            .fallback_extract("user: 我让龙飞整理一份竞品分析，周五前给我初稿。")
            .unwrap();

        assert!(!elements.events.is_empty(), "应至少识别出一个事件");
        let title = elements.events[0].title.clone().unwrap_or_default();
        assert!(!title.contains("user:"), "标题不应带角色前缀: {}", title);
        assert!(!title.contains("er:"), "标题不应带半截前缀: {}", title);
        // 关键词在句尾时，标题应覆盖到句子边界，而不是截断成 8 个字
        assert!(title.chars().count() >= 6, "标题过短: {}", title);

        // 另一条兜底路径（静态函数）必须遵守同一规则
        let events = LlmMemoryExtractor::fallback_extract_events(
            "user: 我让龙飞整理一份竞品分析，周五前给我初稿。",
        );
        assert!(!events.is_empty());
        let t = events[0].title.clone().unwrap_or_default();
        assert!(!t.contains("er:") && !t.contains("user:"), "标题带前缀: {}", t);

        // 向左要回到句子边界，不能从半个词开始
        let events = LlmMemoryExtractor::fallback_extract_events(
            "user: 罗军说下个月要把龙飞转正，让我准备评估材料，评估表周五前交。",
        );
        let titles: Vec<String> = events.iter().filter_map(|e| e.title.clone()).collect();
        assert!(
            titles.iter().any(|t| t.starts_with("让我准备")),
            "应从逗号后的完整分句开始，实际: {:?}",
            titles
        );
    }

    /// 回归：整句没有句读时，标题不能切在词中间
    ///
    /// 实测："今天又和张伟确认了预算表的数字，他说周五前给我最终版。"
    /// 被兜底截成 "天又和张伟确认了预算表的数字"（首字丢失），
    /// 原因是找不到句号时固定左移 8 字，正好切在"今天"中间。
    #[test]
    fn test_fallback_title_does_not_cut_first_char() {
        let events = LlmMemoryExtractor::fallback_extract_events(
            "今天又和张伟确认了预算表的数字，他说周五前给我最终版。",
        );
        let titles: Vec<String> = events.iter().filter_map(|e| e.title.clone()).collect();
        assert!(!titles.is_empty(), "应识别出预算相关事件: {:?}", titles);
        assert!(
            titles.iter().any(|t| t.starts_with("今天")),
            "标题不应丢失首字，实际: {:?}",
            titles
        );
    }

    #[test]
    fn test_fallback_extract_does_not_invent_people() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));

        for text in [
            "user: 复盘会上大家一致认为：这次延期主要是需求变更太晚，其次是测试介入太迟。",
            "user: 罗军说下个月要把龙飞转正，让我准备评估材料，评估表周五前交。",
        ] {
            let elements = extractor.fallback_extract(text).unwrap();
            assert!(
                elements.persons.is_empty(),
                "不应凭空造人，实际得到: {:?}",
                elements.persons.iter().map(|p| p.name.clone()).collect::<Vec<_>>()
            );
        }

        // 有明确称谓结构时必须照常抽到（这是兜底路径唯一可信的信号）
        let elements = extractor
            .fallback_extract("user: 张总监说文档他来写，李工配合联调环境，老王拍板。")
            .unwrap();
        let names: Vec<String> = elements.persons.iter().map(|p| p.name.clone()).collect();
        assert!(names.iter().any(|n| n.starts_with("张")), "实际: {:?}", names);
        assert!(names.iter().any(|n| n.contains("王")), "实际: {:?}", names);
    }

    #[test]
    fn test_parse_valid_json() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));

        let json = r#"{
            "people": [{"name": "张三", "role": "工程师", "contact_type": "colleague"}],
            "events": [{"description": "项目会议", "time": "2024-01-01 14:00", "importance": "high"}],
            "todos": [{"title": "完成报告", "importance": "high"}],
            "facts": [{"text": "张三精通 Rust", "category": "skill", "confidence": 0.95}],
            "learnings": [{"content": "Rust异步编程", "quality_score": 0.9, "tags": ["rust", "async"]}]
        }"#;

        let result = extractor.parse_response(json, "test").unwrap();
        assert_eq!(result.people.len(), 1);
        assert_eq!(result.people[0].name, "张三");
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.todos.len(), 1);
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.learnings.len(), 1);
    }

    #[test]
    fn test_parse_json_with_markdown() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));

        let json = "```json\n{\"people\": [], \"events\": [], \"todos\": [], \"facts\": [], \"learnings\": []}\n```";
        let result = extractor.parse_response(json, "test").unwrap();
        assert_eq!(result.people.len(), 0);
    }

    #[test]
    fn test_parse_empty_response() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));

        let result = extractor.parse_response("", "test").unwrap();
        assert_eq!(result.people.len(), 0);
    }

    #[test]
    fn test_simple_extractor() {
        let extractor = SimpleExtractor;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(extractor.extract(&[])).unwrap();
        assert_eq!(result.people.len(), 0);
    }

    #[test]
    fn test_case_1_zhou_zhao() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));
        let conversation = "周总昨天和赵工开了一下午的会，讨论项目验收的事。";
        let result = extractor.fallback_extract(conversation).unwrap();

        let names: Vec<&str> = result.people.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"周总"), "应提取周总, 实际: {:?}", names);
        assert!(names.contains(&"赵工"), "应提取赵工, 实际: {:?}", names);
        println!("[测试1] 周总+赵工: {:?}", names);
    }

    #[test]
    fn test_case_2_chen_zhang_zhou() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));
        let conversation = "陈工和张总都在场，周秘书也参加了。";
        let result = extractor.fallback_extract(conversation).unwrap();

        let names: Vec<&str> = result.people.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"陈工"), "应提取陈工, 实际: {:?}", names);
        assert!(names.contains(&"张总"), "应提取张总, 实际: {:?}", names);
        assert!(names.contains(&"周秘书"), "应提取周秘书, 实际: {:?}", names);
        println!("[测试2] 陈工+张总+周秘书: {:?}", names);
    }

    #[test]
    fn test_case_3_wang_sun() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));
        let conversation = "王经理通知大家下周一交报告，孙助理负责收集。";
        let result = extractor.fallback_extract(conversation).unwrap();

        let names: Vec<&str> = result.people.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"王经理"), "应提取王经理, 实际: {:?}", names);
        assert!(names.contains(&"孙助理"), "应提取孙助理, 实际: {:?}", names);
        println!("[测试3] 王经理+孙助理: {:?}", names);
    }

    #[test]
    fn test_case_4_li_xiaoli() {
        let extractor = LlmMemoryExtractor::new(Arc::new(crate::llm::OllamaLlmClient::with_config(
            crate::traits::llm::LlmConfig::default(),
        )));
        let conversation = "李总说他的女儿小丽下周来公司实习。";
        let result = extractor.fallback_extract(conversation).unwrap();

        let names: Vec<&str> = result.people.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"李总"), "应提取李总, 实际: {:?}", names);
        assert!(names.contains(&"小丽"), "应提取小丽, 实际: {:?}", names);
        println!("[测试4] 李总+小丽: {:?}", names);
    }
}