//! 统一记忆写入层（MemoryWriter）
//!
//! 背景：v0.5.1 异步化时，`BackgroundTaskQueue` 自己重写了一套写库逻辑，
//! 与同步路径 `ThreeLibraryMemoryManager::add` / `DataDispatcher` 各写一遍，
//! 于是同一种数据在两条路径上产生了不同的字段处理（F-M5），并派生出
//! F-M1（字典库字段被清空）、F-M2（事件历史缺失）、F-M3（owner 错位）等问题。
//!
//! 本模块把这些"写入语义"收敛成**单一实现**，两条路径都调用它：
//! - 字典库是权威：用 `ContactPatch` 增量合并，绝不用低信息量视图整条覆盖；
//! - 关系库是派生索引：从字典库返回的权威记录构建，缺失时才回退旧值；
//! - owner 必须由调用方显式传入，存储层不猜默认值。
//!
//! 后续阶段会继续把事件写入、学习素材写入迁入本模块（见
//! `TRANSACTION_WRITER_DESIGN.md` 阶段 4）。

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tracing::{debug, info, warn};

use crate::core::contact::{ContactIndex, ContactPatch, OwnerType};
use crate::error::ShouxinError;
use crate::memory_extractor::{ExtractedLearning, ExtractedPerson};
use crate::stores::relational::RelationalStore;
use crate::traits::store_traits::{DictionaryStoreTrait, RelationalStoreTrait};
use crate::traits::{Embedder, VectorStore};

/// 联系人写入结果
#[derive(Debug, Default)]
pub struct ContactWriteOutcome {
    /// LLM 临时 person_id → 去重后的最终 contact_id
    pub id_map: HashMap<String, String>,
    /// 新建的联系人数量
    pub created: usize,
    /// 更新的联系人数量
    pub updated: usize,
    /// 因命中虚拟角色而跳过的数量
    pub skipped_virtual: usize,
    /// LLM 临时 person_id → 虚拟角色 id（事件侧用它建立 event↔character 关联）
    pub character_id_map: std::collections::HashMap<String, String>,
}

/// 统一写入层
///
/// 持有字典库 + 关系库；向量库/嵌入器通过 [`MemoryWriter::with_embedding`] 注入，
/// 用于学习素材与事件写入。
pub struct MemoryWriter {
    dictionary: Arc<dyn DictionaryStoreTrait>,
    relational: Arc<RelationalStore>,
    vector: Option<Arc<dyn VectorStore>>,
    embedder: Option<Arc<dyn Embedder>>,
    config: MemoryWriterConfig,
}

/// 写入策略阈值（替代散落在两条路径里的硬编码）
#[derive(Debug, Clone)]
pub struct MemoryWriterConfig {
    /// 学习素材最低质量分
    pub min_learning_quality: f32,
    /// 学习素材重复判定阈值（余弦相似度）
    pub learning_dedup_threshold: f32,
}

impl Default for MemoryWriterConfig {
    fn default() -> Self {
        Self {
            min_learning_quality: 0.8,
            learning_dedup_threshold: 0.98,
        }
    }
}

/// 一次写入产生的派生资源与统计
///
/// 资源 ID 会被跨库提交意图记录，用于失败补偿；因此即使中途出错，
/// 已写入的部分也必须保留在 outcome 里（调用方按引用传入）。
#[derive(Debug, Default)]
pub struct WriteOutcome {
    /// 新建的事件索引 ID
    pub event_ids: Vec<String>,
    /// 新建的待确认合并记录 ID
    pub merge_ids: Vec<i64>,
    /// 新建的向量记忆 ID
    pub memory_ids: Vec<String>,
    /// 因质量不足被跳过的学习素材数
    pub skipped_low_quality: usize,
    /// 因重复被跳过的学习素材数
    pub skipped_duplicates: usize,
}

impl MemoryWriter {
    pub fn new(
        dictionary: Arc<dyn DictionaryStoreTrait>,
        relational: Arc<RelationalStore>,
    ) -> Self {
        Self {
            dictionary,
            relational,
            vector: None,
            embedder: None,
            config: MemoryWriterConfig::default(),
        }
    }

    /// 注入向量库与嵌入器（写入学习素材/事件所需）
    pub fn with_embedding(
        mut self,
        vector: Arc<dyn VectorStore>,
        embedder: Arc<dyn Embedder>,
    ) -> Self {
        self.vector = Some(vector);
        self.embedder = Some(embedder);
        self
    }

    /// 注入写入策略
    pub fn with_config(mut self, config: MemoryWriterConfig) -> Self {
        self.config = config;
        self
    }

    pub fn dictionary(&self) -> &Arc<dyn DictionaryStoreTrait> {
        &self.dictionary
    }

    pub fn relational(&self) -> &Arc<RelationalStore> {
        &self.relational
    }

    /// 写入一批联系人（字典库权威 + 关系库索引 + 别名索引 + 活跃度）
    ///
    /// 与旧实现的差异：
    /// - 用 `find_contact_for_person_with_hints` 按「姓名/别名/消歧标签/身份线索」去重；
    /// - 虚拟角色不写入真人体系（旧同步路径没有这个判断）；
    /// - 别名是"并入"，不是覆盖；
    /// - `ContactIndex` 的业务字段来自字典库权威记录，缺失时才回退关系库旧值，
    ///   不再硬编码 `"[]"` 把已有别名/关系标签清空。
    pub async fn write_contacts(
        &self,
        persons: &[ExtractedPerson],
        owner_type: OwnerType,
        owner_id: &str,
    ) -> Result<ContactWriteOutcome, ShouxinError> {
        let mut outcome = ContactWriteOutcome::default();

        for person in persons {
            debug!(
                "[MemoryWriter] contact: name='{}', aliases={:?}, contact_id={}",
                person.name, person.aliases, person.contact_id
            );

            // 最后一道闸：关系短语不是人名。
            //
            // 抽取链路的 rule_based_contact_filter 已经拦过一次，但事件里的
            // related_person_ids 会走另一条写入口，绕过那次过滤
            // （242 条压测里仍出现"调整和人""客户的工程师"这类条目）。
            // 写入层再拦一次，成本是一个字符串判断。
            if crate::memory_extractor::extractor::is_relational_phrase(&person.name) {
                debug!("[MemoryWriter] 跳过关系短语: {}", person.name);
                continue;
            }

            // 1) 去重：按「姓名（或别名） + 同名消歧标签」匹配
            //
            // 姓名**不唯一**：两个"刘洋"若分别带"前端""后端"标签，会被判成两个人，
            // 各拿一个独立 UUID；只有姓名相同且标签也相同（或双方都没标签）才归并。
            let mut existing = self
                .relational
                .find_contact_for_person_with_hints(
                    &person.name,
                    &person.disambiguation,
                    &person.aliases,
                    person.role.as_deref(),
                    owner_type,
                    owner_id,
                )
                .await?;

            // 1b) 姓名归一化兜底：模型有时把区分词并进姓名（"前端刘洋"），
            //     而库里已经有"刘洋"这个人。
            //     此时若直接入库，就会凭空多出一个联系人"前端刘洋"——
            //     实测同一条语料第二次被提及时，模型输出名字变体，联系人从 2 条涨到 3 条。
            //     规则：只有当"去掉前缀后的名字"**确实已在库中**时，才把它拆开，
            //     前缀当作消歧标签；否则保持原样（不能凭猜测改用户的名字）。
            let mut final_name = person.name.clone();
            let mut final_disambiguation = person.disambiguation.clone();
            if existing.is_none() {
                let chars: Vec<char> = person.name.chars().collect();
                if (3..=6).contains(&chars.len()) {
                    for cut in 1..=2usize {
                        let suffix: String = chars[cut..].iter().collect();
                        if suffix.chars().count() < 2 {
                            continue;
                        }
                        // 只认**姓名精确相等**：别名命中不能作为拆分依据，
                        // 否则"欧阳娜娜"这类复姓/多字名容易被误拆。
                        let owner_type_str = format!("{:?}", owner_type);
                        let same_name = self
                            .relational
                            .search_contacts_by_name(&suffix, &owner_type_str, owner_id)
                            .await
                            .unwrap_or_default();
                        if same_name.is_empty() || same_name[0].name != suffix {
                            continue;
                        }
                        let prefix: String = chars[..cut].iter().collect();
                        // 模型可能同时给了标签（"name=前端刘洋, disambiguation=前端"），
                        // 两处一致时优先用模型给的标签，不一致时也用模型给的（更可信）。
                        let disp = if !final_disambiguation.trim().is_empty() {
                            final_disambiguation.trim().to_string()
                        } else {
                            prefix.trim().to_string()
                        };
                        debug!(
                            "[MemoryWriter] 姓名归一化：'{}' → 姓名='{}'，消歧='{}'（库中已有 ID {}）",
                            person.name, suffix, disp, same_name[0].id
                        );
                        final_disambiguation = disp;
                        final_name = suffix;
                        existing = self
                            .relational
                            .find_contact_for_person_with_hints(
                                &final_name,
                                &final_disambiguation,
                                &person.aliases,
                                person.role.as_deref(),
                                owner_type,
                                owner_id,
                            )
                            .await?;
                        break;
                    }
                }
            }

            let final_contact_id = existing
                .as_ref()
                .map(|e| e.id.clone())
                .unwrap_or_else(|| person.contact_id.clone());

            // 2) 虚拟角色不进入真人联系人体系（判定要在登记 id_map 之前：
            //    角色 id 不能出现在 contact 映射里，否则事件侧会写出悬空 contact_id）
            //
            // 角色现在存在**独立的表**（virtual_character）里，这里只负责：
            // a) 不再给角色建真人联系人；
            // b) 把 person_id → character_id 记下来，事件侧据此建立 event↔character 关联
            //    （之前是把角色 id 塞进 event_index.contact_ids，拆表后那样会变成悬空 id）。
            let virtual_hit = self
                .relational
                .check_virtual_character_match(&person.name)
                .await
                .unwrap_or(None);
            if let Some(info) = virtual_hit {
                info!(
                    "[MemoryWriter] '{}' 命中虚拟角色「{}」（出处：{:?}），跳过真人建档",
                    person.name, info.character.name, info.character.source_work
                );
                outcome
                    .character_id_map
                    .insert(person.contact_id.clone(), info.character.id.clone());
                outcome.skipped_virtual += 1;
                continue;
            }

            // 3) 真人：登记映射，事件侧据此解析 contact_id
            outcome
                .id_map
                .insert(person.contact_id.clone(), final_contact_id.clone());

            // 4) 合并别名（已有 + 本次提取，去重且忽略大小写与空串）
            let mut merged_aliases: Vec<String> = existing
                .as_ref()
                .map(|e| serde_json::from_str::<Vec<String>>(&e.aliases).unwrap_or_default())
                .unwrap_or_default();
            for alias in &person.aliases {
                let trimmed = alias.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !merged_aliases.iter().any(|a| a.eq_ignore_ascii_case(trimmed)) {
                    merged_aliases.push(trimmed.to_string());
                }
            }

            // 5) 字典库：增量补丁（读取-合并-写入在存储事务内完成）
            let existing_total = self.dictionary.get_contact_total(&final_contact_id).await?;
            let is_new = existing_total.is_none();

            let mut patch = ContactPatch::new(final_contact_id.clone())
                .with_owner(owner_type, owner_id)
                .touched_at(Utc::now())
                .with_disambiguation(final_disambiguation.clone())
                // 身份线索写进 notes：同名的人下周再被提到时，靠它认出是同一个
                .with_identity_hint(person.role.clone())
                .merge_aliases(merged_aliases.iter().cloned());
            if is_new {
                // 只有新建时才用提取到的姓名；已存在的记录以字典库为准，避免改名被旧名回滚
                patch = patch.with_name(final_name.clone());
            }
            let contact_total = self.dictionary.patch_contact(&patch).await?;

            // 6) 关系库索引：以字典库权威记录为准，旧值仅作兜底
            let contact_index = self.build_contact_index(
                &contact_total,
                existing.as_ref(),
                owner_type,
                owner_id,
            );
            // 并发/归档竞态兜底：唯一索引冲突时按姓名重新定位再试一次，
            // 避免因为一个联系人写失败而丢掉整条对话的抽取结果。
            if let Err(e) = self.relational.upsert_contact_index(&contact_index).await {
                let msg = e.to_string();
                if msg.contains("UNIQUE constraint failed") {
                    if let Ok(Some(found)) = self
                        .relational
                        .find_contact_for_person_with_hints(
                            &final_name,
                            &final_disambiguation,
                            &person.aliases,
                            person.role.as_deref(),
                            owner_type,
                            owner_id,
                        )
                        .await
                    {
                        let mut retried = contact_index.clone();
                        retried.id = found.id.clone();
                        retried.is_archived = false;
                        retried.archived_at = None;
                        self.relational.upsert_contact_index(&retried).await?;
                        outcome.id_map.insert(person.contact_id.clone(), found.id.clone());
                        warn!(
                            "[MemoryWriter] 联系人 '{}' 触发唯一约束，已按姓名归并到已有 ID {}",
                            person.name, found.id
                        );
                    } else {
                        return Err(e);
                    }
                } else {
                    return Err(e);
                }
            }

            // 7) 别名索引（供合并候选识别）
            if !person.aliases.is_empty() {
                if let Err(e) = self
                    .relational
                    .insert_contact_aliases(&final_contact_id, &person.aliases, owner_type, owner_id)
                    .await
                {
                    warn!(
                        "[MemoryWriter] failed to insert aliases for {}: {}",
                        final_contact_id, e
                    );
                }
            }

            // 8) 活跃度
            let _ = self
                .relational
                .update_contact_activity(&final_contact_id, 0.1)
                .await;

            if is_new {
                outcome.created += 1;
            } else {
                outcome.updated += 1;
            }
        }

        Ok(outcome)
    }

    /// 写入学习素材（质量过滤 + 0.98 去重 + 向量入库）
    ///
    /// 这是同步/异步两条路径历史上各写一遍的地方（F-X4/F-X5），现在收敛为一处：
    /// - `quality_score < min_learning_quality` 直接跳过；
    /// - 同 owner、`material_type=learning`、相似度 > 阈值视为重复，跳过；
    /// - 向量写入失败直接返回错误（由调用方触发跨库补偿）。
    pub async fn write_learnings(
        &self,
        learnings: &[ExtractedLearning],
        owner_type: OwnerType,
        owner_id: &str,
        user_id: Option<&str>,
        conversation_id: &str,
        outcome: &mut WriteOutcome,
    ) -> Result<(), ShouxinError> {
        if learnings.is_empty() {
            return Ok(());
        }

        let filtered: Vec<&ExtractedLearning> = learnings
            .iter()
            .filter(|l| l.quality_score >= self.config.min_learning_quality)
            .collect();

        outcome.skipped_low_quality += learnings.len() - filtered.len();
        if filtered.len() < learnings.len() {
            info!(
                "[MemoryWriter] skipped {} low-quality learnings (threshold {}) for {}",
                learnings.len() - filtered.len(),
                self.config.min_learning_quality,
                conversation_id
            );
        }
        if filtered.is_empty() {
            return Ok(());
        }

        // 只有确实要写向量时才要求 vector/embedder（便于单测质量过滤逻辑）
        let (vector, embedder) = match (self.vector.as_ref(), self.embedder.as_ref()) {
            (Some(v), Some(e)) => (v, e),
            _ => {
                return Err(ShouxinError::GeneralError(
                    "MemoryWriter: vector store / embedder not configured".to_string(),
                ))
            }
        };

        let texts: Vec<String> = filtered.iter().map(|l| l.content.clone()).collect();
        let embeddings = embedder.embed_batch(texts).await?;

        let owner_type_str = format!("{:?}", owner_type);
        let mut written = 0usize;

        for (i, learning) in filtered.iter().enumerate() {
            let embedding = embeddings.get(i).cloned();

            if let Some(ref emb) = embedding {
                let is_duplicate = match vector.search(emb, 1, None).await {
                    Ok(results) => results.iter().any(|r| {
                        let mat_type = r
                            .memory
                            .metadata
                            .get("material_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let res_owner_type = r
                            .memory
                            .metadata
                            .get("owner_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let res_owner_id = r
                            .memory
                            .metadata
                            .get("owner_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        mat_type == "learning"
                            && res_owner_type == owner_type_str.as_str()
                            && res_owner_id == owner_id
                            && r.score > self.config.learning_dedup_threshold
                    }),
                    Err(e) => {
                        warn!("[MemoryWriter] learning dedup search failed: {}", e);
                        false
                    }
                };

                if is_duplicate {
                    outcome.skipped_duplicates += 1;
                    info!(
                        "[MemoryWriter] skip duplicate learning (> {}): {}",
                        self.config.learning_dedup_threshold,
                        learning.content.chars().take(40).collect::<String>()
                    );
                    continue;
                }
            }

            let memory_id = uuid::Uuid::new_v4().to_string();
            let mut metadata = HashMap::new();
            metadata.insert(
                "source_conversation_id".to_string(),
                serde_json::Value::String(conversation_id.to_string()),
            );
            metadata.insert(
                "quality_score".to_string(),
                serde_json::Value::Number(
                    serde_json::Number::from_f64(learning.quality_score as f64)
                        .unwrap_or(serde_json::Number::from(0)),
                ),
            );
            metadata.insert(
                "material_type".to_string(),
                serde_json::Value::String("learning".to_string()),
            );
            metadata.insert(
                "owner_type".to_string(),
                serde_json::Value::String(owner_type_str.clone()),
            );
            metadata.insert(
                "owner_id".to_string(),
                serde_json::Value::String(owner_id.to_string()),
            );

            let memory = crate::core::Memory {
                id: memory_id.clone(),
                user_id: user_id.map(|s| s.to_string()),
                agent_id: None,
                content: learning.content.clone(),
                embedding,
                metadata,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                memory_type: crate::core::MemoryType::LongTerm,
                state: crate::core::MemoryState::Active,
                hash: None,
                score: learning.quality_score as f32,
            };

            vector.insert(memory).await?;
            outcome.memory_ids.push(memory_id);
            written += 1;
        }

        info!(
            "[MemoryWriter] stored {} learning materials for {}",
            written, conversation_id
        );
        Ok(())
    }

    /// 由权威记录构建关系库索引
    ///
    /// 字典库缺失时（历史漂移），回退到关系库已有值，避免把旧索引清空。
    fn build_contact_index(
        &self,
        total: &crate::core::ContactTotal,
        existing: Option<&ContactIndex>,
        owner_type: OwnerType,
        owner_id: &str,
    ) -> ContactIndex {
        let aliases = if total.aliases.is_empty() {
            existing.map(|e| e.aliases.clone()).unwrap_or_else(|| "[]".to_string())
        } else {
            serde_json::to_string(&total.aliases).unwrap_or_else(|_| "[]".to_string())
        };

        let related_contact_ids = if total.related_contact_ids.is_empty() {
            existing
                .map(|e| e.related_contact_ids.clone())
                .unwrap_or_else(|| "[]".to_string())
        } else {
            serde_json::to_string(&total.related_contact_ids)
                .unwrap_or_else(|_| "[]".to_string())
        };

        let relationship_tags = if total.relationship_tags.is_empty() {
            existing
                .map(|e| e.relationship_tags.clone())
                .unwrap_or_else(|| "[]".to_string())
        } else {
            serde_json::to_string(&total.relationship_tags)
                .unwrap_or_else(|_| "[]".to_string())
        };

        ContactIndex {
            id: total.id.clone(),
            name: total.name.clone(),
            // 消歧标签：字典库有就用字典库的，没有就保留索引里已有的（不能清空，
            // 否则"前端刘洋"和"后端刘洋"下次写入会重新变成同名同标签而被误并）
            disambiguation: if total.disambiguation.is_empty() {
                existing
                    .map(|e| e.disambiguation.clone())
                    .unwrap_or_default()
            } else {
                total.disambiguation.clone()
            },
            aliases,
            is_self: total.is_self,
            created_at: total.created_at,
            last_interaction_at: total.last_interaction_at,
            related_contact_ids,
            relationship_tags,
            relation_weight: total
                .relation_weight
                .clone()
                .or_else(|| existing.map(|e| e.relation_weight.clone()))
                .unwrap_or_default(),
            background: total
                .background
                .clone()
                .or_else(|| existing.map(|e| e.background.clone()))
                .unwrap_or_default(),
            notes: total
                .notes
                .clone()
                .or_else(|| existing.map(|e| e.notes.clone()))
                .unwrap_or_default(),
            owner_type,
            owner_id: owner_id.to_string(),
            is_agent: total.is_agent,
            parent_agent_id: total.parent_agent_id.clone(),
            is_virtual: total.is_virtual,
            // 写入即代表"又有互动"：把可能已被热表裁剪归档的联系人复活，
            // 否则它虽然能查到（去重命中），却永远不会出现在检索结果里。
            is_archived: false,
            archived_at: None,
            activity_score: existing.map(|e| e.activity_score).unwrap_or(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ContactTotal;
    use crate::stores::dictionary::SledDictionaryStore;

    async fn setup() -> (
        Arc<SledDictionaryStore>,
        Arc<RelationalStore>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&db_url).await.unwrap();
        let relational = RelationalStore::new(pool);
        relational.init_schema().await.unwrap();
        let relational = Arc::new(relational);

        let dict = SledDictionaryStore::open(dir.path().join("dict").to_str().unwrap()).unwrap();
        (Arc::new(dict), relational, dir)
    }

    fn person(contact_id: &str, name: &str, aliases: &[&str]) -> ExtractedPerson {
        let mut p = ExtractedPerson::new(name.to_string());
        p.contact_id = contact_id.to_string();
        p.aliases = aliases.iter().map(|s| s.to_string()).collect();
        p
    }

    /// 回归：同名不同人必须各自拿到独立编号，不能因为重名就被合并
    ///
    /// 这是"每个联系人一个独立编号、联系人列表不怕重名"设计的核心行为：
    /// 姓名 + 消歧标签都相同才归并，标签不同必须新建。
    #[tokio::test]
    async fn test_same_name_different_disambiguation_creates_two_contacts() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        let mut front = person("p1", "刘洋", &[]);
        front.disambiguation = "前端".to_string();
        let a = writer
            .write_contacts(&[front], OwnerType::User, "u1")
            .await
            .unwrap();

        let mut back = person("p2", "刘洋", &[]);
        back.disambiguation = "后端".to_string();
        let b = writer
            .write_contacts(&[back], OwnerType::User, "u1")
            .await
            .unwrap();

        assert_eq!(a.created, 1);
        assert_eq!(b.created, 1, "同名但消歧不同，应该新建一条而不是归并");
        let id_front = a.id_map.get("p1").cloned().unwrap();
        let id_back = b.id_map.get("p2").cloned().unwrap();
        assert_ne!(id_front, id_back, "两个刘洋必须是两个独立编号");

        // 再次提到"前端刘洋"，应命中已有的那条，不新建
        let mut again = person("p3", "刘洋", &[]);
        again.disambiguation = "前端".to_string();
        let c = writer
            .write_contacts(&[again], OwnerType::User, "u1")
            .await
            .unwrap();
        assert_eq!(c.created, 0);
        assert_eq!(c.id_map.get("p3").cloned().unwrap(), id_front);
    }

    /// 跨对话同名：靠"身份线索（role）"认出是同一个人
    ///
    /// 今天说"刘洋是前端工程师"，下周只说"刘洋"（不提部门），
    /// 系统要能靠已落库的身份线索命中同一条记录，而不是重建一个人。
    #[tokio::test]
    async fn test_same_name_cross_conversation_matches_by_role() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        let mut front = person("p1", "刘洋", &[]);
        front.disambiguation = "前端".to_string();
        front.role = Some("前端工程师".to_string());
        let a = writer.write_contacts(&[front], OwnerType::User, "u1").await.unwrap();

        let mut back = person("p2", "刘洋", &[]);
        back.disambiguation = "后端".to_string();
        back.role = Some("后端工程师".to_string());
        let b = writer.write_contacts(&[back], OwnerType::User, "u1").await.unwrap();

        assert_eq!(a.created, 1);
        assert_eq!(b.created, 1);
        let id_front = a.id_map.get("p1").cloned().unwrap();
        let id_back = b.id_map.get("p2").cloned().unwrap();
        assert_ne!(id_front, id_back);

        // 跨对话：只说"后端工程师刘洋"，没给 disambiguation
        let mut later = person("p3", "刘洋", &[]);
        later.role = Some("后端工程师".to_string());
        let c = writer.write_contacts(&[later], OwnerType::User, "u1").await.unwrap();
        assert_eq!(c.created, 0, "应命中已有联系人，而不是新建");
        assert_eq!(
            c.id_map.get("p3").cloned().unwrap(),
            id_back,
            "身份线索=后端工程师，应命中后端那个刘洋"
        );
    }

    /// 回归（真实链路暴露）：同名、都没有消歧标签，但**身份矛盾**时必须分开
    ///
    /// "前端工程师刘洋" 与 "后端工程师刘洋" 长得很像，但共享后缀说明是同类身份的不同取值。
    /// 修复前会把两条合并成一条，notes 变成"身份：前端工程师；身份：后端工程师"。
    #[tokio::test]
    async fn test_same_name_conflicting_roles_create_two_contacts() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        let mut a = person("p1", "刘洋", &[]);
        a.role = Some("前端工程师".to_string());
        let r1 = writer.write_contacts(&[a], OwnerType::User, "u1").await.unwrap();
        assert_eq!(r1.created, 1);

        let mut b = person("p2", "刘洋", &[]);
        b.role = Some("后端工程师".to_string());
        let r2 = writer.write_contacts(&[b], OwnerType::User, "u1").await.unwrap();

        assert_eq!(r2.created, 1, "身份矛盾（前端 vs 后端）应视为两个人");
        assert_ne!(
            r1.id_map.get("p1").cloned().unwrap(),
            r2.id_map.get("p2").cloned().unwrap()
        );
    }

    /// 同名两条都没有标签时，无标签的新提及归到最近互动的那条，不新建
    #[tokio::test]
    async fn test_same_name_without_hints_falls_back_to_recent() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        let mut a = person("p1", "王强", &[]);
        a.disambiguation = "测试".to_string();
        writer.write_contacts(&[a], OwnerType::User, "u1").await.unwrap();

        let mut b = person("p2", "王强", &[]);
        b.disambiguation = "运维".to_string();
        let nb = writer.write_contacts(&[b], OwnerType::User, "u1").await.unwrap();
        let id_recent = nb.id_map.get("p2").cloned().unwrap();

        // 无标签、无身份线索的"王强说…"
        let c = writer
            .write_contacts(&[person("p3", "王强", &[])], OwnerType::User, "u1")
            .await
            .unwrap();
        assert_eq!(c.created, 0);
        assert_eq!(c.id_map.get("p3").cloned().unwrap(), id_recent);
    }

    #[tokio::test]
    async fn test_write_contacts_creates_dictionary_and_index() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        let outcome = writer
            .write_contacts(&[person("c1", "张三", &["小张"])], OwnerType::User, "u1")
            .await
            .unwrap();

        assert_eq!(outcome.created, 1);
        assert_eq!(outcome.id_map.get("c1").map(String::as_str), Some("c1"));

        let total = dict.get_contact_total("c1").await.unwrap().unwrap();
        assert_eq!(total.name, "张三");
        assert_eq!(total.owner_type, OwnerType::User);
        assert_eq!(total.owner_id, "u1");
        assert!(total.aliases.contains(&"小张".to_string()));

        let index = writer
            .relational()
            .find_contact_by_any_name("张三", OwnerType::User, "u1")
            .await
            .unwrap()
            .expect("contact index should exist");
        assert!(index.aliases.contains("小张"));
    }

    #[tokio::test]
    async fn test_write_contacts_preserves_business_fields() {
        let (dict, rel, _dir) = setup().await;
        let writer = MemoryWriter::new(dict.clone(), rel);

        writer
            .write_contacts(&[person("c1", "张三", &["小张"])], OwnerType::User, "u1")
            .await
            .unwrap();

        // 模拟心跳/人工补全的业务字段
        let mut total = dict.get_contact_total("c1").await.unwrap().unwrap();
        total.background = Some("大学同学".to_string());
        total.notes = Some("喜欢咖啡".to_string());
        total.relationship_tags = vec!["朋友".to_string()];
        total.relation_weight = Some("0.8".to_string());
        dict.save_contact(&total).await.unwrap();

        // 第二次提取：没有别名、只有名字
        let outcome = writer
            .write_contacts(&[person("c1", "张三", &[])], OwnerType::User, "u1")
            .await
            .unwrap();
        assert_eq!(outcome.updated, 1);

        let after = dict.get_contact_total("c1").await.unwrap().unwrap();
        assert_eq!(after.background.as_deref(), Some("大学同学"));
        assert_eq!(after.notes.as_deref(), Some("喜欢咖啡"));
        assert_eq!(after.relationship_tags, vec!["朋友".to_string()]);
        assert_eq!(after.relation_weight.as_deref(), Some("0.8"));
        assert!(after.aliases.contains(&"小张".to_string()), "alias should be preserved");
    }

    #[tokio::test]
    async fn test_write_contacts_skips_virtual_entity() {
        let (dict, rel, _dir) = setup().await;

        // 预置虚拟角色：现在写在**独立表** virtual_character 里，
        // 不再往 contact_index 里塞 is_virtual=1 的行
        rel.virtual_alias_store()
            .init_schema()
            .await
            .unwrap();
        rel.virtual_alias_store()
            .create(crate::stores::relational::NewVirtualCharacter {
                name: "皮卡丘".to_string(),
                source_work: Some("宝可梦".to_string()),
                category: Some("builtin".to_string()),
                owner_id: Some("u1".to_string()),
                is_builtin: true,
                ..Default::default()
            })
            .await
            .unwrap();

        let writer = MemoryWriter::new(dict.clone(), rel);
        let outcome = writer
            .write_contacts(&[person("p1", "皮卡丘", &[])], OwnerType::User, "u1")
            .await
            .unwrap();

        assert_eq!(outcome.skipped_virtual, 1);
        assert_eq!(outcome.created, 0);
        // 角色不会进真人联系人表
        assert!(outcome.id_map.is_empty());
        assert_eq!(outcome.character_id_map.len(), 1);
    }
}

#[cfg(test)]
mod learning_tests {
    use super::*;

    fn learning(content: &str, score: f32) -> ExtractedLearning {
        let mut l = ExtractedLearning::new(content.to_string());
        l.quality_score = score;
        l
    }

    /// 低质量素材在进入向量库之前就被过滤掉，
    /// 因此即使没有配置 vector/embedder，也应正常返回而不是报错。
    #[tokio::test]
    async fn test_low_quality_learnings_filtered_without_vector() {
        let dict = Arc::new(
            crate::stores::dictionary::SledDictionaryStore::in_memory().unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let rel = RelationalStore::new(pool);
        rel.init_schema().await.unwrap();

        // 故意不注入 vector/embedder
        let writer = MemoryWriter::new(dict, Arc::new(rel));
        let mut outcome = WriteOutcome::default();

        let items = vec![learning("低质量-1", 0.1), learning("低质量-2", 0.5)];
        writer
            .write_learnings(&items, OwnerType::User, "u1", None, "conv-1", &mut outcome)
            .await
            .expect("all items filtered out, should not require vector store");

        assert_eq!(outcome.skipped_low_quality, 2);
        assert!(outcome.memory_ids.is_empty());
    }

    /// 有达标素材但未配置向量库时，必须显式报错而不是静默丢弃
    #[tokio::test]
    async fn test_missing_vector_store_is_error() {
        let dict = Arc::new(
            crate::stores::dictionary::SledDictionaryStore::in_memory().unwrap(),
        );
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t2.db");
        let url = format!("sqlite://{}?mode=rwc", db_path.display());
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let rel = RelationalStore::new(pool);
        rel.init_schema().await.unwrap();

        let writer = MemoryWriter::new(dict, Arc::new(rel));
        let mut outcome = WriteOutcome::default();

        let err = writer
            .write_learnings(
                &[learning("高质量", 0.95)],
                OwnerType::User,
                "u1",
                None,
                "conv-2",
                &mut outcome,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }
}
