use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn, error};

use crate::core::contact::OwnerType;
use crate::error::ShouxinError;
use crate::memory_extractor::{ExtractedElements, MemoryExtractor};
use crate::stores::dictionary::SledDictionaryStore;
use crate::stores::relational::RelationalStore;
use crate::traits::store_traits::DictionaryStoreTrait;
use crate::traits::{Embedder, VectorStore};

use crate::stores::dictionary::PENDING_EXTRACTION_TASKS_TREE;

const PENDING_TASKS_TREE: &str = PENDING_EXTRACTION_TASKS_TREE;
const TASK_KEY_PREFIX: &str = "task_";

/// 活跃联系人数量限制（与 dispatcher.rs 保持一致）
const MAX_ACTIVE_CONTACTS: usize = 50;

/// 学习素材最低质量分（与 dispatcher.rs 保持一致）
const MIN_LEARNING_QUALITY: f32 = 0.8;

/// 学习素材重复判定阈值（与 three_library_manager.rs 保持一致）
const LEARNING_DEDUP_THRESHOLD: f32 = 0.98;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractionTask {
    pub conversation_id: String,
    pub conversation_text: String,
    pub owner_type: OwnerType,
    pub owner_id: String,
    pub user_id: Option<String>,
    #[serde(default)]
    pub retry_count: u32,
    /// 对话实际发生时间（历史导入时不一定等于入库时间）
    ///
    /// 抽取里的"下周三""三天后"都必须以它为基准；用 Utc::now() 会让
    /// 10-20 说的"下周三"被算成 11-04（N-01）。
    #[serde(default)]
    pub conversation_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// 判断输入是否"病态"到不适合送进 LLM 分词器
///
/// 三条判据（任一命中即跳过 LLM）：
/// 1. 数字占比 > 50% 且长度 > 200（纯数字轰炸）；
/// 2. 同一字符连续出现 > 100 次（"哈哈哈哈…"）；
/// 3. 连续 > 400 字符没有任何标点或空白（超长单句）。
///
/// 这类文本既没有实体也没有关系，正则兜底足够；而送进 llama.cpp 会让
/// MSVC std::regex 递归爆掉并 abort 整个进程。
pub(crate) fn is_pathological_input(text: &str) -> bool {
    let len = text.chars().count();
    if len < 200 {
        return false;
    }

    let digits = text.chars().filter(|c| c.is_ascii_digit()).count();
    if digits * 2 > len {
        return true;
    }

    let mut same_run = 1usize;
    let mut max_same = 1usize;
    let mut no_punct_run = 0usize;
    let mut max_no_punct = 0usize;
    let mut prev: Option<char> = None;

    for ch in text.chars() {
        if Some(ch) == prev {
            same_run += 1;
            max_same = max_same.max(same_run);
        } else {
            same_run = 1;
        }
        prev = Some(ch);

        if ch.is_whitespace() || is_punct(ch) {
            no_punct_run = 0;
        } else {
            no_punct_run += 1;
            max_no_punct = max_no_punct.max(no_punct_run);
        }
    }

    max_same > 100 || max_no_punct > 400
}

fn is_punct(ch: char) -> bool {
    ch.is_ascii_punctuation()
        || matches!(ch,
            '，' | '。' | '、' | '；' | '：' | '？' | '！' | '“' | '”' | '‘' | '’'
            | '（' | '）' | '《' | '》' | '【' | '】' | '…' | '—' | '·' | '～')
}

// 事件合并阈值的唯一来源见 `crate::utils::merge_thresholds`：
// 同步路径（three_library_manager）和异步路径（本文件）**必须读同一份**，
// 否则改 config.toml 只会影响其中一条路径。
pub use crate::utils::merge_thresholds::{merge_thresholds, set_merge_thresholds};

pub struct BackgroundTaskQueue {
    sender: mpsc::UnboundedSender<ExtractionTask>,
    pending_tasks: Option<sled::Tree>,
}

impl BackgroundTaskQueue {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let queue = Self { sender: tx, pending_tasks: None };
        tokio::spawn(Self::worker(rx));
        queue
    }

    pub fn push(&self, task: ExtractionTask) -> Result<(), ShouxinError> {
        self.sender.send(task)
            .map_err(|e| ShouxinError::InternalError(format!("BackgroundTaskQueue: failed to push task (channel closed): {}", e)))
    }

    pub fn with_stores(
        dictionary: Arc<SledDictionaryStore>,
        relational: Arc<RelationalStore>,
        vector: Arc<dyn VectorStore>,
        extractor: Arc<dyn MemoryExtractor>,
        embedder: Arc<dyn Embedder>,
        config_user_id: String,
        extraction_timeout_secs: u64,
    ) -> Self {
        let pending_tasks = dictionary.db().open_tree(PENDING_TASKS_TREE)
            .expect("Failed to open pending_tasks tree");

        let state = Arc::new(WorkerState {
            dictionary: dictionary as Arc<dyn DictionaryStoreTrait>,
            relational,
            vector,
            extractor,
            embedder,
            config_user_id,
            extraction_timeout_secs,
            pending_tasks: pending_tasks.clone(),
        });

        let (tx, rx) = mpsc::unbounded_channel();
        let queue = Self {
            sender: tx,
            pending_tasks: Some(pending_tasks),
        };
        tokio::spawn(Self::worker_with_state(rx, state));
        queue
    }

    pub fn spawn_on_runtime(
        runtime_handle: tokio::runtime::Handle,
        dictionary: Arc<SledDictionaryStore>,
        relational: Arc<RelationalStore>,
        vector: Arc<dyn VectorStore>,
        extractor: Arc<dyn MemoryExtractor>,
        embedder: Arc<dyn Embedder>,
        config_user_id: String,
        extraction_timeout_secs: u64,
    ) -> Self {
        let pending_tasks = dictionary.db().open_tree(PENDING_TASKS_TREE)
            .expect("Failed to open pending_tasks tree");

        let state = Arc::new(WorkerState {
            dictionary: dictionary as Arc<dyn DictionaryStoreTrait>,
            relational,
            vector,
            extractor,
            embedder,
            config_user_id,
            extraction_timeout_secs,
            pending_tasks: pending_tasks.clone(),
        });

        let (tx, rx) = mpsc::unbounded_channel();
        let queue = Self {
            sender: tx,
            pending_tasks: Some(pending_tasks),
        };

        runtime_handle.spawn(async move {
            Self::worker_with_state(rx, state).await;
        });

        queue
    }

    pub fn recover_pending_tasks(&self) {
        let pending_tasks = match &self.pending_tasks {
            Some(tree) => tree,
            None => {
                info!("BackgroundTaskQueue: no pending_tasks store configured, skipping recovery");
                return;
            }
        };

        info!("BackgroundTaskQueue: recovering pending tasks from sled...");
        let mut recovered_count = 0;

        for result in pending_tasks.iter() {
            match result {
                Ok((key, value)) => {
                    let key_str = String::from_utf8_lossy(&key);
                    if key_str.starts_with(TASK_KEY_PREFIX) {
                        match serde_json::from_slice::<ExtractionTask>(&value) {
                            Ok(mut task) => {
                                // 每恢复一次就 +1：进程级崩溃（abort）不会走普通失败计数，
                                // 如果不在恢复时累加，同一条"毒输入"会让服务无限重启-崩溃。
                                task.retry_count = task.retry_count.saturating_add(1);

                                if task.retry_count > 3 {
                                    error!(
                                        "BackgroundTaskQueue: task {} 已导致进程崩溃 {} 次，放弃该条（保留原文，可手动重放）",
                                        task.conversation_id, task.retry_count - 1
                                    );
                                    let _ = pending_tasks.remove(&key);
                                    continue;
                                }

                                // 把累加后的次数写回，保证下次重启还能继续计数
                                if let Ok(bytes) = serde_json::to_vec(&task) {
                                    let _ = pending_tasks.insert(key.clone(), bytes);
                                }

                                info!("BackgroundTaskQueue: recovering task {} (retry: {})",
                                    task.conversation_id, task.retry_count);
                                if let Err(e) = self.sender.send(task) {
                                    error!("BackgroundTaskQueue: failed to recover task (channel closed): {}", e);
                                    break;
                                }
                                recovered_count += 1;
                            }
                            Err(e) => {
                                warn!("BackgroundTaskQueue: failed to deserialize task {}: {}", key_str, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("BackgroundTaskQueue: error iterating pending_tasks: {}", e);
                }
            }
        }

        info!("BackgroundTaskQueue: recovered {} pending tasks", recovered_count);
    }

    async fn worker_with_state(
        mut rx: mpsc::UnboundedReceiver<ExtractionTask>,
        state: Arc<WorkerState>,
    ) {
        while let Some(task) = rx.recv().await {
            let cid = task.conversation_id.clone();
            let state_clone = state.clone();
            let task_clone = task;
            let result = tokio::task::spawn_blocking(move || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("failed to create runtime for task processing");
                    rt.block_on(Self::process_task(&state_clone, task_clone));
                }))
            }).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    error!("BackgroundTaskQueue: task {} processing panicked: {:?}", cid, e);
                    let state_clone = state.clone();
                    let cid_clone = cid.clone();
                    tokio::spawn(async move {
                        let _ = state_clone.dictionary.update_conversation_status(&cid_clone, "failed", Some(&format!("task panicked: {:?}", e))).await;
                        Self::remove_from_sled(&state_clone.pending_tasks, &cid_clone).await;
                    });
                }
                Err(join_err) => {
                    error!("BackgroundTaskQueue: task {} join error: {:?}", cid, join_err);
                    let state_clone = state.clone();
                    let cid_clone = cid.clone();
                    tokio::spawn(async move {
                        let _ = state_clone.dictionary.update_conversation_status(&cid_clone, "failed", Some("task join error")).await;
                        Self::remove_from_sled(&state_clone.pending_tasks, &cid_clone).await;
                    });
                }
            }
        }
        info!("BackgroundTaskQueue worker shutting down (channel closed)");
    }

    async fn worker(mut rx: mpsc::UnboundedReceiver<ExtractionTask>) {
        while let Some(_task) = rx.recv().await {
            warn!("BackgroundTaskQueue: task received but no stores configured -- dropping");
        }
        info!("BackgroundTaskQueue worker (no-stores) shutting down");
    }

    async fn process_task(state: &WorkerState, task: ExtractionTask) {
        let cid = task.conversation_id.clone();

        if let Err(e) = state.dictionary.update_conversation_status(&cid, "processing", None).await {
            error!("Failed to update status to processing for {}: {}", cid, e);
        }

        info!(
            "BackgroundTaskQueue: processing extraction for {} ({} chars, retry: {})",
            cid,
            task.conversation_text.len(),
            task.retry_count
        );

        // 把"对话发生时间"送进模型。
        //
        // 系统提示词是静态的，模型不知道这段对话是哪天说的，于是"下周三"只能靠猜：
        // 实测 2026-06-10 说的"下周三"被填成当天、2026-09-21 说的"11 月 12 号"
        // 被填成 2027-11-12（P1-1）。提示词里不给日期，靠事后校验是修不干净
        // ——模型给的日期本身自洽，只是基准错了。
        let ref_time = task
            .conversation_time
            .unwrap_or_else(chrono::Utc::now);
        let weekday_cn = ["一", "二", "三", "四", "五", "六", "日"]
            [(chrono::Datelike::weekday(&ref_time).num_days_from_monday() as usize) % 7];
        let time_hint = format!(
            "【对话发生时间】{}（周{}）\n\
             所有相对时间（今天/明天/本周X/下周X/下下周X/月底/周末）都以该时间为基准换算，\
             不要使用其他日期。\n\n",
            ref_time.format("%Y-%m-%d %H:%M"),
            weekday_cn
        );
        let messages = vec![crate::core::Message::new(
            crate::core::message::MessageRole::User,
            format!("{}{}", time_hint, task.conversation_text),
        ).unwrap()];

        // 病态输入直接跳过 LLM，走正则兜底。
        //
        // 背景：llama.cpp 的预分词用 MSVC `std::regex`，遇到"超长无标点串/纯数字/
        // 超长重复字符"时会抛 `regex_error(error_stack)`；该 C++ 异常跨 FFI 抛回
        // Rust 无法捕获，会直接 abort 整个进程（200 条压测中复现 4 次，
        // 每次中断 50+ 条排队任务）。这类输入本身也不含实体/关系信息，
        // 用正则兜底完全够用，没必要冒着崩服务的风险送进模型。
        let skip_llm = is_pathological_input(&task.conversation_text);

        let extracted = match tokio::time::timeout(
            std::time::Duration::from_secs(state.extraction_timeout_secs),
            async {
                if skip_llm {
                    return Err(ShouxinError::InvalidArgument(
                        "病态输入（超长无标点/纯数字/超长重复），已跳过 LLM".to_string(),
                    ));
                }
                state.extractor.extract(&messages).await
            },
        ).await {
            Ok(Ok(elements)) => elements,
            Ok(Err(e)) => {
                warn!("BackgroundTaskQueue: LLM extraction failed for {}: {}", cid, e);
                warn!("BackgroundTaskQueue: falling back to regex extraction for {}", cid);
                match state.extractor.fallback_extract(&task.conversation_text) {
                    Ok(fallback_elements) => {
                        info!("BackgroundTaskQueue: fallback extraction succeeded for {} ({} people, {} events)",
                            cid, fallback_elements.people.len(), fallback_elements.events.len());
                        fallback_elements
                    }
                    Err(fallback_err) => {
                        error!("BackgroundTaskQueue: fallback extraction also failed for {}: {}", cid, fallback_err);
                        let _ = state.dictionary.update_conversation_status(&cid, "failed", Some(&format!("LLM: {}, fallback: {}", e, fallback_err))).await;
                        Self::remove_from_sled(&state.pending_tasks, &cid).await;
                        return;
                    }
                }
            }
            Err(_elapsed) => {
                warn!("BackgroundTaskQueue: extraction timed out ({}s) for {}, falling back to regex", state.extraction_timeout_secs, cid);
                match state.extractor.fallback_extract(&task.conversation_text) {
                    Ok(fallback_elements) => {
                        info!("BackgroundTaskQueue: fallback extraction (after timeout) succeeded for {} ({} people, {} events)",
                            cid, fallback_elements.people.len(), fallback_elements.events.len());
                        fallback_elements
                    }
                    Err(fallback_err) => {
                        error!("BackgroundTaskQueue: fallback extraction after timeout also failed for {}: {}", cid, fallback_err);
                        let timed_out_msg = format!(
                            "extraction timed out ({}s), fallback also failed",
                            state.extraction_timeout_secs
                        );
                        let _ = state.dictionary.update_conversation_status(&cid, "failed",
                            Some(&timed_out_msg)).await;
                        Self::remove_from_sled(&state.pending_tasks, &cid).await;
                        return;
                    }
                }
            }
        };

        if let Err(e) = Self::write_extraction_results(state, &task, &extracted).await {
            error!("BackgroundTaskQueue: write_results failed for {}: {}", cid, e);
            let _ = state.dictionary.update_conversation_status(&cid, "failed", Some(&e.to_string())).await;
            Self::remove_from_sled(&state.pending_tasks, &cid).await;
            return;
        }

        let _ = state.dictionary.update_conversation_status(&cid, "completed", None).await;
        Self::remove_from_sled(&state.pending_tasks, &cid).await;
        info!("BackgroundTaskQueue: extraction completed for {}", cid);
    }

    async fn remove_from_sled(pending_tasks: &sled::Tree, conversation_id: &str) {
        let key = format!("{}{}", TASK_KEY_PREFIX, conversation_id);
        if let Err(e) = pending_tasks.remove(key.as_bytes()) {
            warn!("BackgroundTaskQueue: failed to remove task {} from sled: {}", conversation_id, e);
        }
        if let Err(e) = pending_tasks.flush() {
            warn!("BackgroundTaskQueue: failed to flush after remove: {}", e);
        }
    }

    pub fn push_with_persist(&self, task: ExtractionTask) -> Result<(), ShouxinError> {
        if let Some(ref pending_tasks) = self.pending_tasks {
            let key = Self::task_key(&task.conversation_id);
            let value = Self::serialize_task(&task)?;
            pending_tasks.insert(key.as_bytes(), value.as_slice())
                .map_err(|e| ShouxinError::StorageError(format!("Failed to persist task: {}", e)))?;
            pending_tasks.flush()
                .map_err(|e| ShouxinError::StorageError(format!("Failed to flush task: {}", e)))?;
        }

        self.push(task)?;
        Ok(())
    }

    /// 待提取任务的 sled key（与字典库原子写入共用）
    pub fn task_key(conversation_id: &str) -> String {
        format!("{}{}", TASK_KEY_PREFIX, conversation_id)
    }

    /// 序列化待提取任务（与字典库原子写入共用）
    pub fn serialize_task(task: &ExtractionTask) -> Result<Vec<u8>, ShouxinError> {
        serde_json::to_vec(task)
            .map_err(|e| ShouxinError::StorageError(format!("Failed to serialize task: {}", e)))
    }

    /// 仅投递到内存队列（任务已由字典库原子持久化）
    pub fn push_persisted(&self, task: ExtractionTask) -> Result<(), ShouxinError> {
        self.push(task)
    }

    /// 写入提取结果到三库（带跨库提交意图）
    ///
    /// 两阶段提交的第一阶段：先落 `prepared` 意图，成功后置 `committed`，
    /// 失败置 `aborted`。进程崩溃留下的 `prepared` 意图由启动扫描处理
    /// （见 `recover_stale_write_intents`）。
    ///
    /// 注意：当前只做"状态机 + 可观测"，**补偿删除**（按 intent 里记录的
    /// 事件/向量 ID 回滚派生数据）尚在下一阶段实现。
    async fn write_extraction_results(
        state: &WorkerState,
        task: &ExtractionTask,
        elements: &ExtractedElements,
    ) -> Result<(), ShouxinError> {
        use crate::stores::dictionary::{WriteIntent, WriteIntentState};

        let intent = WriteIntent::prepared(
            task.conversation_id.clone(),
            format!("{:?}", task.owner_type),
            task.owner_id.clone(),
        );
        state.dictionary.begin_write_intent(&intent).await?;

        let mut tracker = WriteTracker::default();
        match Self::write_extraction_results_inner(state, task, elements, &mut tracker).await {
            Ok(()) => {
                state
                    .dictionary
                    .finish_write_intent(&task.conversation_id, WriteIntentState::Committed, None)
                    .await?;
                Ok(())
            }
            Err(e) => {
                // 补偿：回滚本次尝试新建的派生数据（关系库 → 向量库）
                Self::compensate_write(state, &tracker).await;

                if let Err(mark_err) = state
                    .dictionary
                    .finish_write_intent(
                        &task.conversation_id,
                        WriteIntentState::Aborted,
                        Some(&e.to_string()),
                    )
                    .await
                {
                    error!(
                        "BackgroundTaskQueue: failed to mark write intent aborted for {}: {}",
                        task.conversation_id, mark_err
                    );
                }
                Err(e)
            }
        }
    }

    /// 补偿清理：回滚一次失败的跨库写入所产生的派生数据
    ///
    /// 只删派生库（关系库 / 向量库），**不动物典库**——原文与事件历史是权威存档，
    /// 保留以便重放。补偿失败只记日志，不掩盖原始错误。
    async fn compensate_write(state: &WorkerState, tracker: &WriteTracker) {
        for event_id in &tracker.event_ids {
            match state.relational.delete_event_by_id(event_id).await {
                Ok(0) => warn!(
                    "BackgroundTaskQueue: compensate: event {} already absent",
                    event_id
                ),
                Ok(_) => info!("BackgroundTaskQueue: compensate: deleted event {}", event_id),
                Err(e) => warn!(
                    "BackgroundTaskQueue: compensate: failed to delete event {}: {}",
                    event_id, e
                ),
            }
        }

        for merge_id in &tracker.merge_ids {
            match state
                .relational
                .delete_pending_event_merge_by_id(*merge_id)
                .await
            {
                Ok(_) => info!(
                    "BackgroundTaskQueue: compensate: deleted pending merge {}",
                    merge_id
                ),
                Err(e) => warn!(
                    "BackgroundTaskQueue: compensate: failed to delete pending merge {}: {}",
                    merge_id, e
                ),
            }
        }

        if !tracker.memory_ids.is_empty() {
            match state.vector.delete_batch(tracker.memory_ids.clone()).await {
                Ok(()) => info!(
                    "BackgroundTaskQueue: compensate: deleted {} vector memories",
                    tracker.memory_ids.len()
                ),
                Err(e) => warn!(
                    "BackgroundTaskQueue: compensate: failed to delete {} vector memories: {}",
                    tracker.memory_ids.len(),
                    e
                ),
            }
        }
    }

    async fn write_extraction_results_inner(
        state: &WorkerState,
        task: &ExtractionTask,
        elements: &ExtractedElements,
        tracker: &mut WriteTracker,
    ) -> Result<(), ShouxinError> {
        use chrono::Utc;
        use serde_json;
        use std::collections::HashMap;

        let persons = &elements.persons;
        let events = &elements.events;
        let learnings = &elements.learnings;
        let owner_type = task.owner_type;
        let owner_id = &task.owner_id;

        // 对话原文也入向量库。
        //
        // 之前只有"事件"进向量库，语义召回看不到没有抽取出事件的对话
        // （纯闲聊、抽取漏召回），于是"换一种问法就找不到"。实测 12 个自然语言问题里
        // 有 4 个因此失败。这里给原文补一条 embedding，类型记为 conversation，
        // 不计入素材统计（COUNTED_MATERIAL_TYPES 不含它）。
        if let Ok(emb) = state.embedder.embed(&task.conversation_text).await {
            let mut md = HashMap::new();
            md.insert(
                "material_type".to_string(),
                serde_json::Value::String("conversation".to_string()),
            );
            md.insert(
                "source_conversation_id".to_string(),
                serde_json::Value::String(task.conversation_id.clone()),
            );
            md.insert(
                "owner_type".to_string(),
                serde_json::Value::String(format!("{:?}", owner_type)),
            );
            md.insert(
                "owner_id".to_string(),
                serde_json::Value::String(owner_id.clone()),
            );
            let conv_memory = crate::core::Memory {
                id: format!("conversation:{}", task.conversation_id),
                user_id: task.user_id.clone(),
                agent_id: None,
                content: task.conversation_text.clone(),
                embedding: Some(emb),
                metadata: md,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                memory_type: crate::core::MemoryType::LongTerm,
                state: crate::core::MemoryState::Active,
                hash: None,
                score: 0.5,
            };
            // 写失败不阻断抽取主流程：召回质量下降比整条链路失败更可接受
            if let Err(e) = state.vector.insert(conv_memory).await {
                tracing::warn!("[write_results] 对话原文入向量库失败: {}", e);
            }
        }

        // 统一写入层：联系人写入（字典库权威 + 关系库索引 + 别名 + 活跃度）
        //
        // 这里不再自己拼 ContactIndex（旧实现硬编码 aliases="[]" 等，会把已有
        // 别名/关系标签清空）。所有写入语义集中在 `MemoryWriter`，同步路径共用。
        let writer = crate::memory::MemoryWriter::new(
            state.dictionary.clone(),
            state.relational.clone(),
        );
        let contact_outcome = writer.write_contacts(persons, owner_type, owner_id).await?;

        // 保留原变量名，事件保存处仍按它做 person_id → final_contact_id 映射
        let person_id_to_contact_id: HashMap<String, String> = contact_outcome.id_map;
        // 命中虚拟角色的人：不进 contact_ids，改走 event_character_link
        let character_id_map = contact_outcome.character_id_map;

        if contact_outcome.skipped_virtual > 0 {
            tracing::info!(
                "[write_results] skipped {} virtual contacts",
                contact_outcome.skipped_virtual
            );
        }


        for e in events {
            // 去重键用"标题 + 描述"：只用标题会把同标题不同内容的事件静默丢弃
            // （同步路径已经这么做，这里对齐）。
            let dedup_key: String = format!(
                "{} {}",
                e.title.as_deref().unwrap_or(""),
                e.description
            )
            .chars()
            .take(200)
            .collect();
            let existing = state.relational
                .search_events_by_summary(&dedup_key, &format!("{:?}", owner_type), owner_id)
                .await?;
            if !existing.is_empty() {
                continue;
            }

            // P1-7: 语义合并检查 (与同步路径 three_library_manager.rs 一致)
            // ⑤ description 常常以 title 开头，直接拼会把标题说两遍，抬高短文本相似度
            let event_summary_for_merge = {
                let title = e.title.as_deref().unwrap_or("").trim();
                let desc = e.description.as_str().trim();
                if !title.is_empty() && desc.starts_with(title) {
                    desc.to_string()
                } else {
                    format!("{} {}", title, desc)
                }
            };

            // P1-7: 生成 embedding 用于语义合并检查；失败时跳过合并检查，仍创建新事件
            let embedding_opt = match state.embedder.embed(&event_summary_for_merge).await {
                Ok(emb) => Some(emb),
                Err(err) => {
                    warn!("BackgroundTaskQueue: failed to embed event for merge check: {}", err);
                    None
                }
            };

            // P1-7: 只有在 embedding 成功时才进行语义合并检查
            // ⑥ 待确认合并的目标（目标事件 id / 内容 / 相似度），事件落库后再登记
            let mut defer_pending_merge: Option<(String, String, f32)> = None;
            if let Some(ref embedding) = embedding_opt {
                let mut best_score = 0.0f32;
                let mut best_target_event_id = String::new();
                let mut best_target_content = String::new();

                match state.vector.search(embedding, 5, None).await {
                    Ok(results) => {
                        let owner_type_str = format!("{:?}", owner_type);
                        for result in &results {
                            let mat_type = result.memory.metadata.get("material_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if mat_type != "event" {
                                continue;
                            }
                            let res_owner_type = result.memory.metadata.get("owner_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let res_owner_id = result.memory.metadata.get("owner_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if res_owner_type != owner_type_str || res_owner_id != owner_id {
                                continue;
                            }
                            best_score = result.score;
                            best_target_event_id = result.memory.metadata.get("event_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&result.memory.id)
                                .to_string();
                            best_target_content = result.memory.content.clone();
                            break;
                        }
                    }
                    Err(err) => {
                        warn!("BackgroundTaskQueue: semantic search failed: {}", err);
                    }
                }

                let (auto_th, pending_th) = merge_thresholds();
                let (auto_th, pending_th) = (auto_th as f32, pending_th as f32);
                if best_score > pending_th {
                    // 检查目标事件在 SQLite 中是否存在（防止向量库孤儿记忆导致合并静默失败）
                    //
                    // 还要排除 is_merged=1 的事件：被合并掉的源事件仍留在向量库里，
                    // 而它"本来就跟新事件很像"，极容易被再次选成目标。
                    // 一旦选中，新内容会追加到一条**已对用户隐藏**的事件上，等于丢记忆。
                    let target = state.relational
                        .get_event_by_id(&best_target_event_id).await
                        .ok()
                        .flatten();
                    let target_exists = target.as_ref().map(|ev| !ev.is_merged).unwrap_or(false);

                    if !target_exists {
                        warn!(
                            "[P1-7] 合并目标事件 {} 不可用（不存在或已被合并掉），跳过合并，创建新事件。来源: '{}'",
                            best_target_event_id,
                            event_summary_for_merge.chars().take(30).collect::<String>()
                        );
                        // 不 continue，继续走创建新事件路径
                    } else if best_score > auto_th {
                        // 自动合并到已有事件
                        let contact_ids: Vec<String> = e.related_person_ids.iter()
                            .filter(|pid| !character_id_map.contains_key(*pid))
                            .filter_map(|pid| person_id_to_contact_id.get(pid).cloned())
                            .collect();
                        let merged_character_ids: Vec<String> = e.related_person_ids.iter()
                            .filter_map(|pid| character_id_map.get(pid).cloned())
                            .collect();
                        let _ = state
                            .relational
                            .virtual_alias_store()
                            .link_event_characters(&best_target_event_id, &merged_character_ids)
                            .await;
                        match state.relational.update_event_after_merge(
                            &best_target_event_id,
                            &contact_ids,
                            Some(&event_summary_for_merge),
                        ).await {
                            Ok(0) => {
                                // 目标事件在关系库不存在（向量库孤儿记忆）：
                                // 不能算成功，否则源事件会被静默吞掉
                                warn!(
                                    "[P1-7] 自动合并未生效，目标事件 {} 不存在，回退创建新事件",
                                    best_target_event_id
                                );
                            }
                            Ok(_) => {
                                info!(
                                    "[P1-7] 自动合并事件 '{}' -> '{}' (相似度: {:.4})",
                                    event_summary_for_merge.chars().take(30).collect::<String>(),
                                    best_target_content.chars().take(30).collect::<String>(),
                                    best_score
                                );
                                // F-M2: 合并后仍要把源摘要写入字典库的联系人事件历史，
                                // 保证"字典库永不丢原始数据"，即便关系库只剩合并后的目标事件。
                                let merged_contact_ids: Vec<String> = e.related_person_ids.iter()
                                    .map(|pid| person_id_to_contact_id.get(pid).cloned().unwrap_or_else(|| pid.clone()))
                                    .collect();
                                persist_contact_event_history(
                                    &state.dictionary,
                                    &best_target_event_id,
                                    &merged_contact_ids,
                                    &event_summary_for_merge,
                                    Utc::now(),
                                    e.importance,
                                    owner_type,
                                    owner_id,
                                    &task.conversation_id,
                                ).await?;
                                continue;
                            }
                            Err(err) => {
                                // 合并失败不能丢源事件：回退到创建新事件
                                warn!("[P1-7] 自动合并失败，回退创建新事件: {}", err);
                            }
                        }
                    } else {
                        // ⑥ 进待确认区：先标记，等事件真正落库后再登记。
                        // 这里不能 continue——continue 会让源事件永不写进 event_index，
                        // 用户点"不合并"就等于永久丢弃这条记忆。
                        defer_pending_merge = Some((
                            best_target_event_id.clone(),
                            best_target_content.clone(),
                            best_score,
                        ));
                    }
                }
            }

            // 创建新事件
            let llm_description = if e.description.trim().is_empty() {
                String::new()
            } else {
                e.description.clone()
            };


            let mut contact_ids: Vec<String> = e.related_person_ids.iter()
                    .filter(|pid| !character_id_map.contains_key(*pid))
                    .filter_map(|pid| person_id_to_contact_id.get(pid).cloned())
                    .collect();
            let event_character_ids: Vec<String> = e.related_person_ids.iter()
                    .filter_map(|pid| character_id_map.get(pid).cloned())
                    .collect();

            // 如果 contact_ids 为空，尝试从事件文本中匹配已有联系人
            //
            // 注意：这里**不能**用 `search_contacts_by_name("")` —— 它的 SQL 是 `name = ''`，
            // 永远查不到人，所以这段兜底长期是死代码。实测 13 个事件只有 2 个带 contact_ids，
            // 直接后果是画像的 major_events 永远为空、按联系人查事件查不到。
            if contact_ids.is_empty() {
                let event_text = format!("{} {}", e.title.as_deref().unwrap_or(""), e.description);
                if let Ok(existing_contacts) = state
                    .relational
                    .get_active_contacts(owner_type, owner_id, 50)
                    .await
                {
                    contact_ids = contacts_mentioned_in(&event_text, &existing_contacts);
                }
            }

            let contact_ids_json = serde_json::to_string(&contact_ids).unwrap_or("[]".to_string());
            // 时间优先级：time_info → time 字段 → 事件描述 → **对话原文**。
            //
            // 最后一层兜底是因为小模型经常把时间词从描述里删掉
            // （原文"上周三……季度复盘"，描述写成"负责数据部分"），
            // 事件时间就会退化成写入时间。对话里通常只有一个时间表达，
            // 用它兜底比"全部变成今天"更接近事实。
            // 时间基准取"对话发生时间"，不是"抽取时刻"：历史导入时两者可能差几个月。
            let ref_time = task.conversation_time.unwrap_or_else(Utc::now);
            let event_start = crate::utils::try_resolve_event_time(
                e.time_info.as_deref(),
                e.time.as_deref(),
                &e.description,
                ref_time,
            )
            .or_else(|| crate::utils::parse_event_time(&task.conversation_text, ref_time))
            .unwrap_or(ref_time);
            let mut event_index = crate::core::EventIndex {
                event_id: e.event_id.clone(),
                contact_ids: contact_ids_json,
                owner_id: owner_id.clone(),
                owner_type,
                time_range_start: event_start,
                time_range_end: event_start,
                importance: if e.importance >= 0.8 { "high".to_string() } else if e.importance >= 0.5 { "medium".to_string() } else { "low".to_string() },
                status: "pending".to_string(),
                created_at: Utc::now(),
                completed_at: None,
                // summary 存"描述"，不是标题副本；event_type 留空（事件没有类型维度）
                summary: e.description.chars().take(200).collect(),
                title: e.title.clone(),
                event_type: String::new(),
                deadline: None,
                auto_invalid_at: None,
                is_auto_invalid: false,
                is_merged: false,
                merge_target_id: None,
                last_merge_check_at: None,
                event_time: Some(event_start),
                importance_score: e.importance as f32,
                is_archived: false,
                archived_at: None,
            };
            event_index.compute_auto_invalid_at();
            state.relational.insert_event_index(&event_index).await?;
            if !event_character_ids.is_empty() {
                let _ = state
                    .relational
                    .virtual_alias_store()
                    .link_event_characters(&event_index.event_id, &event_character_ids)
                    .await;
            }

            // ⑥ 源事件已落库，用真实 event_id 登记待确认合并
            if let Some((target_id, target_content, score)) = defer_pending_merge.take() {
                match state.relational.insert_pending_event_merge(
                    &event_index.event_id,
                    Some(&target_id),
                    &event_summary_for_merge,
                    Some(&target_content),
                    score,
                    owner_type,
                    owner_id,
                ).await {
                    Ok(merge_id) => {
                        tracker.merge_ids.push(merge_id);
                        let (a_th, p_th) = merge_thresholds();
                        info!(
                            "[P1-7] 待确认合并: '{}' -> '{}' 相似度 {:.4} (阈值 {:.2}-{:.2})",
                            event_summary_for_merge.chars().take(30).collect::<String>(),
                            target_content.chars().take(30).collect::<String>(),
                            score,
                            p_th,
                            a_th
                        );
                    }
                    Err(err) => {
                        // 事件已经落库，登记失败只告警，不能丢事件
                        warn!("[P1-7] 插入待确认合并失败（事件已保留）: {}", err);
                    }
                }
            }

            tracker.event_ids.push(e.event_id.clone());

            // P1-3/P1-7: 将事件写入向量库（复用已生成的 embedding；embedding 失败时跳过）
            if let Some(embedding) = embedding_opt {
                let event_content = format!("事件: {} - {}",
                    e.title.as_deref().unwrap_or("无标题"),
                    e.description.as_str()
                );
                let mut event_metadata = HashMap::new();
                event_metadata.insert("source_conversation_id".to_string(),
                    serde_json::Value::String(task.conversation_id.clone()));
                event_metadata.insert("material_type".to_string(),
                    serde_json::Value::String("event".to_string()));
                event_metadata.insert("event_id".to_string(),
                    serde_json::Value::String(e.event_id.clone()));
                event_metadata.insert("owner_type".to_string(),
                    serde_json::Value::String(format!("{:?}", owner_type)));
                event_metadata.insert("owner_id".to_string(),
                    serde_json::Value::String(task.owner_id.clone()));

                let event_memory = crate::core::Memory {
                    id: uuid::Uuid::new_v4().to_string(),
                    user_id: task.user_id.clone(),
                    agent_id: None,
                    content: event_content,
                    embedding: Some(embedding),
                    metadata: event_metadata,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    memory_type: crate::core::MemoryType::LongTerm,
                    state: crate::core::MemoryState::Active,
                    hash: None,
                    score: e.importance as f32,
                };
                let event_memory_id = event_memory.id.clone();

                // 强事务：向量写失败不能吞掉，否则关系库/向量库会静默不一致。
                // 失败后由 compensate_write 回滚本事件的关系库行。
                state.vector.insert(event_memory).await?;
                tracker.memory_ids.push(event_memory_id);
            }

            // F-M2: 写入字典库的联系人事件历史（权威存档）
            //
            // 顺序约定：字典库是 append-only、不可回滚，因此**放在最后写**——
            // 只有关系库与向量库都写过之后才落字典，避免"关系库/向量库回滚了，
            // 字典库却留下指向不存在事件的历史记录"。这是跨库两阶段提交的前置条件。
            persist_contact_event_history(
                &state.dictionary,
                &e.event_id,
                &contact_ids,
                &dedup_key,
                event_start,
                e.importance,
                owner_type,
                owner_id,
                &task.conversation_id,
            ).await?;
        }

        // Process todo update intent
        if let Some(ref intent) = elements.todo_update_intent {
            if let Some(ref title_hint) = intent.title_hint {
                let owner_type_str = format!("{:?}", task.owner_type);
                match state.relational.get_todo_store().search_pending_todos_by_title_fuzzy(
                    title_hint,
                    &owner_type_str,
                    &task.owner_id,
                ).await {
                    Ok(todos) => {
                        if todos.len() == 1 {
                            let todo = &todos[0];
                            if let Some(ref new_deadline) = intent.new_deadline {
                                match state.relational.get_todo_store().update_todo_deadline(
                                    &todo.todo_id,
                                    new_deadline,
                                ).await {
                                    Ok(()) => {
                                        tracing::info!(
                                            "Updated todo '{}' deadline to {} based on conversation intent",
                                            todo.title,
                                            new_deadline
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to update todo deadline: {}", e);
                                    }
                                }
                            }
                        } else if todos.is_empty() {
                            tracing::warn!(
                                "Todo update intent: no pending todo matching '{}'",
                                title_hint
                            );
                        } else {
                            tracing::warn!(
                                "Todo update intent: multiple ({}) pending todos match '{}', skipping",
                                todos.len(),
                                title_hint
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Todo fuzzy search failed: {}", e);
                    }
                }
            }
        }

        // P1-4: Process extracted todos (new待办事项 from conversation)
        if !elements.todos.is_empty() {
            info!("[P1-4] Processing {} extracted todos for {}", elements.todos.len(), task.conversation_id);
            let owner_type_str = format!("{:?}", task.owner_type);
            for extracted_todo in &elements.todos {
                let todo_id = uuid::Uuid::new_v4().to_string();

                // 截止时间解析（可靠性优先）：
                // 1) 模型给的 ISO 日期若**已过期**（早于当前时间），视为小模型幻觉，丢弃；
                // 2) 回到对话原文，用本地 `time_parser` 解析自然语言相对时间
                //    （例："下周一之前要完成" → 2026-11-02），比依赖小模型可靠；
                // 3) 仍取不到 → 默认 7 天后当天 23:59:59。
                // P1-4: 使用当天 23:59:59 而非精确时间戳，确保同一天内多次提取的默认 deadline 一致，使去重索引生效
                let model_deadline = extracted_todo
                    .deadline
                    .as_ref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc));

                let ref_time = task.conversation_time.unwrap_or_else(Utc::now);
                let ref_day_start = chrono::DateTime::<Utc>::from_naive_utc_and_offset(
                    ref_time.date_naive().and_hms_opt(0, 0, 0).unwrap(),
                    Utc,
                );

                // 模型给的日期只在"相对对话时间合理"时采信：
                //  - 不早于对话发生日（模型常把年份写成今年，或给一个早已过去的日期）；
                //  - 不超过两年（原来是 5 年，几乎等于不设限，
                //    实测 2026-09-21 说的"11 月 12 号"被填成 2027-11-12）；
                //  - 统一成当天 23:59:59，与本地解析口径一致（模型给 00:00:00Z 会让
                //    待办在截止日凌晨就过期）。
                let model_deadline = extracted_todo
                    .deadline
                    .as_ref()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .filter(|dt| *dt >= ref_day_start)
                    .filter(|dt| *dt < ref_time + chrono::Duration::days(730))
                    .map(crate::utils::end_of_day);

                let local = crate::utils::resolve_deadline_ex(
                    &task.conversation_text,
                    &extracted_todo.title,
                    ref_time,
                );

                // 交叉校验：两边都可能错，但错法不同。
                //  - 本地命中"显式截止"（X 前 / 最后期限）却与模型差 >1 天 → 信本地，
                //    模型读不懂中文相对时间，这类句式它必错；
                //  - 模型原样返回对话当天，而原文里有未来时间词 → 视为模型失败；
                //  - 其余情况以模型为准（它能读懂上下文，本地只会取"第一个时间"）。
                let deadline = match (model_deadline, local) {
                    (Some(m), Some(l))
                        if l.explicit
                            && (m.date_naive() - l.at.date_naive()).num_days().abs() > 1 =>
                    {
                        warn!(
                            "[deadline] 模型 {} 与本地显式截止 {} 相差过大，采用本地：{}",
                            m.date_naive(),
                            l.at.date_naive(),
                            extracted_todo.title
                        );
                        l.at
                    }
                    (Some(m), Some(l)) if m.date_naive() == ref_time.date_naive() => {
                        warn!(
                            "[deadline] 模型返回对话当天 {}，改用本地解析 {}：{}",
                            m.date_naive(),
                            l.at.date_naive(),
                            extracted_todo.title
                        );
                        l.at
                    }
                    (Some(m), _) => m,
                    (None, Some(l)) => {
                        warn!(
                            "[deadline] 模型未给可用日期，改用本地解析：{} → {}（explicit={}）",
                            extracted_todo.title,
                            l.at.date_naive(),
                            l.explicit
                        );
                        l.at
                    }
                    (None, None) => {
                        // 一点时间线索都没有：这条是猜的，必须留痕，
                        // 否则用户会把"11-08"当成自己说过的话（N-14）。
                        let guessed = crate::utils::default_deadline_from(ref_time, 7);
                        warn!(
                            "[deadline] 对话中没有可识别的时间表达，按 +7 天估算：{} → {}（conversation={}）",
                            extracted_todo.title,
                            guessed.date_naive(),
                            task.conversation_id
                        );
                        guessed
                    }
                };

                // Validate importance: must be one of high/medium/low/short_term/long_term/critical
                let valid_importance = ["high", "medium", "low", "short_term", "long_term", "critical"];
                let importance = if valid_importance.contains(&extracted_todo.importance.as_str()) {
                    extracted_todo.importance.clone()
                } else {
                    warn!("[P1-4] Invalid importance '{}', defaulting to 'medium'", extracted_todo.importance);
                    "medium".to_string()
                };

                // 把 assigned_to 映射成**落库后**的联系人 id
                //
                // 原实现直接沿用抽取阶段的 `p.contact_id`（合并/去重前的临时 id），
                // 实测 50% 的待办挂着悬空 id：/api/contacts/:id/detail 查不到关联待办。
                // 这里先过 person_id -> final_contact_id 映射，再按姓名回查权威 id。
                let contact_ids: Vec<String> = if let Some(ref assigned_to) = extracted_todo.assigned_to {
                    let mut ids: Vec<String> = elements
                        .persons
                        .iter()
                        .filter(|p| p.name == *assigned_to)
                        .filter_map(|p| person_id_to_contact_id.get(&p.contact_id).cloned())
                        .collect();
                    if ids.is_empty() {
                        if let Ok(list) = state
                            .relational
                            .get_active_contacts(task.owner_type, &task.owner_id, 200)
                            .await
                        {
                            ids = list
                                .iter()
                                .filter(|c| c.name == *assigned_to)
                                .map(|c| c.id.clone())
                                .collect();
                        }
                    }
                    ids
                } else {
                    Vec::new()
                };

                // 同一句话重复抽取会产出措辞微差的待办（"把…改成" / "将…改成"），
                // 插入前按"同 owner + pending + 标题相似"挡掉（N-16）。
                if let Ok(existing) = state
                    .relational
                    .get_todo_store()
                    .get_all_todos(task.owner_type, &task.owner_id, Some("pending"), 200, false, None, None)
                    .await
                {
                    let new_norm = normalize_todo_title(&extracted_todo.title);
                    if let Some(dup) = existing
                        .iter()
                        .find(|t| todo_titles_similar(&normalize_todo_title(&t.title), &new_norm))
                    {
                        tracing::info!(
                            "[todo] 跳过重复待办：'{}' ≈ 已有 '{}'（conversation={}）",
                            extracted_todo.title,
                            dup.title,
                            task.conversation_id
                        );
                        continue;
                    }
                }

                let todo = crate::stores::relational::TodoItem::new(
                    todo_id,
                    contact_ids,
                    task.owner_id.clone(),
                    task.owner_type,
                    extracted_todo.title.clone(),
                    deadline,
                    importance,
                );

                match state.relational.get_todo_store().add_todo(&todo).await {
                    Ok(()) => {
                        info!("[P1-4] Created todo '{}' for {}/{}", todo.title, owner_type_str, task.owner_id);
                    }
                    Err(ShouxinError::ConflictError(msg)) => {
                        info!("[P1-4] Todo already exists (dedup): {}", msg);
                    }
                    Err(e) => {
                        warn!("[P1-4] Failed to create todo '{}': {}", todo.title, e);
                    }
                }
            }
        }

        // 学习素材写入（质量过滤 + 0.98 去重 + 向量入库）统一由 MemoryWriter 负责，
        // 与同步路径共用同一实现（F-X4/F-X5 的根因是两套写法）。
        let writer = crate::memory::MemoryWriter::new(
            state.dictionary.clone(),
            state.relational.clone(),
        )
        .with_embedding(state.vector.clone(), state.embedder.clone())
        .with_config(crate::memory::MemoryWriterConfig {
            min_learning_quality: MIN_LEARNING_QUALITY,
            learning_dedup_threshold: LEARNING_DEDUP_THRESHOLD,
        });

        writer
            .write_learnings(
                learnings,
                owner_type,
                owner_id,
                task.user_id.as_deref(),
                &task.conversation_id,
                tracker,
            )
            .await?;


        // DC-04 / P1-4: 管理活跃联系人数量限制（热表仅保留 MAX_ACTIVE_CONTACTS 条）
        // get_active_contacts 按 last_interaction_at 降序返回（最近交互在前），
        // 因此 [MAX_ACTIVE_CONTACTS..] 切片取到的是最久未交互的尾部联系人，将其归档到冷表（is_archived=1），
        // 而非物理删除——这样字典库 ContactTotal 数据与关系库索引保持一致，
        // contact_alias_index 行也保留，后续若同一别名再次出现可重新识别并合并。
        let active_contacts = state.relational
            .get_active_contacts(owner_type, owner_id, i64::MAX)
            .await?;
        if active_contacts.len() > MAX_ACTIVE_CONTACTS {
            let to_archive = &active_contacts[MAX_ACTIVE_CONTACTS..];
            let archive_count = to_archive.len();
            for contact in to_archive {
                if let Err(e) = state.relational.archive_contact_by_id(&contact.id).await {
                    warn!("BackgroundTaskQueue: failed to archive excess contact {}: {}", contact.id, e);
                }
            }
            info!(
                "BackgroundTaskQueue: enforced MAX_ACTIVE_CONTACTS={}, archived {} excess contacts (moved to cold table) for {}/{}",
                MAX_ACTIVE_CONTACTS, archive_count, format!("{:?}", owner_type), owner_id
            );
        }

        // 把本次新建的派生资源登记到提交意图（供崩溃后补偿）
        if !tracker.event_ids.is_empty() || !tracker.memory_ids.is_empty() {
            if let Err(e) = state
                .dictionary
                .record_write_intent_resources(
                    &task.conversation_id,
                    &tracker.event_ids,
                    &tracker.memory_ids,
                )
                .await
            {
                warn!(
                    "BackgroundTaskQueue: failed to record intent resources for {}: {}",
                    task.conversation_id, e
                );
            }
        }

        Ok(())
    }
}

/// 将事件摘要写入字典库的联系人事件历史（权威存档）
///
/// 字典库是所有记忆的源文件：事件即便之后在关系库被归档、失效或合并，
/// 这里的历史记录也必须保留，供回溯与"联系人视角"的时间线检索。
///
/// 任一条写入失败即返回错误，由上层任务状态体现为 `failed`，
/// 避免出现"关系库有事件、字典库无历史"的静默缺口。
/// 待办标题归一化：去空白/标点、转小写，用于查重
fn normalize_todo_title(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !"，。、；：！？,.!?;:（）()【】\"'“”‘’".contains(*c))
        .collect::<String>()
        .to_lowercase()
}

/// 两条待办标题是否在说同一件事
///
/// 判定：包含关系，或字符集合 Jaccard ≥ 0.85。
/// 只差"把/将""的"这类虚词的表述会被判为重复。
fn todo_titles_similar(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b || a.contains(b) || b.contains(a) {
        return true;
    }
    let sa: std::collections::HashSet<char> = a.chars().collect();
    let sb: std::collections::HashSet<char> = b.chars().collect();
    let inter = sa.intersection(&sb).count() as f32;
    let union = sa.union(&sb).count() as f32;
    union > 0.0 && inter / union >= 0.85
}

/// 从事件文本中挑出被提到的联系人（姓名或别名逐字出现）
///
/// 用于 `related_person_ids` 没能映射出联系人时的兜底：
/// 早些时候这里调的是 `search_contacts_by_name("")`，而它的 SQL 是 `name = ''`，
/// 永远返回空，等于兜底从未生效（实测 13 个事件只有 2 个带 contact_ids）。
fn contacts_mentioned_in(text: &str, contacts: &[crate::core::ContactIndex]) -> Vec<String> {
    contacts
        .iter()
        .filter(|c| !c.is_self)
        .filter(|c| {
            if !c.name.is_empty() && text.contains(&c.name) {
                return true;
            }
            // 别名也要认（"老张"这类称呼经常出现在事件描述里）
            serde_json::from_str::<Vec<String>>(&c.aliases)
                .unwrap_or_default()
                .iter()
                .any(|a| !a.is_empty() && text.contains(a.as_str()))
        })
        .map(|c| c.id.clone())
        .collect()
}

async fn persist_contact_event_history(
    dictionary: &Arc<dyn DictionaryStoreTrait>,
    event_id: &str,
    contact_ids: &[String],
    summary: &str,
    event_time: chrono::DateTime<chrono::Utc>,
    importance_score: f32,
    owner_type: OwnerType,
    owner_id: &str,
    conversation_id: &str,
) -> Result<(), ShouxinError> {
    if contact_ids.is_empty() {
        return Ok(());
    }

    let importance = crate::utils::classify_importance(importance_score).to_string();
    let sentiment = crate::utils::analyze_sentiment(summary).to_string();

    for contact_id in contact_ids {
        let record = crate::stores::dictionary::store::ContactEventRecord::new(
            event_id.to_string(),
            contact_id.clone(),
            owner_id.to_string(),
            owner_type,
            summary.to_string(),
            event_time,
            importance.clone(),
            sentiment.clone(),
            Some(conversation_id.to_string()),
        );
        if let Err(e) = dictionary.save_contact_event(&record).await {
            error!(
                "BackgroundTaskQueue: failed to persist contact event history (event={}, contact={}): {}",
                event_id, contact_id, e
            );
            return Err(e);
        }
    }

    Ok(())
}

/// 一次写入尝试中新建的派生资源，用于失败补偿
///
/// 与 `MemoryWriter` 的 `WriteOutcome` 是同一个类型：写入层负责登记，
/// worker 负责在失败时按这些 ID 回滚。
type WriteTracker = crate::memory::WriteOutcome;

struct WorkerState {
    dictionary: Arc<dyn DictionaryStoreTrait>,
    relational: Arc<RelationalStore>,
    vector: Arc<dyn VectorStore>,
    extractor: Arc<dyn MemoryExtractor>,
    embedder: Arc<dyn Embedder>,
    #[allow(dead_code)]
    config_user_id: String,
    /// LLM 提取超时（秒），来自 [extraction].timeout_secs
    extraction_timeout_secs: u64,
    pending_tasks: sled::Tree,
}

#[cfg(test)]
mod pathological_input_tests {
    use super::is_pathological_input;

    #[test]
    fn detects_junk_inputs() {
        assert!(is_pathological_input(&"1234567890 ".repeat(100)));
        assert!(is_pathological_input(&"哈".repeat(400)));
        let long_single = "今天开会讨论了很多事情".to_string() + &"包括架构调整和人员安排".repeat(60);
        assert!(is_pathological_input(&long_single));
    }

    #[test]
    fn keeps_normal_text() {
        let normal = "老王是我的同事，在研发部做后端。小李和老王是同一个项目组。下周五之前交付接口文档。";
        assert!(!is_pathological_input(normal));

        let long_doc = "今天开会讨论了系统架构和接口设计。\n".repeat(200);
        assert!(!is_pathological_input(&long_doc));
    }
}

#[cfg(test)]
mod todo_dedup_tests {
    use super::{normalize_todo_title, todo_titles_similar};

    /// N-16：只差"把/将""的"的两条待办必须判为同一条
    #[test]
    fn test_similar_titles_with_different_function_words() {
        let a = normalize_todo_title("把 internal_api_token 默认空串改成必填");
        let b = normalize_todo_title("将 internal_api_token 的默认空串改成必填");
        assert!(todo_titles_similar(&a, &b), "应判为重复: {a} vs {b}");
    }

    /// 不相干的两条待办不能被误判
    #[test]
    fn test_different_titles_are_not_similar() {
        let a = normalize_todo_title("给用户发测试报告");
        let b = normalize_todo_title("把索引重建脚本交出来");
        assert!(!todo_titles_similar(&a, &b));
    }
}
