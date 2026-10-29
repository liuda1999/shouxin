//! 联系人全息详情的存储层回归测试
//!
//! 覆盖 `GET /api/contacts/:id/detail` 依赖的三个取数能力：
//! 1. `contact_index.is_virtual` 能被读出来（区分虚拟人物）；
//! 2. 事件能按 `contact_ids` 反查（该联系人参与过什么）；
//! 3. 对话能按 owner + 姓名/别名逐字命中（谁提到过他），且不越 owner。

use chrono::Utc;
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::TempDir;

use shouxin::core::contact::{ContactIndex, EventIndex, OwnerType};
use shouxin::stores::dictionary::SledDictionaryStore;
use shouxin::stores::relational::RelationalStore;
use shouxin::traits::store_traits::DictionaryStoreTrait;

struct Ctx {
    rel: Arc<RelationalStore>,
    dict: Arc<SledDictionaryStore>,
    _temp: TempDir,
}

async fn setup() -> Ctx {
    let temp = TempDir::new().unwrap();
    let db_url = format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("contact_detail.db").display()
    );
    let pool = SqlitePool::connect(&db_url).await.unwrap();
    let rel = RelationalStore::new(pool);
    rel.init_schema().await.unwrap();

    let dict = SledDictionaryStore::open(temp.path().join("sled").to_str().unwrap()).unwrap();

    Ctx { rel: Arc::new(rel), dict: Arc::new(dict), _temp: temp }
}

fn contact(id: &str, name: &str, owner: &str, virtual_flag: bool) -> ContactIndex {
    let now = Utc::now();
    ContactIndex {
        id: id.to_string(),
        name: name.to_string(),
        aliases: format!("[\"{}\"]", name),
        is_self: false,
        created_at: now,
        last_interaction_at: now,
        related_contact_ids: "[]".to_string(),
        relationship_tags: "[]".to_string(),
        relation_weight: String::new(),
        background: String::new(),
        notes: String::new(),
        owner_type: OwnerType::User,
        owner_id: owner.to_string(),
        is_agent: false,
        parent_agent_id: None,
        is_virtual: virtual_flag,
        is_archived: false,
        archived_at: None,
        activity_score: 0.0,
    }
}

/// 虚拟人物与真实联系人必须能被区分出来
#[tokio::test]
async fn is_virtual_is_readable_via_profile_fields() {
    let ctx = setup().await;
    ctx.rel.upsert_contact_index(&contact("c1", "苏晚晴", "u1", true)).await.unwrap();
    ctx.rel.upsert_contact_index(&contact("c2", "张伟", "u1", false)).await.unwrap();

    let map = ctx
        .rel
        .fetch_contact_profiles(&["c1".to_string(), "c2".to_string()])
        .await
        .unwrap();

    assert!(map.get("c1").unwrap().is_virtual, "虚拟角色应标记 is_virtual");
    assert!(!map.get("c2").unwrap().is_virtual, "真实联系人不应被误标");
}

/// 事件按联系人反查：只返回 contact_ids 里真的包含该 id 的事件
#[tokio::test]
async fn events_are_queryable_by_contact_id() {
    let ctx = setup().await;
    ctx.rel.upsert_contact_index(&contact("c1", "老王", "u1", false)).await.unwrap();

    let mut e1 = EventIndex::new("e1".into(), vec!["c1".into()], "架构评审会".into());
    e1.owner_id = "u1".into();
    let mut e2 = EventIndex::new("e2".into(), vec!["c_other".into()], "与老王无关的会议".into());
    e2.owner_id = "u1".into();
    ctx.rel.insert_event_index(&e1).await.unwrap();
    ctx.rel.insert_event_index(&e2).await.unwrap();

    let events = ctx.rel.search_events_by_contact("c1", OwnerType::User, "u1").await.unwrap();
    let ids: Vec<String> = events.iter().map(|e| e.event_id.clone()).collect();
    assert_eq!(ids, vec!["e1".to_string()], "只应命中 contact_ids 含 c1 的事件");
}

/// 相关记忆：姓名/别名逐字命中，且严格限定 owner
#[tokio::test]
async fn conversations_are_matched_by_name_and_owner() {
    let ctx = setup().await;
    let now = Utc::now();

    ctx.dict
        .save_conversation("conv1", "老王是我的同事，在研发部做后端", now, OwnerType::User, "u1")
        .await
        .unwrap();
    ctx.dict
        .save_conversation("conv2", "今天讨论的是缓存过期策略", now, OwnerType::User, "u1")
        .await
        .unwrap();
    // 同名但不同 owner，必须被隔离掉
    ctx.dict
        .save_conversation("conv3", "老王欠我一顿饭", now, OwnerType::User, "u2")
        .await
        .unwrap();

    let hits = ctx
        .dict
        .search_conversations_by_owner_keywords(
            OwnerType::User,
            "u1",
            &["老王".to_string(), "王建国".to_string()],
            100,
            10,
        )
        .await
        .unwrap();

    let ids: Vec<String> = hits.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids, vec!["conv1".to_string()], "应只命中含姓名的、同 owner 的对话");
}
