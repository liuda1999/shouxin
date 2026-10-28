//! ShouXin v0.5.1 任务8验证测试：联系人热冷分离与自动归档

use chrono::{DateTime, Duration, Utc};
use shouxin::core::contact::{ContactIndex, OwnerType};
use shouxin::stores::relational::RelationalStore;
use sqlx::{Row, SqlitePool};
use std::sync::Arc;
use tempfile::TempDir;

struct TestContext {
    store: Arc<RelationalStore>,
    pool: SqlitePool,
    _temp: TempDir,
}

async fn create_test_store() -> TestContext {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_contacts.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePool::connect(&db_url).await.unwrap();
    let store = RelationalStore::new(pool.clone());
    store.init_schema().await.unwrap();
    let store = Arc::new(store);
    TestContext { store, pool, _temp: temp_dir }
}

fn make_contact(id: &str, name: &str, last_interaction: DateTime<Utc>) -> ContactIndex {
    let now = Utc::now();
    ContactIndex {
        id: id.to_string(),
        name: name.to_string(),
        aliases: "[]".to_string(),
        is_self: false,
        created_at: last_interaction,
        last_interaction_at: last_interaction,
        related_contact_ids: "[]".to_string(),
        relationship_tags: "[]".to_string(),
        relation_weight: String::new(),
        background: String::new(),
        notes: String::new(),
        owner_type: OwnerType::User,
        owner_id: "contact_archive_user".to_string(),
        is_agent: false,
        parent_agent_id: None,
        is_virtual: false,
        is_archived: false,
        archived_at: None,
        activity_score: 0.0,
    }
}

async fn insert_contact_raw(pool: &SqlitePool, id: &str, name: &str, last_interaction: &str) {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO contact_index (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, relationship_tags, relation_weight, background, notes, owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived) VALUES (?, ?, '[]', 0, ?, ?, '[]', '[]', '', '', '', 'User', 'contact_archive_user', 0, NULL, 0, 0)"
    )
    .bind(id)
    .bind(name)
    .bind(&now)
    .bind(last_interaction)
    .execute(pool)
    .await
    .unwrap();
}

// ==================== 一、归档测试 ====================

/// 1.1: 超过 180 天未联系的联系人被归档
#[tokio::test]
async fn test_archive_old_contact() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);

    let contact = make_contact("test-old", "旧联系人", old_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();

    // 归档前验证
    let row = sqlx::query("SELECT is_archived FROM contact_index WHERE id = ?")
        .bind("test-old")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let is_archived: i64 = row.get("is_archived");
    assert_eq!(is_archived, 0, "归档前 is_archived 应为 0");

    // 触发归档
    let count = ctx.store.archive_stale_contacts(1000).await.unwrap();
    assert_eq!(count, 1, "应归档 1 个联系人");

    // 验证归档后
    let row = sqlx::query("SELECT is_archived, archived_at FROM contact_index WHERE id = ?")
        .bind("test-old")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let archived: i64 = row.get("is_archived");
    let archived_at: Option<String> = row.get("archived_at");
    assert_eq!(archived, 1, "归档后 is_archived 应为 1");
    assert!(archived_at.is_some(), "archived_at 不应为空");
}

/// 1.2: 未满 180 天不应被归档
#[tokio::test]
async fn test_no_archive_recent_contact() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let recent_time = now - Duration::days(30);

    let contact = make_contact("test-recent", "最近联系人", recent_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();

    let count = ctx.store.archive_stale_contacts(1000).await.unwrap();
    assert_eq!(count, 0, "30 天前不应被归档");

    let row = sqlx::query("SELECT is_archived FROM contact_index WHERE id = ?")
        .bind("test-recent")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let is_archived: i64 = row.get("is_archived");
    assert_eq!(is_archived, 0, "最近联系人的 is_archived 应为 0");
}

/// 1.3: 批量归档，遵守 batch_limit
#[tokio::test]
async fn test_batch_archive_limit() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = (now - Duration::days(200)).to_rfc3339();

    // 创建 150 个旧联系人
    for i in 0..150 {
        insert_contact_raw(
            &ctx.pool,
            &format!("old-batch-{}", i),
            &format!("旧联系人 {}", i),
            &old_time,
        ).await;
    }

    // batch_limit = 100
    let count = ctx.store.archive_stale_contacts(100).await.unwrap();
    assert!(count <= 100, "单次归档不应超过 100");

    // 再次归档剩余
    let count2 = ctx.store.archive_stale_contacts(100).await.unwrap();
    assert_eq!(count + count2, 150, "两次归档应覆盖全部 150 个");

    // 验证全部已归档
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM contact_index WHERE is_archived = 1 AND id LIKE 'old-batch-%'"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 150, "全部 150 个应被归档");
}

/// 1.4: last_interaction_at 在 180 天内联系人不被归档
/// 注意: 当前 schema 中 last_interaction_at 为 NOT NULL，不支持真正的 NULL 值。
/// archive_stale_contacts 的 SQL 中已包含 `last_interaction_at IS NOT NULL` 保护，
/// 如果未来 schema 允许 NULL，NULL 值也会被正确跳过。
#[tokio::test]
async fn test_no_archive_within_threshold() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    // 使用 150 天前（在 180 天阈值内），验证不被归档
    let within_threshold = (now - chrono::Duration::days(150)).to_rfc3339();

    sqlx::query(
        "INSERT OR IGNORE INTO contact_index (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, relationship_tags, relation_weight, background, notes, owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived) VALUES (?, ?, '[]', 0, ?, ?, '[]', '[]', '', '', '', 'User', 'contact_archive_user', 0, NULL, 0, 0)"
    )
    .bind("within-threshold")
    .bind("阈值内联系人")
    .bind(&within_threshold)
    .bind(&within_threshold)
    .execute(&ctx.pool)
    .await
    .unwrap();

    let count = ctx.store.archive_stale_contacts(1000).await.unwrap();
    assert_eq!(count, 0, "阈值内联系人不应被归档");

    let row: (i64,) = sqlx::query_as(
        "SELECT is_archived FROM contact_index WHERE id = 'within-threshold'"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 0, "is_archived 应为 0");
}

// ==================== 二、查询过滤测试 ====================

/// 2.1: 默认搜索不返回归档联系人
#[tokio::test]
async fn test_default_search_excludes_archived() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);

    // 归档联系人
    let contact = make_contact("archived-search", "旧联系人搜索", old_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();
    ctx.store.archive_stale_contacts(1000).await.unwrap();

    // 默认搜索
    let results = ctx.store.search_contacts_with_owner(
        "旧联系人", OwnerType::User, "contact_archive_user", 10, false,
    ).await.unwrap();
    assert_eq!(results.len(), 0, "默认搜索不应返回归档联系人");
}

/// 2.2: include_archived 搜索返回归档联系人
#[tokio::test]
async fn test_include_archived_search() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);

    let contact = make_contact("archived-include", "旧联系人", old_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();
    ctx.store.archive_stale_contacts(1000).await.unwrap();

    let results = ctx.store.search_contacts_with_owner(
        "旧联系人", OwnerType::User, "contact_archive_user", 10, true,
    ).await.unwrap();
    assert_eq!(results.len(), 1, "include_archived=true 应返回归档联系人");
    assert_eq!(results[0].contact_id, "archived-include");
}

/// 2.3: 混合搜索结果只返回热联系人
#[tokio::test]
async fn test_mixed_search_hot_only() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);
    let recent_time = now - Duration::days(10);

    // 归档联系人
    let old = make_contact("old-mixed", "张三旧", old_time);
    ctx.store.upsert_contact_index(&old).await.unwrap();
    ctx.store.archive_stale_contacts(1000).await.unwrap();

    // 热联系人
    let recent = make_contact("new-mixed", "张三新", recent_time);
    ctx.store.upsert_contact_index(&recent).await.unwrap();

    // 默认搜索 "张"
    let results = ctx.store.search_contacts_with_owner(
        "张", OwnerType::User, "contact_archive_user", 10, false,
    ).await.unwrap();
    assert_eq!(results.len(), 1, "默认只返回热联系人");
    assert_eq!(results[0].name, "张三新");

    // include_archived 返回全部
    let all = ctx.store.search_contacts_with_owner(
        "张", OwnerType::User, "contact_archive_user", 10, true,
    ).await.unwrap();
    assert_eq!(all.len(), 2, "include_archived=true 应返回全部 2 个");
}

// ==================== 三、UPSERT 保护测试 ====================

/// 3.1: 归档联系人重新出现时 is_archived 不被重置
#[tokio::test]
async fn test_upsert_preserves_archived_status() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);

    // 先创建并归档
    let contact = make_contact("archived-upsert", "旧联系人", old_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();
    ctx.store.archive_stale_contacts(1000).await.unwrap();

    // 确认已归档
    let row: (i64,) = sqlx::query_as("SELECT is_archived FROM contact_index WHERE id = 'archived-upsert'")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    assert_eq!(row.0, 1, "归档后 is_archived 应为 1");

    // 模拟联系人再次被提及：upsert
    let new_time = Utc::now();
    let updated = ContactIndex {
        id: "archived-upsert".to_string(),
        name: "旧联系人".to_string(),
        aliases: "[]".to_string(),
        is_self: false,
        created_at: old_time,
        last_interaction_at: new_time,  // 更新为当前时间
        related_contact_ids: "[]".to_string(),
        relationship_tags: "[]".to_string(),
        relation_weight: String::new(),
        background: String::new(),
        notes: String::new(),
        owner_type: OwnerType::User,
        owner_id: "contact_archive_user".to_string(),
        is_agent: false,
        parent_agent_id: None,
        is_virtual: false,
        is_archived: false,  // 注意：传入 false，但 upsert 不应覆盖
        archived_at: None,
        activity_score: 0.0,
    };
    ctx.store.upsert_contact_index(&updated).await.unwrap();

    // 验证 is_archived 保持 1
    let row = sqlx::query("SELECT is_archived, last_interaction_at FROM contact_index WHERE id = ?")
        .bind("archived-upsert")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let is_archived: i64 = row.get("is_archived");
    let last_ia: String = row.get("last_interaction_at");

    assert_eq!(is_archived, 1, "is_archived 应保持为 1（归档状态）");
    // last_interaction_at 应被更新为最新时间
    assert!(last_ia.contains(&new_time.format("%Y-%m-%d").to_string()),
        "last_interaction_at 应被更新为当前时间");
}

/// 3.1 扩展: name 等其他字段可正常更新
#[tokio::test]
async fn test_upsert_updates_other_fields() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = now - Duration::days(200);

    let contact = make_contact("name-update", "原始名", old_time);
    ctx.store.upsert_contact_index(&contact).await.unwrap();
    ctx.store.archive_stale_contacts(1000).await.unwrap();

    // upsert 修改 name
    let updated = ContactIndex {
        id: "name-update".to_string(),
        name: "更新后的名字".to_string(),
        aliases: "[]".to_string(),
        is_self: false,
        created_at: old_time,
        last_interaction_at: now,
        related_contact_ids: "[]".to_string(),
        relationship_tags: "[]".to_string(),
        relation_weight: String::new(),
        background: "新背景".to_string(),
        notes: "新备注".to_string(),
        owner_type: OwnerType::User,
        owner_id: "contact_archive_user".to_string(),
        is_agent: false,
        parent_agent_id: None,
        is_virtual: false,
        is_archived: false,
        archived_at: None,
        activity_score: 0.0,
    };
    ctx.store.upsert_contact_index(&updated).await.unwrap();

    let row = sqlx::query("SELECT name, is_archived, background, notes FROM contact_index WHERE id = ?")
        .bind("name-update")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let name: String = row.get("name");
    let is_archived: i64 = row.get("is_archived");
    let background: String = row.get("background");

    assert_eq!(name, "更新后的名字", "name 应被更新");
    assert_eq!(is_archived, 1, "is_archived 应保持 1");
    assert_eq!(background, "新背景", "background 应被更新");
}

/// 3.2: 新联系人正常创建，is_archived=0
#[tokio::test]
async fn test_new_contact_is_archived_zero() {
    let ctx = create_test_store().await;
    let now = Utc::now();

    let contact = make_contact("new-contact", "新联系人", now);
    ctx.store.upsert_contact_index(&contact).await.unwrap();

    let row = sqlx::query("SELECT is_archived FROM contact_index WHERE id = ?")
        .bind("new-contact")
        .fetch_one(&ctx.pool)
        .await
        .unwrap();
    let is_archived: i64 = row.get("is_archived");
    assert_eq!(is_archived, 0, "新联系人的 is_archived 应为 0");
}

// ==================== 四、隔离测试 ====================

/// 不同 owner 独立归档
#[tokio::test]
async fn test_archive_by_owner() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = (now - Duration::days(200)).to_rfc3339();
    let old_time2 = (now - Duration::days(200)).to_rfc3339();

    // 不同 owner 的旧联系人
    sqlx::query(
        "INSERT OR IGNORE INTO contact_index (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, relationship_tags, relation_weight, background, notes, owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived) VALUES (?, ?, '[]', 0, ?, ?, '[]', '[]', '', '', '', 'User', ?, 0, NULL, 0, 0)"
    )
    .bind("owner-a")
    .bind("用户A联系人")
    .bind(&old_time)
    .bind(&old_time)
    .bind("user_a")
    .execute(&ctx.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT OR IGNORE INTO contact_index (id, name, aliases, is_self, created_at, last_interaction_at, related_contact_ids, relationship_tags, relation_weight, background, notes, owner_type, owner_id, is_agent, parent_agent_id, is_virtual, is_archived) VALUES (?, ?, '[]', 0, ?, ?, '[]', '[]', '', '', '', 'User', ?, 0, NULL, 0, 0)"
    )
    .bind("owner-b")
    .bind("用户B联系人")
    .bind(&old_time2)
    .bind(&old_time2)
    .bind("user_b")
    .execute(&ctx.pool)
    .await
    .unwrap();

    // 归档（全局扫描，不区分 owner）
    let count = ctx.store.archive_stale_contacts(1000).await.unwrap();
    assert_eq!(count, 2, "应归档两个用户的联系人");

    // 两个都已被归档
    for id in &["owner-a", "owner-b"] {
        let row: (i64,) = sqlx::query_as("SELECT is_archived FROM contact_index WHERE id = ?")
            .bind(id)
            .fetch_one(&ctx.pool)
            .await
            .unwrap();
        assert_eq!(row.0, 1, "{} 应被归档", id);
    }
}

/// 搜索按 owner 隔离
#[tokio::test]
async fn test_search_by_owner() {
    let ctx = create_test_store().await;
    let now = Utc::now();

    // user_a 的联系人
    let ca = make_contact("ca-user", "共享名", now);
    let mut ca_a = ca.clone();
    ca_a.owner_id = "user_a".to_string();
    ctx.store.upsert_contact_index(&ca_a).await.unwrap();

    // user_b 的联系人（同名）
    let mut ca_b = ca.clone();
    ca_b.id = "ca-user-b".to_string();
    ca_b.owner_id = "user_b".to_string();
    ctx.store.upsert_contact_index(&ca_b).await.unwrap();

    // user_a 搜索
    let results_a = ctx.store.search_contacts_with_owner(
        "共享名", OwnerType::User, "user_a", 10, false,
    ).await.unwrap();
    assert_eq!(results_a.len(), 1, "user_a 只应找到自己的");
    assert_eq!(results_a[0].contact_id, "ca-user");

    // user_b 搜索
    let results_b = ctx.store.search_contacts_with_owner(
        "共享名", OwnerType::User, "user_b", 10, false,
    ).await.unwrap();
    assert_eq!(results_b.len(), 1, "user_b 只应找到自己的");
    assert_eq!(results_b[0].contact_id, "ca-user-b");
}

// ==================== 五、性能测试 ====================

/// 批量归档性能
#[tokio::test]
async fn test_archive_performance() {
    let ctx = create_test_store().await;
    let now = Utc::now();
    let old_time = (now - Duration::days(200)).to_rfc3339();

    // 创建 2000 个旧联系人
    let batch_start = std::time::Instant::now();
    for i in 0..2000 {
        insert_contact_raw(
            &ctx.pool,
            &format!("perf-{}", i),
            &format!("性能联系人 {}", i),
            &old_time,
        ).await;
    }
    let insert_time = batch_start.elapsed();

    // 归档（batch_limit=1000）
    let arch_start = std::time::Instant::now();
    let count1 = ctx.store.archive_stale_contacts(1000).await.unwrap();
    let arch1_time = arch_start.elapsed();

    let arch2_start = std::time::Instant::now();
    let count2 = ctx.store.archive_stale_contacts(1000).await.unwrap();
    let arch2_time = arch2_start.elapsed();

    let total_time = batch_start.elapsed();

    println!("=== Contact Archive Performance ===");
    println!("Insert 2000 contacts: {:?}", insert_time);
    println!("Archive batch 1 ({} contacts): {:?}", count1, arch1_time);
    println!("Archive batch 2 ({} contacts): {:?}", count2, arch2_time);
    println!("Total: {:?}", total_time);

    assert_eq!(count1, 1000, "第一批应归档 1000");
    assert_eq!(count2, 1000, "第二批应归档 1000");

    // 归档操作本身应在毫秒级完成
    assert!(arch1_time.as_millis() < 500, "首批 1000 条归档应 < 500ms: {:?}", arch1_time);
    assert!(arch2_time.as_millis() < 500, "次批 1000 条归档应 < 500ms: {:?}", arch2_time);
}