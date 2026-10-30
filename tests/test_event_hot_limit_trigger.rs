//! 事件热表上限「触发点」验证
//!
//! 背景：归档逻辑（`archive_oldest_hot_event`）本身是对的，但此前**只在同步抽取路径
//! `three_library_manager::add()` 里手动调用过一次**。真实对话走的是
//! 「POST /api/memory/add → 入队 → 后台 worker 写库」，`background_queue.rs` 那条路
//! 完全没有检查，于是热表只增不减，冷热分层静默失效且无任何报错。
//!
//! 修复方式：把触发点收敛到事件写入的唯一出口 `RelationalStore::insert_event_index`，
//! 这样异步队列 / 同步抽取 / 心跳 / 内部 API 各条链路都会被裁剪。
//!
//! 本文件只验证「触发点是否接上」——即**不做任何显式归档调用**，
//! 仅通过 `insert_event_index` 插入，然后断言热表被自动裁剪。

use chrono::{DateTime, Duration, Utc};
use shouxin::core::contact::OwnerType;
use shouxin::core::EventIndex;
use shouxin::stores::relational::RelationalStore;
use sqlx::SqlitePool;
use tempfile::TempDir;

async fn create_store_with_limits(
    long_term: usize,
    short_term: usize,
) -> (RelationalStore, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("hot_trigger.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePool::connect(&db_url).await.unwrap();
    let store = RelationalStore::new(pool).with_hot_limits(long_term, short_term);
    store.init_schema().await.unwrap();
    (store, temp_dir)
}

fn make_event(
    event_id: &str,
    owner_type: OwnerType,
    owner_id: &str,
    importance_score: f32,
    created_at: DateTime<Utc>,
) -> EventIndex {
    EventIndex {
        event_id: event_id.to_string(),
        contact_ids: "[]".to_string(),
        owner_id: owner_id.to_string(),
        owner_type,
        time_range_start: created_at,
        time_range_end: created_at,
        importance: if importance_score > 0.7 {
            "high".to_string()
        } else {
            "medium".to_string()
        },
        status: "pending".to_string(),
        created_at,
        completed_at: None,
        summary: format!("event-{}", event_id),
        title: Some(format!("Test Event {}", event_id)),
        event_type: "test".to_string(),
        deadline: None,
        auto_invalid_at: None,
        is_auto_invalid: false,
        is_merged: false,
        merge_target_id: None,
        last_merge_check_at: None,
        event_time: Some(created_at),
        importance_score,
        is_archived: false,
        archived_at: None,
    }
}

async fn insert_many(
    store: &RelationalStore,
    owner_type: OwnerType,
    owner_id: &str,
    importance_score: f32,
    count: usize,
    base: DateTime<Utc>,
) {
    insert_many_from(store, owner_type, owner_id, importance_score, count, base, 0).await
}

/// 带起始序号的插入：同一 owner 分多批插入时必须错开 id，
/// 否则会命中 `INSERT OR IGNORE` 去重而根本没写入新事件
async fn insert_many_from(
    store: &RelationalStore,
    owner_type: OwnerType,
    owner_id: &str,
    importance_score: f32,
    count: usize,
    base: DateTime<Utc>,
    start: usize,
) {
    for i in 0..count {
        let id = format!("evt-{}-{}", owner_id, start + i);
        let ev = make_event(
            &id,
            owner_type,
            owner_id,
            importance_score,
            base + Duration::seconds((start + i) as i64),
        );
        store.insert_event_index(&ev).await.unwrap();
    }
}

async fn counts(
    store: &RelationalStore,
    owner_id: &str,
    min: f32,
    max: f32,
) -> (i64, i64) {
    let hot: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM event_index WHERE owner_type = 'User' AND owner_id = ? \
         AND is_archived = 0 AND importance_score > ? AND importance_score <= ?",
    )
    .bind(owner_id)
    .bind(min)
    .bind(max)
    .fetch_one(store.pool())
    .await
    .unwrap();

    let cold: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM event_index WHERE owner_type = 'User' AND owner_id = ? \
         AND is_archived = 1 AND importance_score > ? AND importance_score <= ?",
    )
    .bind(owner_id)
    .bind(min)
    .bind(max)
    .fetch_one(store.pool())
    .await
    .unwrap();

    (hot.0, cold.0)
}

/// 核心用例：默认限额（长期 200 / 短期 100）下，仅靠 insert_event_index 就会裁剪热表
#[tokio::test]
async fn test_default_limits_triggered_on_insert() {
    let (store, _tmp) = create_store_with_limits(200, 100).await;
    let base = Utc::now() - Duration::days(10);

    // 短期（importance <= 0.7）：热表上限 100
    insert_many(&store, OwnerType::User, "u-short", 0.5, 100, base).await;
    let (hot, cold) = counts(&store, "u-short", 0.0, 0.7).await;
    assert_eq!((hot, cold), (100, 0), "刚好到上限时不应归档");

    // 再插 5 条（id 从第 100 起，避免被去重拦掉）→ 应自动淘汰最早的 5 条，热表回到 100
    insert_many_from(
        &store,
        OwnerType::User,
        "u-short",
        0.5,
        5,
        base + Duration::seconds(200),
        100,
    )
    .await;
    let (hot, cold) = counts(&store, "u-short", 0.0, 0.7).await;
    assert_eq!(hot, 100, "超出上限后热表应回到 100");
    assert_eq!(cold, 5, "超出上限的 5 条应被自动归档（无需显式调用）");

    // 长期（importance > 0.7）：上限 200，逐条淘汰
    insert_many(&store, OwnerType::User, "u-long", 0.9, 203, base).await;
    let (hot, cold) = counts(&store, "u-long", 0.7, 1.1).await;
    assert_eq!(hot, 200, "长期热表应保持在 200");
    assert_eq!(cold, 3, "多出的 3 条应被自动归档");
}

/// 自定义限额生效（`with_hot_limits`）：这是端到端验证要用的小阈值场景
#[tokio::test]
async fn test_custom_limits_respected() {
    let (store, _tmp) = create_store_with_limits(3, 2).await;
    let base = Utc::now() - Duration::days(5);

    insert_many(&store, OwnerType::User, "u-custom", 0.9, 6, base).await;
    let (hot, cold) = counts(&store, "u-custom", 0.7, 1.1).await;
    assert_eq!(hot, 3, "长期上限被配成 3，热表应只留 3 条");
    assert_eq!(cold, 3, "其余 3 条应被归档");
}

/// 淘汰顺序：必须是最早创建的先走
#[tokio::test]
async fn test_oldest_archived_first() {
    let (store, _tmp) = create_store_with_limits(5, 5).await;
    let base = Utc::now() - Duration::days(5);
    insert_many(&store, OwnerType::User, "u-order", 0.5, 8, base).await;

    let archived_ids: Vec<(String,)> = sqlx::query_as(
        "SELECT event_id FROM event_index WHERE owner_type = 'User' AND owner_id = ? \
         AND is_archived = 1 ORDER BY event_id ASC",
    )
    .bind("u-order")
    .fetch_all(store.pool())
    .await
    .unwrap();

    let archived: Vec<String> = archived_ids.into_iter().map(|r| r.0).collect();
    assert_eq!(archived.len(), 3, "8 条 - 上限 5 = 应归档 3 条");
    for i in 0..3 {
        assert_eq!(archived[i], format!("evt-u-order-{}", i), "最早的第 {} 条应被归档", i);
    }
}

/// 不同 owner 之间互不干扰
#[tokio::test]
async fn test_isolation_between_owners() {
    let (store, _tmp) = create_store_with_limits(4, 4).await;
    let base = Utc::now() - Duration::days(3);
    insert_many(&store, OwnerType::User, "u-a", 0.5, 4, base).await;
    insert_many(&store, OwnerType::User, "u-b", 0.5, 4, base).await;

    let (hot_a, cold_a) = counts(&store, "u-a", 0.0, 0.7).await;
    let (hot_b, cold_b) = counts(&store, "u-b", 0.0, 0.7).await;
    assert_eq!((hot_a, cold_a), (4, 0), "u-a 未超限");
    assert_eq!((hot_b, cold_b), (4, 0), "u-b 未超限，不应被 u-a 的写入挤掉");
}

/// 重复插入（去重拦截）不应触发额外归档
#[tokio::test]
async fn test_duplicate_insert_does_not_over_archive() {
    let (store, _tmp) = create_store_with_limits(3, 3).await;
    let base = Utc::now() - Duration::days(2);
    insert_many(&store, OwnerType::User, "u-dup", 0.5, 3, base).await;

    // 重复插入同一批：INSERT OR IGNORE 命中，rows_affected = 0
    insert_many(&store, OwnerType::User, "u-dup", 0.5, 3, base).await;

    let (hot, cold) = counts(&store, "u-dup", 0.0, 0.7).await;
    assert_eq!((hot, cold), (3, 0), "重复插入不应被计为新事件而误归档");
}
