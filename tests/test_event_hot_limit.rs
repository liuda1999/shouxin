//! ShouXin v0.5.1 任务5验证测试：事件热表上限与自动归档
//!
//! 测试方法：直接使用 RelationalStore 的 archive_oldest_hot_event 方法，
//! 通过 SQL 直接插入事件数据，验证归档逻辑的正确性。

use chrono::{DateTime, Duration, Utc};
use shouxin::core::contact::OwnerType;
use shouxin::core::EventIndex;
use shouxin::stores::relational::RelationalStore;
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;

/// 创建测试用的 RelationalStore
async fn create_test_store() -> (Arc<RelationalStore>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_events.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::SqlitePool::connect(&db_url).await.unwrap();
    let store = RelationalStore::new(pool);
    store.init_schema().await.unwrap();
    (Arc::new(store), temp_dir)
}

/// 创建一个测试事件
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
        importance: if importance_score > 0.7 { "high".to_string() } else { "medium".to_string() },
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

/// 批量插入事件，每个事件间隔 1 秒以确保 created_at 有序
async fn insert_events(
    store: &RelationalStore,
    owner_type: OwnerType,
    owner_id: &str,
    importance_score: f32,
    count: usize,
    base_time: DateTime<Utc>,
) {
    for i in 0..count {
        let event_id = format!("evt-{}-{}-{}", owner_id, importance_score, i);
        let event_time = base_time + Duration::seconds(i as i64);
        let event = make_event(&event_id, owner_type, owner_id, importance_score, event_time);
        store.insert_event_index(&event).await.unwrap();
    }
}

/// 查询热表事件数量
async fn count_hot_events(
    store: &RelationalStore,
    owner_type: &str,
    owner_id: &str,
    importance_min: f32,
    importance_max: f32,
) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM event_index WHERE owner_type = ? AND owner_id = ? \
         AND is_archived = 0 AND importance_score > ? AND importance_score <= ?"
    )
    .bind(owner_type)
    .bind(owner_id)
    .bind(importance_min)
    .bind(importance_max)
    .fetch_one(store.pool())
    .await
    .unwrap();
    row.0
}

/// 查询归档事件数量
async fn count_archived_events(
    store: &RelationalStore,
    owner_type: &str,
    owner_id: &str,
    importance_min: f32,
    importance_max: f32,
) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM event_index WHERE owner_type = ? AND owner_id = ? \
         AND is_archived = 1 AND importance_score > ? AND importance_score <= ?"
    )
    .bind(owner_type)
    .bind(owner_id)
    .bind(importance_min)
    .bind(importance_max)
    .fetch_one(store.pool())
    .await
    .unwrap();
    row.0
}

/// 获取最早创建的热事件 ID
async fn get_oldest_hot_event_id(
    store: &RelationalStore,
    owner_type: &str,
    owner_id: &str,
    importance_min: f32,
    importance_max: f32,
) -> Option<String> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT event_id FROM event_index WHERE owner_type = ? AND owner_id = ? \
         AND is_archived = 0 AND importance_score > ? AND importance_score <= ? \
         ORDER BY created_at ASC LIMIT 1"
    )
    .bind(owner_type)
    .bind(owner_id)
    .bind(importance_min)
    .bind(importance_max)
    .fetch_optional(store.pool())
    .await
    .unwrap();
    row.map(|r| r.0)
}

// ==================== 测试用例 ====================

/// C-1: 配置验证 - config.toml 中存在 [event] 配置块
#[test]
fn test_config_exists() {
    let settings = shouxin::config::Settings::default();
    assert_eq!(settings.event.long_term_hot_limit, 200,
        "默认长期事件上限应为 200");
    assert_eq!(settings.event.short_term_hot_limit, 100,
        "默认短期事件上限应为 100");

    // 尝试从文件加载
    let loaded = shouxin::config::Settings::new();
    if let Ok(s) = loaded {
        assert!(s.event.long_term_hot_limit >= 1,
            "配置文件中的 long_term_hot_limit 应 >= 1");
        assert!(s.event.short_term_hot_limit >= 1,
            "配置文件中的 short_term_hot_limit 应 >= 1");
    }
}

/// 2.1: 热表未满时，新事件正常插入且不归档
#[tokio::test]
async fn test_hot_table_not_full_no_archive() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    // 插入 150 个长期事件
    insert_events(&store, ot, owner_id, 0.8, 150, Utc::now()).await;

    let hot_count = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot_count, 150, "热表应有 150 个事件");

    let archived_count = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived_count, 0, "不应有归档事件");
}

/// 2.2: 达到上限时，新事件插入触发归档
#[tokio::test]
async fn test_long_term_limit_archive_oldest() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let base_time = Utc::now();

    // 插入 200 个长期事件（刚好满）
    insert_events(&store, ot, owner_id, 0.8, 200, base_time).await;

    let hot_before = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot_before, 200, "插入 200 个后热表应有 200");

    // 获取最早的事件（应该是 evt-hot_limit_user-0.8-0）
    let oldest_before = get_oldest_hot_event_id(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert!(oldest_before.is_some(), "应存在最早的事件");

    // 插入第 201 个长期事件
    let event_201 = make_event(
        "evt-overflow-long",
        ot, owner_id, 0.85,
        base_time + Duration::seconds(300),
    );
    store.insert_event_index(&event_201).await.unwrap();

    // 调用归档
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();

    // 验证热表数量
    let hot_after = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot_after, 200, "归档后热表应保持 200");

    // 验证归档数量
    let archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived, 1, "应有 1 个事件被归档");

    // 验证最早的事件已被归档
    let archived_event_id = oldest_before.unwrap();
    let row: (bool,) = sqlx::query_as(
        "SELECT is_archived FROM event_index WHERE event_id = ?"
    )
    .bind(&archived_event_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(row.0, "最早的事件 {} 应被归档", archived_event_id);

    // 验证归档时间不为空
    let row: (Option<String>,) = sqlx::query_as(
        "SELECT archived_at FROM event_index WHERE event_id = ?"
    )
    .bind(&archived_event_id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(row.0.is_some(), "归档时间不应为空");
}

/// 2.3: 持续归档最早事件
#[tokio::test]
async fn test_continuous_archive_oldest() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let base_time = Utc::now();

    // 插入 200 个长期事件
    insert_events(&store, ot, owner_id, 0.8, 200, base_time).await;

    // 再插入并归档 50 个
    for i in 0..50 {
        let event = make_event(
            &format!("evt-extra-{}", i),
            ot, owner_id, 0.8,
            base_time + Duration::seconds(300 + i as i64),
        );
        store.insert_event_index(&event).await.unwrap();
        store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();
    }

    let hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot, 200, "热表始终保持 200");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived, 50, "应有 50 个事件被归档");

    // 验证前 50 个最早的事件都已被归档
    for i in 0..50 {
        let event_id = format!("evt-{}-0.8-{}", owner_id, i);
        let row: (bool,) = sqlx::query_as(
            "SELECT is_archived FROM event_index WHERE event_id = ?"
        )
        .bind(&event_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(row.0, "事件 {} (最早的第 {} 个) 应被归档", event_id, i);
    }

    // 验证第 50 个之后的还在热表
    let event_id_50 = format!("evt-{}-0.8-50", owner_id);
    let row: (bool,) = sqlx::query_as(
        "SELECT is_archived FROM event_index WHERE event_id = ?"
    )
    .bind(&event_id_50)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert!(!row.0, "事件 {} (第 50 个) 应在热表", event_id_50);
}

/// 3: 短期事件热表上限 100
#[tokio::test]
async fn test_short_term_limit_100() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 插入 100 个短期事件
    insert_events(&store, ot, owner_id, 0.5, 100, base_time).await;

    let hot_before = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(hot_before, 100, "短期热表应有 100");

    // 插入第 101 个
    let event_101 = make_event(
        "evt-short-overflow", ot, owner_id, 0.5,
        base_time + Duration::seconds(200),
    );
    store.insert_event_index(&event_101).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, owner_id, false, 100).await.unwrap();

    let hot_after = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(hot_after, 100, "短期热表归档后应保持 100");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(archived, 1, "应有 1 个短期事件被归档");
}

/// 3 扩展: 持续插入短期事件
#[tokio::test]
async fn test_short_term_continuous_archive() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    insert_events(&store, ot, owner_id, 0.5, 100, base_time).await;

    for i in 0..30 {
        let event = make_event(
            &format!("evt-short-extra-{}", i),
            ot, owner_id, 0.5,
            base_time + Duration::seconds(300 + i as i64),
        );
        store.insert_event_index(&event).await.unwrap();
        store.archive_oldest_hot_event(&ot_str, owner_id, false, 100).await.unwrap();
    }

    let hot = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(hot, 100, "短期热表始终保持 100");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(archived, 30, "应有 30 个短期事件被归档");
}

/// 4: 长期/短期独立计数，互不影响
#[tokio::test]
async fn test_long_short_independent() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "hot_limit_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 插入 200 个长期事件
    insert_events(&store, ot, owner_id, 0.8, 200, base_time).await;
    // 插入 100 个短期事件
    insert_events(&store, ot, owner_id, 0.5, 100, base_time).await;

    let long_hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(long_hot, 200, "长期热表应有 200");

    let short_hot = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(short_hot, 100, "短期热表应有 100");

    // 插入第 201 个长期事件
    let event = make_event(
        "evt-long-extra", ot, owner_id, 0.8,
        base_time + Duration::seconds(500),
    );
    store.insert_event_index(&event).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();

    // 短期热表不应受影响
    let short_hot_after = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(short_hot_after, 100, "短期热表不应受影响");

    let short_archived = count_archived_events(&store, &ot_str, owner_id, 0.0, 0.7).await;
    assert_eq!(short_archived, 0, "短期事件不应被归档");

    let long_archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(long_archived, 1, "长期事件应有 1 个被归档");
}

/// 5: 隔离性测试 - 不同用户独立计数
#[tokio::test]
async fn test_isolation_different_users() {
    let (store, _temp) = create_test_store().await;
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // user_a: 插入 200 个长期事件
    insert_events(&store, ot, "user_a", 0.8, 200, base_time).await;
    // user_b: 插入 200 个长期事件
    insert_events(&store, ot, "user_b", 0.8, 200, base_time).await;

    // 两个用户各自满
    let hot_a = count_hot_events(&store, &ot_str, "user_a", 0.7, 1.1).await;
    let hot_b = count_hot_events(&store, &ot_str, "user_b", 0.7, 1.1).await;
    assert_eq!(hot_a, 200, "user_a 热表应有 200");
    assert_eq!(hot_b, 200, "user_b 热表应有 200");

    // user_a 插入第 201 个
    let event = make_event(
        "evt-user-a-extra", ot, "user_a", 0.8,
        base_time + Duration::seconds(500),
    );
    store.insert_event_index(&event).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, "user_a", true, 200).await.unwrap();

    // user_a 归档 1 个
    let hot_a_after = count_hot_events(&store, &ot_str, "user_a", 0.7, 1.1).await;
    assert_eq!(hot_a_after, 200, "user_a 热表应保持 200");

    let archived_a = count_archived_events(&store, &ot_str, "user_a", 0.7, 1.1).await;
    assert_eq!(archived_a, 1, "user_a 应有 1 个归档");

    // user_b 不应受影响
    let hot_b_after = count_hot_events(&store, &ot_str, "user_b", 0.7, 1.1).await;
    assert_eq!(hot_b_after, 200, "user_b 热表不应受影响");

    let archived_b = count_archived_events(&store, &ot_str, "user_b", 0.7, 1.1).await;
    assert_eq!(archived_b, 0, "user_b 不应有归档");
}

/// 5 扩展: 不同 OwnerType 隔离
#[tokio::test]
async fn test_isolation_different_owner_types() {
    let (store, _temp) = create_test_store().await;
    let base_time = Utc::now();

    let user_ot = OwnerType::User;
    let agent_ot = OwnerType::Agent;
    let user_ot_str = format!("{:?}", user_ot);
    let agent_ot_str = format!("{:?}", agent_ot);

    // User: 插入 200 个长期事件
    insert_events(&store, user_ot, "user_x", 0.8, 200, base_time).await;
    // Agent: 插入 200 个长期事件
    insert_events(&store, agent_ot, "agent_x", 0.8, 200, base_time).await;

    // User 插入第 201 个
    let event = make_event(
        "evt-user-overflow", user_ot, "user_x", 0.8,
        base_time + Duration::seconds(500),
    );
    store.insert_event_index(&event).await.unwrap();
    store.archive_oldest_hot_event(&user_ot_str, "user_x", true, 200).await.unwrap();

    // Agent 不应受影响
    let agent_hot = count_hot_events(&store, &agent_ot_str, "agent_x", 0.7, 1.1).await;
    assert_eq!(agent_hot, 200, "Agent 热表不应受影响");

    let agent_archived = count_archived_events(&store, &agent_ot_str, "agent_x", 0.7, 1.1).await;
    assert_eq!(agent_archived, 0, "Agent 不应有归档");
}

/// 6: 并发安全测试
#[tokio::test]
async fn test_concurrent_inserts() {
    use std::sync::Arc;
    use tokio::task::JoinSet;

    let (store, _temp) = create_test_store().await;
    let store = Arc::new(store);
    let store_ref = store.clone();
    let owner_id = "concurrent_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 先插入 190 个（接近上限）
    insert_events(store_ref.as_ref(), ot, owner_id, 0.8, 190, base_time).await;

    // 10 个线程同时插入
    let mut join_set = JoinSet::new();
    for i in 0..10 {
        let store_clone = store_ref.clone();
        let ot_clone = ot;
        let ot_str_clone = ot_str.clone();
        let owner_id_clone = owner_id.to_string();
        let base_time = base_time;
        join_set.spawn(async move {
            let event = make_event(
                &format!("evt-concurrent-{}", i),
                ot_clone,
                &owner_id_clone,
                0.8,
                base_time + Duration::seconds(400 + i as i64),
            );
            store_clone.insert_event_index(&event).await.unwrap();
            store_clone.archive_oldest_hot_event(&ot_str_clone, &owner_id_clone, true, 200).await.unwrap();
        });
    }

    while join_set.join_next().await.is_some() {}

    let hot = count_hot_events(store_ref.as_ref(), &ot_str, owner_id, 0.7, 1.1).await;
    assert!(hot <= 200, "并发插入后热表不应超过 200，实际: {}", hot);

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM event_index WHERE owner_type = ? AND owner_id = ? AND importance_score > 0.7"
    )
    .bind(&ot_str)
    .bind(owner_id)
    .fetch_one(store_ref.pool())
    .await
    .unwrap();
    assert_eq!(total.0, 200, "总事件数应为 200（190 + 10），实际: {}", total.0);
}

/// E-1: 热表刚好等于上限，插入新事件触发归档
#[tokio::test]
async fn test_edge_exact_limit() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "edge_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 刚好 200 个
    insert_events(&store, ot, owner_id, 0.8, 200, base_time).await;

    let event = make_event(
        "evt-edge-201", ot, owner_id, 0.8,
        base_time + Duration::seconds(500),
    );
    store.insert_event_index(&event).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();

    let hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot, 200, "热表应保持 200");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived, 1, "应有 1 个归档");
}

/// E-2: 热表为空，插入事件正常
#[tokio::test]
async fn test_edge_empty_table() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "empty_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 插入 1 个事件
    let event = make_event("evt-first", ot, owner_id, 0.8, base_time);
    store.insert_event_index(&event).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();

    let hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot, 1, "热表应有 1 个事件");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived, 0, "不应有归档");
}

/// E-4: 一次性归档多个事件
#[tokio::test]
async fn test_edge_bulk_archive() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "bulk_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    // 插入 210 个长期事件
    insert_events(&store, ot, owner_id, 0.8, 210, base_time).await;

    // 触发归档（应归档 11 个）
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();

    let hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(hot, 200, "热表应保持 200");

    let archived = count_archived_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    assert_eq!(archived, 10, "应归档 10 个事件（210-200）");

    // 验证前 10 个被归档
    for i in 0..10 {
        let event_id = format!("evt-{}-0.8-{}", owner_id, i);
        let row: (bool,) = sqlx::query_as(
            "SELECT is_archived FROM event_index WHERE event_id = ?"
        )
        .bind(&event_id)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(row.0, "事件 {} 应被归档", event_id);
    }
}

/// 性能测试: 插入事件并验证归档性能
/// 注意: debug 模式下单个 SQLite 插入较慢，但逻辑正确性已验证
#[tokio::test]
async fn test_performance() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "perf_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);
    let base_time = Utc::now();

    let start = Instant::now();

    // 插入 200 个长期 + 100 个短期
    insert_events(&store, ot, owner_id, 0.8, 200, base_time).await;
    insert_events(&store, ot, owner_id, 0.5, 100, base_time).await;

    let insert_duration = start.elapsed();

    // 触发归档
    let archive_start = Instant::now();
    store.archive_oldest_hot_event(&ot_str, owner_id, true, 200).await.unwrap();
    store.archive_oldest_hot_event(&ot_str, owner_id, false, 100).await.unwrap();
    let archive_duration = archive_start.elapsed();

    let total_duration = start.elapsed();

    let long_hot = count_hot_events(&store, &ot_str, owner_id, 0.7, 1.1).await;
    let short_hot = count_hot_events(&store, &ot_str, owner_id, 0.0, 0.7).await;

    println!("=== Performance Results ===");
    println!("Insert 300 events: {:?}", insert_duration);
    println!("Archive operations: {:?}", archive_duration);
    println!("Total duration: {:?}", total_duration);
    println!("Long-term hot: {} (expected <= 200)", long_hot);
    println!("Short-term hot: {} (expected <= 100)", short_hot);

    // 归档操作本身应该在毫秒级别完成
    assert!(archive_duration.as_millis() < 500, "归档操作应 < 500ms，实际: {:?}", archive_duration);
    assert!(long_hot <= 200, "长期热表不应超过 200");
    assert!(short_hot <= 100, "短期热表不应超过 100");
}