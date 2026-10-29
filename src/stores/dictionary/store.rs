//! 字典库实现（sled 存储）
//!
//! 提供永久全量存档功能，只写入不删除

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;

use crate::core::{ContactPatch, ContactTotal}; use crate::core::contact::OwnerType;
use crate::error::ShouxinError;
use crate::traits::store_traits::{DictionaryStoreTrait, ContactRecord, Conversation};

/// 字典库存储实现
///
/// 使用 sled 作为存储引擎，提供永久全量存档
pub struct DictionaryStore {
    /// sled 数据库实例
    db: Db,
    /// 对话存储树
    conversations: sled::Tree,
    /// 联系人总表树
    contacts_total: sled::Tree,
    /// 联系人历史事件档案树
    contact_event_history: sled::Tree,
    /// 文件存储树
    files: sled::Tree,
    /// 时间戳索引树（用于增量扫描）
    conversation_ts_index: sled::Tree,
    /// ID 索引树：conversation_id → conversation_key (v0.5.1)
    id_index: sled::Tree,
    /// 跨库写入意图树（两阶段提交的状态机）
    commit_intents: sled::Tree,
}

/// 跨库写入意图状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteIntentState {
    /// 已开始，派生库正在写入（对外不可见/可补偿）
    Prepared,
    /// 三库写入完成
    Committed,
    /// 已失败，需要补偿清理
    Aborted,
}

impl WriteIntentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            WriteIntentState::Prepared => "prepared",
            WriteIntentState::Committed => "committed",
            WriteIntentState::Aborted => "aborted",
        }
    }
}

/// 一次"提取结果落三库"的提交意图
///
/// 字典库不可回滚，因此用这份记录标记跨库写入的进度：
/// 崩溃/失败后据此判断哪些派生数据需要补偿清理（见
/// `TRANSACTION_WRITER_DESIGN.md` 第九节）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteIntent {
    /// 关联对话（一次提取对应一条对话）
    pub conversation_id: String,
    pub state: WriteIntentState,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub owner_type: String,
    pub owner_id: String,
    /// 本次写入新建的事件 ID（用于回滚关系库）
    #[serde(default)]
    pub created_event_ids: Vec<String>,
    /// 本次写入新建的向量记忆 ID（用于回滚向量库）
    #[serde(default)]
    pub created_memory_ids: Vec<String>,
    /// 失败原因（仅 Aborted 时有值）
    #[serde(default)]
    pub error: Option<String>,
}

impl WriteIntent {
    pub fn prepared(
        conversation_id: impl Into<String>,
        owner_type: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            conversation_id: conversation_id.into(),
            state: WriteIntentState::Prepared,
            started_at: now,
            updated_at: now,
            owner_type: owner_type.into(),
            owner_id: owner_id.into(),
            created_event_ids: Vec::new(),
            created_memory_ids: Vec::new(),
            error: None,
        }
    }
}

impl DictionaryStore {
    /// 打开或创建字典库
    ///
    /// # 参数
    /// - `path`: 数据库存储路径
    ///
    /// # 返回
    /// DictionaryStore 实例
    ///
    /// # 示例
    /// ```ignore
    /// let store = DictionaryStore::open("./data/dictionary")?;
    /// ```
    pub fn open(path: &str) -> Result<Self, ShouxinError> {
        // 打开或创建 sled 数据库
        let db = sled::open(path)?;

        // 创建或打开对话存储树
        let conversations = db.open_tree("conversations")?;

        // 创建或打开联系人总表树
        let contacts_total = db.open_tree("contacts_total")?;

        // 创建或打开联系人历史事件档案树
        let contact_event_history = db.open_tree("contact_event_history")?;

        let files = db.open_tree("files")?;

        // 创建或打开时间戳索引树（v0.5.0 新增）
        let conversation_ts_index = db.open_tree("conversation_ts_index")?;

        // 创建或打开 ID 索引树（v0.5.1 新增）
        let id_index = db.open_tree("conversation_id_index")?;

        // 跨库写入意图树（两阶段提交）
        let commit_intents = db.open_tree("commit_intents")?;

        let store = Self {
            db,
            conversations,
            contacts_total,
            contact_event_history,
            files,
            conversation_ts_index,
            id_index,
            commit_intents,
        };

        // 确保用户记录存在
        store.ensure_user_exists()?;

        Ok(store)
    }

    /// 获取 sled 数据库引用（用于创建额外的 tree）
    pub fn db(&self) -> &sled::Db {
        &self.db
    }

    /// 确保用户记录存在
    ///
    /// 系统启动时自动创建用户记录，ID 固定为 `__self__`
    fn ensure_user_exists(&self) -> Result<(), ShouxinError> {
        let user_id = "__self__";
        let key = Self::contact_key(user_id);

        if !self.contacts_total.contains_key(key.as_bytes())? {
            let user = ContactTotal {
                id: user_id.to_string(),
                name: "我".to_string(),
                aliases: vec!["用户".to_string()],
                is_self: true,
                created_at: Utc::now(),
                last_interaction_at: Utc::now(),
                related_contact_ids: Vec::new(),
                relationship_tags: Vec::new(),
                relation_weight: None,
                background: None,
                notes: None,
                metadata: std::collections::HashMap::new(),
                owner_type: OwnerType::User,
                owner_id: "default_user".to_string(),
                is_agent: false,
                parent_agent_id: None,
                is_virtual: false,
            };

            // 序列化为 JSON
            let json = serde_json::to_vec(&user)?;

            // 写入 sled
            self.contacts_total.insert(key.as_bytes(), json)?;
        }

        Ok(())
    }

    /// 创建内存模式的字典库（用于测试）
    pub fn in_memory() -> Result<Self, ShouxinError> {
        let db = sled::Config::new().temporary(true).open()?;
        let conversations = db.open_tree("conversations")?;
        let contacts_total = db.open_tree("contacts_total")?;
        let contact_event_history = db.open_tree("contact_event_history")?;
        let files = db.open_tree("files")?;
        let conversation_ts_index = db.open_tree("conversation_ts_index")?;
        let id_index = db.open_tree("conversation_id_index")?;
        let commit_intents = db.open_tree("commit_intents")?;

        let store = Self {
            db,
            conversations,
            contacts_total,
            contact_event_history,
            files,
            conversation_ts_index,
            id_index,
            commit_intents,
        };

        // 确保用户记录存在
        store.ensure_user_exists()?;

        Ok(store)
    }

    fn file_key(file_id: &str) -> String {
        format!("file:{}:text", file_id)
    }

    /// 生成对话存储键
    ///
    /// 格式：{date}:{id}
    /// 例如：2024-01-15:uuid-1234
    fn conversation_key(timestamp: DateTime<Utc>, id: &str) -> String {
        let date = timestamp.format("%Y-%m-%d").to_string();
        format!("{}:{}", date, id)
    }

    /// 生成联系人存储键
    ///
    /// 格式：contact:{id}
    /// 例如：contact:uuid-1234
    fn contact_key(id: &str) -> String {
        format!("contact:{}", id)
    }

    /// 生成联系人历史事件存储键
    ///
    /// 格式：contact_event:{contact_id}:{event_id}
    /// 例如：contact_event:contact-123:event-456
    fn contact_event_key(contact_id: &str, event_id: &str) -> String {
        format!("contact_event:{}:{}", contact_id, event_id)
    }

    /// 生成时间戳索引键
    ///
    /// 格式：{timestamp}:{conversation_key}
    /// 例如：2024-01-15T10:30:00Z:2024-01-15:uuid-1234
    fn ts_index_key(timestamp: DateTime<Utc>, conv_key: &str) -> String {
        format!("{}:{}", timestamp.to_rfc3339(), conv_key)
    }

    /// 保存对话记录
    ///
    /// # 参数
    /// - `id`: 对话唯一标识
    /// - `content`: 对话内容
    /// - `timestamp`: 对话时间戳
    /// - `owner_type`: 所有者类型
    /// - `owner_id`: 所有者 ID
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// store.save_conversation("conv-123", "你好，世界", Utc::now(), OwnerType::User, "user1").await?;
    /// ```
    pub async fn save_conversation(
        &self,
        id: &str,
        content: &str,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
    ) -> Result<(), ShouxinError> {
        // 创建对话记录
        let record = ConversationRecord::new_with_owner(
            id.to_string(),
            content.to_string(),
            timestamp,
            owner_type,
            owner_id.to_string(),
        );

        // 序列化为 JSON
        let json = serde_json::to_vec(&record)?;

        // 生成存储键
        let conv_key = Self::conversation_key(timestamp, id);

        // 写入对话树
        self.conversations.insert(conv_key.as_bytes(), json)?;

        // 写入时间戳索引树（v0.5.0 新增）
        let ts_key = Self::ts_index_key(timestamp, &conv_key);
        self.conversation_ts_index.insert(ts_key.as_bytes(), conv_key.as_bytes())?;

        // 写入 ID 索引树（v0.5.1 新增）
        self.id_index.insert(id.as_bytes(), conv_key.as_bytes())?;

        Ok(())
    }

    /// 原子保存对话并写入待提取任务（同一 sled 事务）
    ///
    /// 强事务边界：对话原文、时间戳索引、ID 索引、待提取任务要么全部写入，要么全部不写入。
    pub fn save_conversation_with_task_atomic(
        &self,
        id: &str,
        content: &str,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        task_key: &str,
        task_value: &[u8],
    ) -> Result<(), ShouxinError> {
        use sled::Transactional;

        let record = ConversationRecord::new_with_owner(
            id.to_string(),
            content.to_string(),
            timestamp,
            owner_type,
            owner_id.to_string(),
        );
        let json = serde_json::to_vec(&record)?;
        let conv_key = Self::conversation_key(timestamp, id);
        let ts_key = Self::ts_index_key(timestamp, &conv_key);
        let pending_tasks = self
            .db
            .open_tree(crate::stores::dictionary::PENDING_EXTRACTION_TASKS_TREE)?;

        let json_value = json.clone();
        let conv_key_value = conv_key.as_bytes().to_vec();
        let task_key_owned = task_key.to_string();
        let task_value_owned = task_value.to_vec();

        (&self.conversations, &self.conversation_ts_index, &self.id_index, &pending_tasks)
            .transaction(|(convs, ts_idx, id_idx, tasks)| {
                convs.insert(conv_key.as_bytes(), json_value.clone())?;
                ts_idx.insert(ts_key.as_bytes(), conv_key_value.clone())?;
                id_idx.insert(id.as_bytes(), conv_key_value.clone())?;
                tasks.insert(task_key_owned.as_bytes(), task_value_owned.clone())?;
                Ok(())
            })
            .map_err(|e: sled::transaction::TransactionError<()>| {
                ShouxinError::StorageError(format!(
                    "atomic save_conversation_with_task failed: {:?}",
                    e
                ))
            })?;

        Ok(())
    }

    /// 获取指定时间戳之后的所有对话（增量扫描）
    ///
    /// # 参数
    /// - `since`: 起始时间戳
    ///
    /// # 返回
    /// 对话记录列表（按时间升序）
    ///
    /// # 示例
    /// ```ignore
    /// let conversations = store.get_conversations_since(last_processed_ts).await?;
    /// ```
    pub async fn get_conversations_since(&self, since: DateTime<Utc>) -> Result<Vec<ConversationRecord>, ShouxinError> {
        self.get_conversations_since_v2(since, 1000).await
    }

    /// 取某 owner 最近的若干条对话（不区分抽取状态）
    ///
    /// 与 `get_conversations_since_v2` 的区别：后者**只返回 pending/failed**（供后台队列消费），
    /// 用它做"历史召回"会永远拿不到已完成对话。画像生成需要的是原文证据，
    /// 因此单开一个按 owner 取最近 N 条的方法。
    pub async fn get_recent_conversations_for_owner(
        &self,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, ShouxinError> {
        let mut results = Vec::new();
        if limit == 0 {
            return Ok(results);
        }

        // 从时间索引尾部往前扫（索引键为时间戳，天然按时间有序）。
        // 必须限制扫描条数：若该 owner 的对话很少，无限倒扫会退化成整库遍历，
        // 心跳每轮都这么做会持续读盘。这里最多扫 500 条，够覆盖近期上下文。
        const MAX_SCAN: usize = 500;
        for item in self.conversation_ts_index.iter().rev().take(MAX_SCAN) {
            if results.len() >= limit {
                break;
            }
            let (_, conv_key_bytes) = item?;
            let conv_key = String::from_utf8_lossy(&conv_key_bytes);
            if let Some(conv_data) = self.conversations.get(conv_key.as_bytes())? {
                if let Ok(record) = serde_json::from_slice::<ConversationRecord>(&conv_data) {
                    if record.owner_type == owner_type && record.owner_id == owner_id {
                        results.push(record);
                    }
                }
            }
        }

        Ok(results)
    }

    /// 批量获取指定时间戳之后的所有对话（优化版）
    ///
    /// 使用前缀扫描优化，性能更好
    ///
    /// # 参数
    /// - `since`: 起始时间戳
    /// - `limit`: 最大返回数量
    ///
    /// # 返回
    /// 对话记录列表
    pub async fn get_conversations_since_v2(&self, since: DateTime<Utc>, limit: usize) -> Result<Vec<ConversationRecord>, ShouxinError> {
        let mut results = Vec::new();
        let since_str = since.to_rfc3339();

        // 用 range 直接定位到起始时间，避免每次心跳都从索引头部全量扫描。
        // sled 的键按字节序排列，而 ts_index_key 以 RFC3339（UTC、同一时区偏移）开头，
        // 字典序即时间序，因此 range(since..) 等价于"时间 >= since"。
        for item in self.conversation_ts_index.range(since_str.as_bytes()..) {
            if results.len() >= limit {
                break;
            }

            let (_index_key_bytes, conv_key_bytes) = item?;
            let conv_key = String::from_utf8_lossy(&conv_key_bytes);

            if let Some(conv_data) = self.conversations.get(conv_key.as_bytes())? {
                if let Ok(conv_record) = serde_json::from_slice::<ConversationRecord>(&conv_data) {
                    // 只处理未被 background_queue 处理过的对话
                    // status="pending" 表示 background_queue 尚未处理
                    // status="completed"/"processing" 表示已处理，跳过避免重复
                    // status="failed" 允许心跳重试
                    if conv_record.extracted_status == "pending" || conv_record.extracted_status == "failed" {
                        results.push(conv_record);
                    }
                }
            }
        }

        results.sort_by_key(|c| c.timestamp);

        Ok(results)
    }

    /// 保存联系人总表记录
    ///
    /// # 参数
    /// - `contact`: 联系人总表记录
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// let contact = ContactTotal::new("contact-123".to_string(), "张三".to_string());
    /// store.save_contact(&contact).await?;
    /// ```
    pub async fn save_contact(&self, contact: &ContactTotal) -> Result<(), ShouxinError> {
        // 序列化为 JSON
        let json = serde_json::to_vec(contact)?;

        // 生成存储键
        let key = Self::contact_key(&contact.id);

        // 写入 sled
        self.contacts_total.insert(key.as_bytes(), json)?;

        Ok(())
    }

    /// 以增量补丁的方式原子更新联系人
    ///
    /// "读取-合并-写入"在同一个 sled 事务内完成，避免并发写入丢更新。
    /// 补丁未提供的字段保留权威存档中的原值。
    ///
    /// # 参数
    /// - `patch`: 联系人增量补丁
    ///
    /// # 返回
    /// 合并后的完整联系人记录
    pub fn patch_contact_atomic(&self, patch: &ContactPatch) -> Result<ContactTotal, ShouxinError> {
        use sled::transaction::{ConflictableTransactionError, TransactionError};

        let key = Self::contact_key(&patch.contact_id);

        self.contacts_total
            .transaction(|tx| {
                let existing: Option<ContactTotal> = match tx.get(key.as_bytes())? {
                    Some(bytes) => Some(serde_json::from_slice(&bytes).map_err(|e| {
                        ConflictableTransactionError::Abort(format!(
                            "failed to deserialize contact {}: {}",
                            patch.contact_id, e
                        ))
                    })?),
                    None => None,
                };

                let mut contact = match existing {
                    Some(contact) => contact,
                    None => patch.materialize(&patch.contact_id),
                };
                patch.apply_to(&mut contact);

                let json = serde_json::to_vec(&contact).map_err(|e| {
                    ConflictableTransactionError::Abort(format!(
                        "failed to serialize contact {}: {}",
                        patch.contact_id, e
                    ))
                })?;
                tx.insert(key.as_bytes(), json)?;

                Ok(contact)
            })
            .map_err(|e: TransactionError<String>| {
                ShouxinError::StorageError(format!("atomic patch_contact failed: {:?}", e))
            })
    }

    /// 以 `ContactRecord` 视图做"保守合并"写入
    ///
    /// `ContactRecord` 只携带姓名/元数据/时间等少量字段，直接整条覆盖会清空
    /// aliases、background、notes、relationship_tags 等业务字段。这里把它翻译成
    /// 增量补丁，只更新它真正掌握的字段。
    pub fn upsert_contact_record(&self, contact: &ContactRecord) -> Result<ContactTotal, ShouxinError> {
        let mut patch = ContactPatch::new(contact.contact_id.clone())
            .with_name(contact.name.clone())
            .touched_at(contact.updated_at);

        if let Some(ref user_id) = contact.user_id {
            patch = patch.with_owner(OwnerType::User, user_id.clone());
        }
        for (key, value) in &contact.metadata {
            patch.merge_metadata.insert(key.clone(), value.clone());
        }

        self.patch_contact_atomic(&patch)
    }

    /// 获取联系人总表记录
    ///
    /// # 参数
    /// - `id`: 联系人唯一标识
    ///
    /// # 返回
    /// 联系人记录，如果不存在则返回 None
    ///
    /// # 示例
    /// ```ignore
    /// let contact = store.get_contact("contact-123").await?;
    /// if let Some(c) = contact {
    ///     println!("联系人姓名: {}", c.name);
    /// }
    /// ```
    pub async fn get_contact(&self, id: &str) -> Result<Option<ContactTotal>, ShouxinError> {
        // 生成存储键
        let key = Self::contact_key(id);

        // 从 sled 读取
        if let Some(bytes) = self.contacts_total.get(key.as_bytes())? {
            // 反序列化 JSON
            let contact: ContactTotal = serde_json::from_slice(&bytes)?;
            Ok(Some(contact))
        } else {
            Ok(None)
        }
    }

    /// 获取所有联系人
    ///
    /// # 返回
    /// 所有联系人记录列表
    pub async fn get_all_contacts(&self) -> Result<Vec<ContactTotal>, ShouxinError> {
        let mut contacts = Vec::new();

        // 遍历联系人总表树
        for item in self.contacts_total.iter() {
            let (_, bytes) = item?;
            let contact: ContactTotal = serde_json::from_slice(&bytes)?;
            contacts.push(contact);
        }

        Ok(contacts)
    }

    /// 按日期查询对话
    ///
    /// # 参数
    /// - `date`: 日期字符串（YYYY-MM-DD）
    /// - `owner_type`: 所有者类型（可选）
    /// - `owner_id`: 所有者 ID（可选）
    ///
    /// # 返回
    /// 该日期的所有对话记录
    pub async fn get_conversations_by_date(
        &self,
        date: &str,
        owner_type: Option<crate::core::contact::OwnerType>,
        owner_id: Option<&str>,
    ) -> Result<Vec<ConversationRecord>, ShouxinError> {
        let mut conversations = Vec::new();
        let prefix = format!("{}:", date);

        // 按前缀扫描
        for item in self.conversations.scan_prefix(prefix.as_bytes()) {
            let (_, bytes) = item?;
            let record: ConversationRecord = serde_json::from_slice(&bytes)?;
            
            // 应用 owner 过滤
            let matches_owner = match (&owner_type, owner_id) {
                (Some(ot), Some(oid)) => record.owner_type == *ot && record.owner_id == oid,
                (Some(ot), None) => record.owner_type == *ot,
                (None, Some(oid)) => record.owner_id == oid,
                (None, None) => true,
            };
            
            if matches_owner {
                conversations.push(record);
            }
        }

        Ok(conversations)
    }

    /// 保存联系人历史事件记录
    ///
    /// # 参数
    /// - `record`: 联系人历史事件记录
    ///
    /// # 返回
    /// 操作结果
    ///
    /// # 示例
    /// ```ignore
    /// let record = ContactEventRecord::new(
    ///     "event-123".to_string(),
    ///     "contact-456".to_string(),
    ///     "小李弄丢了书".to_string(),
    ///     Utc::now(),
    ///     "short_term".to_string(),
    ///     "negative".to_string(),
    ///     Some("conv-789".to_string()),
    /// );
    /// store.save_contact_event(&record).await?;
    /// ```
    pub async fn save_contact_event(&self, record: &ContactEventRecord) -> Result<(), ShouxinError> {
        // 序列化为 JSON
        let json = serde_json::to_vec(record)?;

        // 生成存储键
        let key = Self::contact_event_key(&record.contact_id, &record.event_id);

        // 写入 sled
        self.contact_event_history.insert(key.as_bytes(), json)?;

        Ok(())
    }

    /// 获取联系人的所有历史事件记录
    ///
    /// # 参数
    /// - `contact_id`: 联系人唯一标识
    /// - `limit`: 限制返回数量（可选，None 表示返回全部）
    ///
    /// # 返回
    /// 该联系人的历史事件记录列表（按时间倒序）
    ///
    /// # 示例
    /// ```ignore
    /// let events = store.get_contact_events("contact-123", Some(20)).await?;
    /// for event in events {
    ///     println!("事件: {}", event.event_summary);
    /// }
    /// ```
    pub async fn get_contact_events(
        &self,
        contact_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ContactEventRecord>, ShouxinError> {
        let mut events = Vec::new();
        let prefix = format!("contact_event:{}:", contact_id);

        // 按前缀扫描
        for item in self.contact_event_history.scan_prefix(prefix.as_bytes()) {
            let (_, bytes) = item?;
            let record: ContactEventRecord = serde_json::from_slice(&bytes)?;
            events.push(record);
        }

        // 按时间倒序排序
        events.sort_by_key(|b| std::cmp::Reverse(b.event_time));

        // 应用限制
        if let Some(limit) = limit {
            events.truncate(limit);
        }

        Ok(events)
    }

    /// 刷新数据到磁盘
    pub fn flush(&self) -> Result<usize, ShouxinError> {
        Ok(self.db.flush()?)
    }

    // -----------------------------------------------------------------------
    // 跨库写入意图（两阶段提交）
    // -----------------------------------------------------------------------

    fn intent_key(conversation_id: &str) -> String {
        format!("intent:{}", conversation_id)
    }

    /// 开始一次跨库写入（落 `prepared` 意图）
    pub fn begin_write_intent(&self, intent: &WriteIntent) -> Result<(), ShouxinError> {
        let key = Self::intent_key(&intent.conversation_id);
        let json = serde_json::to_vec(intent)?;
        self.commit_intents.insert(key.as_bytes(), json)?;
        Ok(())
    }

    /// 记录本次写入新建的资源（事件/向量记忆），供失败后补偿清理
    pub fn record_intent_resources(
        &self,
        conversation_id: &str,
        event_ids: &[String],
        memory_ids: &[String],
    ) -> Result<bool, ShouxinError> {
        let key = Self::intent_key(conversation_id);
        let Some(bytes) = self.commit_intents.get(key.as_bytes())? else {
            return Ok(false);
        };
        let mut intent: WriteIntent = serde_json::from_slice(&bytes)?;
        for id in event_ids {
            if !intent.created_event_ids.contains(id) {
                intent.created_event_ids.push(id.clone());
            }
        }
        for id in memory_ids {
            if !intent.created_memory_ids.contains(id) {
                intent.created_memory_ids.push(id.clone());
            }
        }
        intent.updated_at = Utc::now();
        self.commit_intents
            .insert(key.as_bytes(), serde_json::to_vec(&intent)?)?;
        Ok(true)
    }

    /// 结束一次跨库写入（`committed` / `aborted`）
    pub fn finish_write_intent(
        &self,
        conversation_id: &str,
        state: WriteIntentState,
        error: Option<&str>,
    ) -> Result<bool, ShouxinError> {
        let key = Self::intent_key(conversation_id);
        let Some(bytes) = self.commit_intents.get(key.as_bytes())? else {
            return Ok(false);
        };
        let mut intent: WriteIntent = serde_json::from_slice(&bytes)?;
        intent.state = state;
        intent.updated_at = Utc::now();
        intent.error = error.map(|s| s.to_string());
        self.commit_intents
            .insert(key.as_bytes(), serde_json::to_vec(&intent)?)?;
        Ok(true)
    }

    /// 列出指定状态的意图（`None` 表示全部）
    pub fn list_write_intents(
        &self,
        state: Option<WriteIntentState>,
    ) -> Result<Vec<WriteIntent>, ShouxinError> {
        let mut intents = Vec::new();
        for item in self.commit_intents.iter() {
            let (_, bytes) = item?;
            let intent: WriteIntent = serde_json::from_slice(&bytes)?;
            if state.as_ref().map(|s| s == &intent.state).unwrap_or(true) {
                intents.push(intent);
            }
        }
        intents.sort_by_key(|i| i.started_at);
        Ok(intents)
    }

    /// 关闭数据库（刷新缓冲区）
    ///
    /// 注意：由于该方法接收 `&self`，无法真正释放底层 sled::Db 资源。
    /// `drop(self)` 在引用上是空操作，已移除。实际的文件锁释放发生在
    /// `DictionaryStore` 所有者被 drop、`db: Db` 引用计数归零时。
    /// 此处仅保证已写入的数据刷新到磁盘。
    pub fn close(&self) -> Result<(), ShouxinError> {
        self.db.flush()?;
        Ok(())
    }

    pub fn save_file_text(&self, file_id: &str, json: &[u8]) -> Result<(), ShouxinError> {
        let key = Self::file_key(file_id);
        self.files.insert(key.as_bytes(), json)?;
        Ok(())
    }

    pub fn get_file_text(&self, file_id: &str) -> Result<Option<Vec<u8>>, ShouxinError> {
        let key = Self::file_key(file_id);
        match self.files.get(key.as_bytes())? {
            Some(data) => Ok(Some(data.to_vec())),
            None => Ok(None),
        }
    }

    pub fn delete_file_text(&self, file_id: &str) -> Result<(), ShouxinError> {
        let key = Self::file_key(file_id);
        self.files.remove(key.as_bytes())?;
        Ok(())
    }
}

/// 对话记录结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecord {
    /// 对话唯一标识
    pub id: String,
    /// 对话内容
    pub content: String,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 所有者类型（v0.3.0 新增，兼容旧数据默认 User）
    #[serde(default = "default_owner_type")]
    pub owner_type: crate::core::contact::OwnerType,
    /// 所有者 ID（v0.3.0 新增，兼容旧数据默认 default_user）
    #[serde(default = "default_owner_id")]
    pub owner_id: String,
    /// 提取状态: pending / processing / completed / failed (v0.5.1)
    #[serde(default = "default_extracted_status")]
    pub extracted_status: String,
    /// 提取错误信息 (v0.5.1)
    #[serde(default)]
    pub extraction_error: Option<String>,
}

fn default_extracted_status() -> String {
    "pending".to_string()
}

fn default_owner_type() -> crate::core::contact::OwnerType {
    crate::core::contact::OwnerType::User
}

fn default_owner_id() -> String {
    "default_user".to_string()
}

impl ConversationRecord {
    pub fn new(id: String, content: String, timestamp: DateTime<Utc>) -> Self {
        Self {
            id,
            content,
            timestamp,
            owner_type: crate::core::contact::OwnerType::User,
            owner_id: "default_user".to_string(),
            extracted_status: "pending".to_string(),
            extraction_error: None,
        }
    }

    /// 创建新的对话记录（带所有者信息）
    pub fn new_with_owner(
        id: String,
        content: String,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: String,
    ) -> Self {
        Self {
            id,
            content,
            timestamp,
            owner_type,
            owner_id,
            extracted_status: "pending".to_string(),
            extraction_error: None,
        }
    }
}

/// 联系人历史事件记录结构
///
/// 存储在字典库中，永久保留，用于联系人背景总结和风评分析
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactEventRecord {
    /// 事件唯一 ID
    pub event_id: String,
    /// 关联联系人 ID
    pub contact_id: String,
    /// 事件所有者 ID
    pub owner_id: String,
    /// 事件所有者类型
    pub owner_type: OwnerType,
    /// 事件一句话摘要
    pub event_summary: String,
    /// 事件发生时间
    pub event_time: DateTime<Utc>,
    /// 原始重要性（short_term/long_term/critical）
    pub importance: String,
    /// 用户对该事件的情感倾向（positive/neutral/negative）
    pub user_sentiment: String,
    /// 指向字典库中原始对话的 ID（便于回溯）
    pub raw_conversation_id: Option<String>,
}

impl ContactEventRecord {
    /// 创建新的联系人历史事件记录
    ///
    /// # 参数
    /// - `event_id`: 事件唯一 ID
    /// - `contact_id`: 关联联系人 ID
    /// - `owner_id`: 事件所有者 ID
    /// - `owner_type`: 事件所有者类型
    /// - `event_summary`: 事件一句话摘要
    /// - `event_time`: 事件发生时间
    /// - `importance`: 原始重要性
    /// - `user_sentiment`: 用户情感倾向
    /// - `raw_conversation_id`: 原始对话 ID（可选）
    ///
    /// # 返回
    /// ContactEventRecord 实例
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: String,
        contact_id: String,
        owner_id: String,
        owner_type: OwnerType,
        event_summary: String,
        event_time: DateTime<Utc>,
        importance: String,
        user_sentiment: String,
        raw_conversation_id: Option<String>,
    ) -> Self {
        Self {
            event_id,
            contact_id,
            owner_id,
            owner_type,
            event_summary,
            event_time,
            importance,
            user_sentiment,
            raw_conversation_id,
        }
    }
}

impl Default for DictionaryStore {
    fn default() -> Self {
        Self::in_memory().expect("Failed to create in-memory dictionary store")
    }
}

#[async_trait]
impl DictionaryStoreTrait for DictionaryStore {
    async fn save_conversation(&self, id: &str, content: &str, timestamp: DateTime<Utc>, owner_type: crate::core::contact::OwnerType, owner_id: &str) -> Result<(), ShouxinError> {
        let record = ConversationRecord {
            id: id.to_string(),
            content: content.to_string(),
            timestamp,
            owner_type,
            owner_id: owner_id.to_string(),
            extracted_status: "pending".to_string(),
            extraction_error: None,
        };

        let json = serde_json::to_vec(&record)?;

        let conv_key = Self::conversation_key(timestamp, id);

        self.conversations.insert(conv_key.as_bytes(), json)?;

        let ts_index_key = Self::ts_index_key(timestamp, &conv_key);
        self.conversation_ts_index.insert(ts_index_key.as_bytes(), conv_key.as_bytes())?;

        self.id_index.insert(id.as_bytes(), conv_key.as_bytes())?;

        Ok(())
    }

    async fn save_conversation_with_task(
        &self,
        id: &str,
        content: &str,
        timestamp: DateTime<Utc>,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        task_key: &str,
        task_value: &[u8],
    ) -> Result<(), ShouxinError> {
        self.save_conversation_with_task_atomic(
            id, content, timestamp, owner_type, owner_id, task_key, task_value,
        )
    }

    async fn get_conversation_by_time_range(&self, start_time: DateTime<Utc>, end_time: DateTime<Utc>, limit: usize) -> Result<Vec<Conversation>, ShouxinError> {
        let mut conversations = Vec::new();
        let start_date = start_time.format("%Y-%m-%d").to_string();
        let end_date = end_time.format("%Y-%m-%d").to_string();

        // 简化实现：按日期范围获取对话
        // 实际项目中可能需要更精确的时间范围查询
        let start_date_num: i64 = start_date.replace("-", "").parse().map_err(|e: std::num::ParseIntError| crate::error::ShouxinError::GeneralError(e.to_string()))?;
        let end_date_num: i64 = end_date.replace("-", "").parse().map_err(|e: std::num::ParseIntError| crate::error::ShouxinError::GeneralError(e.to_string()))?;

        for date_num in start_date_num..=end_date_num {
            let date_str = format!("{:04}-{:02}-{:02}", date_num / 10000, (date_num / 100) % 100, date_num % 100);
            let prefix = format!("{}:", date_str);

            // 按前缀扫描
            for item in self.conversations.scan_prefix(prefix.as_bytes()) {
                let (_, bytes) = item?;
                let record: ConversationRecord = serde_json::from_slice(&bytes)?;
                if record.timestamp >= start_time && record.timestamp <= end_time {
                    let (uid, aid) = match record.owner_type {
                        crate::core::contact::OwnerType::User => (Some(record.owner_id.clone()), None),
                        crate::core::contact::OwnerType::Agent => (None, Some(record.owner_id.clone())),
                        _ => (Some(record.owner_id.clone()), None),
                    };
                    let conversation = Conversation {
                        id: record.id,
                        content: record.content,
                        user_id: uid,
                        agent_id: aid,
                        created_at: record.timestamp,
                        extracted_status: record.extracted_status,
                        extraction_error: record.extraction_error,
                    };
                    conversations.push(conversation);
                    if conversations.len() >= limit {
                        break;
                    }
                }
            }
            if conversations.len() >= limit {
                break;
            }
        }

        Ok(conversations)
    }

    async fn save_contact(&self, contact: &ContactRecord) -> Result<(), ShouxinError> {
        // `ContactRecord` 是信息量较少的视图：整条覆盖会清空 aliases/background/
        // notes/relationship_tags 等业务字段（F-M1）。这里改为保守合并。
        self.upsert_contact_record(contact)?;
        Ok(())
    }

    async fn get_contact_total(&self, contact_id: &str) -> Result<Option<ContactTotal>, ShouxinError> {
        DictionaryStore::get_contact(self, contact_id).await
    }

    async fn patch_contact(&self, patch: &ContactPatch) -> Result<ContactTotal, ShouxinError> {
        self.patch_contact_atomic(patch)
    }

    async fn save_contact_event(
        &self,
        record: &crate::stores::dictionary::store::ContactEventRecord,
    ) -> Result<(), ShouxinError> {
        DictionaryStore::save_contact_event(self, record).await
    }

    async fn begin_write_intent(
        &self,
        intent: &crate::stores::dictionary::store::WriteIntent,
    ) -> Result<(), ShouxinError> {
        DictionaryStore::begin_write_intent(self, intent)
    }

    async fn finish_write_intent(
        &self,
        conversation_id: &str,
        state: crate::stores::dictionary::store::WriteIntentState,
        error: Option<&str>,
    ) -> Result<(), ShouxinError> {
        DictionaryStore::finish_write_intent(self, conversation_id, state, error).map(|_| ())
    }

    async fn record_write_intent_resources(
        &self,
        conversation_id: &str,
        event_ids: &[String],
        memory_ids: &[String],
    ) -> Result<(), ShouxinError> {
        DictionaryStore::record_intent_resources(self, conversation_id, event_ids, memory_ids)
            .map(|_| ())
    }

    async fn get_contact(&self, contact_id: &str) -> Result<Option<ContactRecord>, ShouxinError> {
        // 生成存储键
        let key = Self::contact_key(contact_id);

        // 从 sled 读取
        if let Some(bytes) = self.contacts_total.get(key.as_bytes())? {
            // 反序列化 JSON
            let contact_total: ContactTotal = serde_json::from_slice(&bytes)?;
            let contact_record = ContactRecord {
                contact_id: contact_total.id,
                name: contact_total.name,
                user_id: None, // 暂不支持
                metadata: contact_total.metadata,
                created_at: contact_total.created_at,
                updated_at: contact_total.last_interaction_at,
                activity_score: 0.0, // 暂不支持
            };
            Ok(Some(contact_record))
        } else {
            Ok(None)
        }
    }

    async fn get_all_contacts(&self, _user_id: Option<&str>) -> Result<Vec<ContactRecord>, ShouxinError> {
        let mut contacts = Vec::new();

        // 遍历联系人总表树
        for item in self.contacts_total.iter() {
            let (_, bytes) = item?;
            let contact_total: ContactTotal = serde_json::from_slice(&bytes)?;
            let contact_record = ContactRecord {
                contact_id: contact_total.id,
                name: contact_total.name,
                user_id: None, // 暂不支持
                metadata: contact_total.metadata,
                created_at: contact_total.created_at,
                updated_at: contact_total.last_interaction_at,
                activity_score: 0.0, // 暂不支持
            };
            contacts.push(contact_record);
        }

        Ok(contacts)
    }

    async fn update_conversation_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<(), ShouxinError> {
        if let Some(conv_key_bytes) = self.id_index.get(id.as_bytes())? {
            let conv_key = String::from_utf8_lossy(&conv_key_bytes);
            if let Some(data) = self.conversations.get(conv_key.as_bytes())? {
                let mut record: ConversationRecord = serde_json::from_slice(&data)?;
                record.extracted_status = status.to_string();
                if let Some(e) = error {
                    record.extraction_error = Some(e.to_string());
                }
                let json = serde_json::to_vec(&record)?;
                self.conversations.insert(conv_key.as_bytes(), json)?;
            }
        }
        Ok(())
    }

    async fn search_conversations_by_owner_keywords(
        &self,
        owner_type: crate::core::contact::OwnerType,
        owner_id: &str,
        keywords: &[String],
        scan_limit: usize,
        limit: usize,
    ) -> Result<Vec<ConversationRecord>, ShouxinError> {
        let mut results = Vec::new();
        if keywords.is_empty() || limit == 0 || scan_limit == 0 {
            return Ok(results);
        }

        // 从时间索引尾部（最新）往前扫，最多 scan_limit 条就停。
        // 联系人详情是低频按需查询，但仍必须设上界，避免退化成整库遍历。
        for item in self.conversation_ts_index.iter().rev().take(scan_limit) {
            if results.len() >= limit {
                break;
            }
            let (_, conv_key_bytes) = item?;
            let conv_key = String::from_utf8_lossy(&conv_key_bytes);
            if let Some(conv_data) = self.conversations.get(conv_key.as_bytes())? {
                if let Ok(record) = serde_json::from_slice::<ConversationRecord>(&conv_data) {
                    if record.owner_type != owner_type || record.owner_id != owner_id {
                        continue;
                    }
                    if keywords.iter().any(|k| !k.is_empty() && record.content.contains(k.as_str())) {
                        results.push(record);
                    }
                }
            }
        }

        Ok(results)
    }

    async fn get_conversation_by_id(&self, id: &str) -> Result<Option<Conversation>, ShouxinError> {
        if let Some(conv_key_bytes) = self.id_index.get(id.as_bytes())? {
            let conv_key = String::from_utf8_lossy(&conv_key_bytes);
            if let Some(data) = self.conversations.get(conv_key.as_bytes())? {
                let record: ConversationRecord = serde_json::from_slice(&data)?;
                let (uid, aid) = match record.owner_type {
                    crate::core::contact::OwnerType::User => (Some(record.owner_id.clone()), None),
                    crate::core::contact::OwnerType::Agent => (None, Some(record.owner_id.clone())),
                    _ => (Some(record.owner_id.clone()), None),
                };
                return Ok(Some(Conversation {
                    id: record.id,
                    content: record.content,
                    user_id: uid,
                    agent_id: aid,
                    created_at: record.timestamp,
                    extracted_status: record.extracted_status,
                    extraction_error: record.extraction_error,
                }));
            }
        }
        Ok(None)
    }

    async fn list_failed_conversation_ids(&self, limit: usize) -> Result<Vec<String>, ShouxinError> {
        let mut ids = Vec::new();
        if limit == 0 {
            return Ok(ids);
        }
        for item in self.conversations.iter() {
            let (_, bytes) = item?;
            let record: ConversationRecord = serde_json::from_slice(&bytes)?;
            if record.extracted_status == "failed" {
                ids.push(record.id);
                if ids.len() >= limit {
                    break;
                }
            }
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::contact::OwnerType;

    #[test]
    fn test_save_conversation_with_task_atomic() {
        let store = DictionaryStore::in_memory().expect("in-memory dictionary");
        let before = store.conversations.len();
        let task_key = "task_atomic-test";
        let task_value: &[u8] = br#"{"conversation_id":"atomic-test","retry_count":0}"#;

        store
            .save_conversation_with_task_atomic(
                "atomic-test",
                "hello world",
                Utc::now(),
                OwnerType::User,
                "u1",
                task_key,
                task_value,
            )
            .expect("atomic save should succeed");

        assert_eq!(store.conversations.len(), before + 1);
        assert!(store.id_index.get("atomic-test").unwrap().is_some());

        let pending = store
            .db
            .open_tree(crate::stores::dictionary::PENDING_EXTRACTION_TASKS_TREE)
            .unwrap();
        let stored = pending.get(task_key).unwrap().expect("task should be persisted");
        assert_eq!(stored.as_ref(), task_value);
    }
}

#[cfg(test)]
mod write_intent_tests {
    use super::*;

    #[test]
    fn test_write_intent_state_machine() {
        let store = DictionaryStore::in_memory().unwrap();

        // prepared
        let intent = WriteIntent::prepared("conv-1", "User", "u1");
        store.begin_write_intent(&intent).unwrap();
        let prepared = store
            .list_write_intents(Some(WriteIntentState::Prepared))
            .unwrap();
        assert_eq!(prepared.len(), 1);
        assert_eq!(prepared[0].conversation_id, "conv-1");

        // committed
        assert!(store
            .finish_write_intent("conv-1", WriteIntentState::Committed, None)
            .unwrap());
        assert!(store
            .list_write_intents(Some(WriteIntentState::Prepared))
            .unwrap()
            .is_empty());
        let all = store.list_write_intents(None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].state, WriteIntentState::Committed);

        // 资源记录 + aborted
        let intent2 = WriteIntent::prepared("conv-2", "Agent", "primary_agent_default");
        store.begin_write_intent(&intent2).unwrap();
        assert!(store
            .record_intent_resources(
                "conv-2",
                &["e1".to_string()],
                &["m1".to_string(), "m2".to_string()]
            )
            .unwrap());
        store
            .finish_write_intent("conv-2", WriteIntentState::Aborted, Some("boom"))
            .unwrap();

        let all = store.list_write_intents(None).unwrap();
        let i2 = all
            .iter()
            .find(|i| i.conversation_id == "conv-2")
            .expect("intent should exist");
        assert_eq!(i2.state, WriteIntentState::Aborted);
        assert_eq!(i2.created_event_ids, vec!["e1".to_string()]);
        assert_eq!(i2.created_memory_ids.len(), 2);
        assert_eq!(i2.error.as_deref(), Some("boom"));
    }

    #[test]
    fn test_finish_unknown_intent_is_noop() {
        let store = DictionaryStore::in_memory().unwrap();
        // 不存在时不应报错，也不应创建记录
        assert!(!store
            .finish_write_intent("nope", WriteIntentState::Committed, None)
            .unwrap());
        assert!(store.list_write_intents(None).unwrap().is_empty());
    }
}

/// 批量重放的前置查询：按 `failed` 状态列出对话 ID
#[cfg(test)]
mod failed_listing_tests {
    use super::*;
    use crate::core::contact::OwnerType;

    #[tokio::test]
    async fn test_list_failed_conversation_ids() {
        let store = DictionaryStore::in_memory().unwrap();
        let now = Utc::now();
        for id in ["c1", "c2", "c3"] {
            store
                .save_conversation(id, "content", now, OwnerType::User, "u1")
                .await
                .unwrap();
        }

        store
            .update_conversation_status("c1", "failed", Some("boom"))
            .await
            .unwrap();
        store
            .update_conversation_status("c2", "completed", None)
            .await
            .unwrap();

        let ids = store.list_failed_conversation_ids(10).await.unwrap();
        assert_eq!(ids, vec!["c1".to_string()], "只应列出 failed 状态");

        // limit 生效
        store
            .update_conversation_status("c3", "failed", Some("boom2"))
            .await
            .unwrap();
        assert_eq!(store.list_failed_conversation_ids(1).await.unwrap().len(), 1);
        assert_eq!(store.list_failed_conversation_ids(0).await.unwrap().len(), 0);
    }
}
