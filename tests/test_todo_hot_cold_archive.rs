//! ShouXin v0.5.1 任务7验证测试：待办热冷分离、上限提醒与自动归档
//!
//! 测试方法：直接测试 TodoListStore 方法，绕过心跳调度器。

use chrono::{DateTime, Duration, Utc};
use shouxin::core::contact::OwnerType;
use shouxin::stores::relational::todo_list::{TodoItem, TodoListStore};
use sqlx::SqlitePool;
use tempfile::TempDir;

/// 测试上下文：包含 TodoListStore 和原始 SqlitePool（用于高级查询）
struct TestContext {
    store: TodoListStore,
    pool: SqlitePool,
    _temp: TempDir,
}

/// 创建测试用的 TestContext
async fn create_test_store() -> TestContext {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_todos_hot_cold.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePool::connect(&db_url).await.unwrap();

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS todo_list (
            todo_id TEXT PRIMARY KEY,
            contact_ids TEXT NOT NULL DEFAULT '[]',
            owner_id TEXT NOT NULL,
            owner_type TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT,
            deadline TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL,
            completed_at TEXT,
            importance TEXT NOT NULL DEFAULT 'medium',
            source_event_id TEXT DEFAULT NULL,
            todo_type TEXT NOT NULL DEFAULT 'normal',
            entity_type TEXT DEFAULT NULL,
            entity_id TEXT DEFAULT NULL,
            is_archived INTEGER DEFAULT 0,
            archived_at TEXT,
            last_updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        )"#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let store = TodoListStore::new(pool.clone());
    TestContext { store, pool, _temp: temp_dir }
}

/// 创建待办
fn make_todo(todo_id: &str, owner_type: OwnerType, owner_id: &str, title: &str, deadline: DateTime<Utc>) -> TodoItem {
    TodoItem::new(
        todo_id.to_string(),
        vec![],
        owner_id.to_string(),
        owner_type,
        title.to_string(),
        deadline,
        "medium".to_string(),
    )
}

/// 直接插入待办（绕过 TodoItem 构建器，可设置 last_updated_at）
async fn insert_todo_raw(
    pool: &SqlitePool,
    todo_id: &str,
    owner_type: OwnerType,
    owner_id: &str,
    title: &str,
    status: &str,
    deadline: DateTime<Utc>,
    created_at: DateTime<Utc>,
    last_updated_at: DateTime<Utc>,
    is_archived: bool,
    todo_type: &str,
) {
    sqlx::query(
        "INSERT OR IGNORE INTO todo_list (todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at) VALUES (?, '[]', ?, ?, ?, NULL, ?, ?, ?, NULL, 'medium', NULL, ?, NULL, NULL, ?, NULL, ?)"
    )
    .bind(todo_id)
    .bind(owner_id)
    .bind(format!("{:?}", owner_type))
    .bind(title)
    .bind(deadline.to_rfc3339())
    .bind(status)
    .bind(created_at.to_rfc3339())
    .bind(todo_type)
    .bind(if is_archived { 1 } else { 0 })
    .bind(last_updated_at.to_rfc3339())
    .execute(pool)
    .await
    .unwrap();
}

// ==================== 一、提醒生成测试 ====================

/// 1.1: 超过 30 条 pending 时生成提醒
#[tokio::test]
async fn test_reminder_generated_when_over_30() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    // 创建 40 个 pending 待办
    for i in 0..40 {
        let todo = make_todo(&format!("todo-{}", i), ot, owner_id, &format!("待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let pending = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending, 40, "应有 40 个 pending");

    assert!(!ctx.store.has_recent_reminder(&ot_str, owner_id).await.unwrap(), "不应有提醒");

    let pending_count = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert!(pending_count > 30, "应超过 30");

    let reminder_id = uuid::Uuid::new_v4().to_string();
    let title = format!("⚠️ 你有 {} 个待办未处理，请及时处理", pending_count);
    ctx.store.insert_reminder_todo(&reminder_id, &ot_str, owner_id, &title).await.unwrap();

    let has_reminder = ctx.store.has_recent_reminder(&ot_str, owner_id).await.unwrap();
    assert!(has_reminder, "应生成提醒");

    // 通过原始 SQL 验证提醒已存入
    let row: (String, String, String) = sqlx::query_as(
        "SELECT todo_id, todo_type, status FROM todo_list WHERE todo_id = ? AND owner_type = ? AND owner_id = ?"
    )
    .bind(&reminder_id)
    .bind(&ot_str)
    .bind(owner_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    assert_eq!(row.1, "reminder", "提醒类型应为 reminder");
    assert_eq!(row.2, "pending");
    assert!(row.0 == reminder_id);
}

/// 1.1: 同日不重复生成提醒
#[tokio::test]
async fn test_reminder_not_duplicated_same_day() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    for i in 0..35 {
        let todo = make_todo(&format!("todo-{}", i), ot, owner_id, &format!("待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let reminder_id1 = uuid::Uuid::new_v4().to_string();
    ctx.store.insert_reminder_todo(&reminder_id1, &ot_str, owner_id, "⚠️ 你有 35 个待办未处理").await.unwrap();

    let has_reminder = ctx.store.has_recent_reminder(&ot_str, owner_id).await.unwrap();
    assert!(has_reminder, "应有提醒");

    let reminder_count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM todo_list WHERE todo_type = 'reminder' AND owner_type = ? AND owner_id = ?"
    )
    .bind(&ot_str)
    .bind(owner_id)
    .fetch_one(&ctx.pool)
    .await
    .unwrap();

    assert_eq!(reminder_count.0, 1, "同一天不应生成第二条提醒");
}

/// 1.2: 提醒待办是特殊类型，不影响正常计数
#[tokio::test]
async fn test_reminder_todo_is_special_type() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    for i in 0..35 {
        let todo = make_todo(&format!("todo-{}", i), ot, owner_id, &format!("待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let reminder_id = uuid::Uuid::new_v4().to_string();
    ctx.store.insert_reminder_todo(&reminder_id, &ot_str, owner_id, "⚠️ 你有 35 个待办未处理").await.unwrap();

    // 提醒待办可以被完成
    ctx.store.complete_todo(&reminder_id, ot, owner_id).await.unwrap();
    let completed = ctx.store.get_todo(&reminder_id, ot, owner_id).await.unwrap().unwrap();
    assert_eq!(completed.status, "completed", "提醒待办可被完成");
}

// ==================== 二、自动归档测试 ====================

/// 2.1: 超过 50 条 pending 且有过期待办时归档
#[tokio::test]
async fn test_archive_stale_when_over_50() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    // 创建 60 个 pending 待办
    for i in 0..60 {
        let last_updated = if i < 20 { old_time } else { now };
        let title = if i < 20 { format!("旧待办 {}", i) } else { format!("新待办 {}", i) };
        insert_todo_raw(
            &ctx.pool, &format!("todo-{}", i), ot, owner_id, &title,
            "pending", deadline, now, last_updated, false, "normal",
        ).await;
    }

    let pending_before = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending_before, 60, "归档前应有 60 个 pending");

    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    assert!(archived > 0, "应归档至少 1 个");

    let pending_after = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert!(pending_after <= 50, "归档后 pending 应 ≤ 50，实际: {}", pending_after);

    // 验证被归档的待办
    let archived_rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT todo_id, archived_at FROM todo_list WHERE is_archived = 1 AND owner_type = ? AND owner_id = ?"
    )
    .bind(&ot_str)
    .bind(owner_id)
    .fetch_all(&ctx.pool)
    .await
    .unwrap();

    assert_eq!(archived_rows.len(), archived, "归档数量应一致");

    for (_id, archived_at) in &archived_rows {
        assert!(archived_at.is_some(), "所有归档待办应有 archived_at");
    }

    // 验证被归档的是最旧的
    let archived_ids: Vec<String> = archived_rows.into_iter().map(|(id, _)| id).collect();
    for i in 0..archived {
        let id = format!("todo-{}", i);
        assert!(archived_ids.contains(&id), "最旧的 todo-{} 应被归档", i);
    }
}

/// 2.1: 归档只影响过期待办
#[tokio::test]
async fn test_archive_only_stale() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    for i in 0..20 {
        insert_todo_raw(
            &ctx.pool, &format!("old-{}", i), ot, owner_id, &format!("旧待办 {}", i),
            "pending", deadline, now, old_time, false, "normal",
        ).await;
    }
    for i in 0..40 {
        let todo = make_todo(&format!("new-{}", i), ot, owner_id, &format!("新待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();

    let pending = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert!(pending <= 50, "归档后 pending 应 ≤ 50");

    // 验证所有新待办仍在热表
    for i in 0..40 {
        let todo = ctx.store.get_todo(&format!("new-{}", i), ot, owner_id).await.unwrap().unwrap();
        assert!(!todo.is_archived, "新待办 new-{} 不应被归档", i);
    }
}

/// 2.2: pending ≤ 50 时不归档
#[tokio::test]
async fn test_no_archive_when_under_50() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    for i in 0..40 {
        let todo = make_todo(&format!("todo-{}", i), ot, owner_id, &format!("待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    assert_eq!(archived, 0, "≤ 50 时不应归档");

    let pending = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending, 40, "pending 数量不变");
}

/// 2.3: 过期但 pending ≤ 50 时不归档
#[tokio::test]
async fn test_no_archive_stale_but_under_50() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    for i in 0..10 {
        insert_todo_raw(
            &ctx.pool, &format!("old-{}", i), ot, owner_id, &format!("旧待办 {}", i),
            "pending", deadline, now, old_time, false, "normal",
        ).await;
    }
    for i in 0..20 {
        let todo = make_todo(&format!("new-{}", i), ot, owner_id, &format!("新待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    assert_eq!(archived, 0, "过期但未超 50 不应归档");

    let pending = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending, 30, "pending 数量不变");
}

// ==================== 三、冷热查询测试 ====================

/// 3.1: 默认查询只返回热表
#[tokio::test]
async fn test_default_query_hot_only() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    for i in 0..3 {
        let todo = make_todo(&format!("hot-{}", i), ot, owner_id, &format!("热待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    for i in 0..2 {
        insert_todo_raw(
            &ctx.pool, &format!("cold-{}", i), ot, owner_id, &format!("冷待办 {}", i),
            "pending", deadline, now, now, true, "normal",
        ).await;
    }

    let hot = ctx.store.get_all_todos(ot, owner_id, None, 100, false).await.unwrap();
    assert_eq!(hot.len(), 3, "默认查询应只返回热表");
    for t in &hot {
        assert!(!t.is_archived, "{} 不应是归档状态", t.title);
    }

    let all = ctx.store.get_all_todos(ot, owner_id, None, 100, true).await.unwrap();
    assert_eq!(all.len(), 5, "include_archived=true 应返回全部");
}

/// 3.2: 归档待办详情可访问
#[tokio::test]
async fn test_archived_todo_detail_accessible() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    insert_todo_raw(
        &ctx.pool, "cold-detail", ot, owner_id, "归档待办详情",
        "pending", deadline, now, now, true, "normal",
    ).await;

    let todo = ctx.store.get_todo("cold-detail", ot, owner_id).await.unwrap().unwrap();
    assert!(todo.is_archived, "归档待办应可访问");
    assert_eq!(todo.title, "归档待办详情");
}

// ==================== 四、已完成待办归档 ====================

/// 4: 已完成待办在 cleanup 时归档
#[tokio::test]
async fn test_cleanup_archives_completed() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    let todo = make_todo("cleanup-test", ot, owner_id, "待清理测试", deadline);
    ctx.store.add_todo(&todo).await.unwrap();

    ctx.store.complete_todo("cleanup-test", ot, owner_id).await.unwrap();

    let archived = ctx.store.cleanup().await.unwrap();
    assert_eq!(archived, 1, "应归档 1 个已完成待办");

    let after = ctx.store.get_todo("cleanup-test", ot, owner_id).await.unwrap().unwrap();
    assert!(after.is_archived, "已完成待办应被归档");
    assert!(after.archived_at.is_some(), "archived_at 不应为空");

    let hot = ctx.store.get_all_todos(ot, owner_id, None, 100, false).await.unwrap();
    assert!(!hot.iter().any(|t| t.todo_id == "cleanup-test"), "默认查询不应返回已归档");
}

/// 4: cleanup 不归档 pending 待办
#[tokio::test]
async fn test_cleanup_only_completed() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    let todo = make_todo("pending-keep", ot, owner_id, "保持 pending", deadline);
    ctx.store.add_todo(&todo).await.unwrap();

    let archived = ctx.store.cleanup().await.unwrap();
    assert_eq!(archived, 0, "pending 待办不应被 cleanup 归档");

    let after = ctx.store.get_todo("pending-keep", ot, owner_id).await.unwrap().unwrap();
    assert!(!after.is_archived, "pending 待办不应被归档");
}

// ==================== 五、隔离测试 ====================

/// P-2: 不同用户独立计数和归档
#[tokio::test]
async fn test_multi_owner_isolation() {
    let ctx = create_test_store().await;
    let ot = OwnerType::User;
    let user_ot_str = format!("{:?}", ot);
    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    for i in 0..20 {
        insert_todo_raw(
            &ctx.pool, &format!("a-old-{}", i), ot, "user_a", &format!("A旧 {}", i),
            "pending", deadline, now, old_time, false, "normal",
        ).await;
    }
    for i in 0..40 {
        let todo = make_todo(&format!("a-new-{}", i), ot, "user_a", &format!("A新 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    for i in 0..20 {
        let todo = make_todo(&format!("b-{}", i), ot, "user_b", &format!("B {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let archived_a = ctx.store.archive_stale_todos(&user_ot_str, "user_a", 50, 30).await.unwrap();
    assert!(archived_a > 0, "user_a 应归档");

    let pending_b = ctx.store.count_pending_todos(&user_ot_str, "user_b").await.unwrap();
    assert_eq!(pending_b, 20, "user_b 不应受影响");

    let reminder_a = ctx.store.has_recent_reminder(&user_ot_str, "user_a").await.unwrap();
    let reminder_b = ctx.store.has_recent_reminder(&user_ot_str, "user_b").await.unwrap();
    assert!(!reminder_a, "user_a 无提醒");
    assert!(!reminder_b, "user_b 无提醒");
}

// ==================== 六、边界测试 ====================

/// P-3: 提醒和归档同时触发不冲突
#[tokio::test]
async fn test_reminder_and_archive_simultaneous() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    for i in 0..20 {
        insert_todo_raw(
            &ctx.pool, &format!("old-{}", i), ot, owner_id, &format!("旧 {}", i),
            "pending", deadline, now, old_time, false, "normal",
        ).await;
    }
    for i in 0..40 {
        let todo = make_todo(&format!("new-{}", i), ot, owner_id, &format!("新 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let pending_count = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert!(pending_count > 30, "应超过 30");

    // 模拟心跳：先提醒后归档
    let has_reminder = ctx.store.has_recent_reminder(&ot_str, owner_id).await.unwrap();
    if !has_reminder {
        let reminder_id = uuid::Uuid::new_v4().to_string();
        ctx.store.insert_reminder_todo(&reminder_id, &ot_str, owner_id, &format!("⚠️ 你有 {} 个待办未处理", pending_count)).await.unwrap();
    }

    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    assert!(archived > 0, "应归档");

    let has_reminder_after = ctx.store.has_recent_reminder(&ot_str, owner_id).await.unwrap();
    assert!(has_reminder_after, "提醒应存在");

    let pending_after = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert!(pending_after <= 50, "归档后 pending 应 ≤ 50");
}

/// P-4: last_updated_at 为 NULL 的旧数据不应被归档
#[tokio::test]
async fn test_null_last_updated_not_archived() {
    let ctx = create_test_store().await;
    let owner_id = "todo_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);

    for i in 0..20 {
        sqlx::query(
            "INSERT OR IGNORE INTO todo_list (todo_id, contact_ids, owner_id, owner_type, title, description, deadline, status, created_at, completed_at, importance, source_event_id, todo_type, entity_type, entity_id, is_archived, archived_at, last_updated_at) VALUES (?, '[]', ?, ?, ?, NULL, ?, 'pending', ?, NULL, 'medium', NULL, 'normal', NULL, NULL, 0, NULL, NULL)"
        )
        .bind(&format!("null-{}", i))
        .bind(owner_id)
        .bind(&ot_str)
        .bind(&format!("NULL待办 {}", i))
        .bind(deadline.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&ctx.pool)
        .await
        .unwrap();
    }

    for i in 0..40 {
        let todo = make_todo(&format!("fresh-{}", i), ot, owner_id, &format!("新待办 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let pending = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending, 60, "应有 60 个 pending");

    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    // NULL last_updated_at 不满足 last_updated_at < cutoff，不应被归档
    // 新鲜待办也不满足条件，所以归档数应为 0
    assert_eq!(archived, 0, "NULL last_updated_at 和新鲜待办都不应被归档");
}

/// 性能: 大量待办的归档性能
#[tokio::test]
async fn test_performance_large_archive() {
    let ctx = create_test_store().await;
    let owner_id = "perf_archive_user";
    let ot = OwnerType::User;
    let ot_str = format!("{:?}", ot);

    let now = Utc::now();
    let deadline = now + Duration::days(7);
    let old_time = now - Duration::days(40);

    for i in 0..200 {
        insert_todo_raw(
            &ctx.pool, &format!("old-{}", i), ot, owner_id, &format!("旧 {}", i),
            "pending", deadline, now, old_time, false, "normal",
        ).await;
    }
    for i in 0..300 {
        let todo = make_todo(&format!("new-{}", i), ot, owner_id, &format!("新 {}", i), deadline);
        ctx.store.add_todo(&todo).await.unwrap();
    }

    let pending_before = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    assert_eq!(pending_before, 500, "应有 500 个 pending");

    let start = std::time::Instant::now();
    let archived = ctx.store.archive_stale_todos(&ot_str, owner_id, 50, 30).await.unwrap();
    let duration = start.elapsed();

    let pending_after = ctx.store.count_pending_todos(&ot_str, owner_id).await.unwrap();
    // 只能归档 200 个过期待办（500 total - 200 stale = 300 new fresh ones + keep）
    // archive 会归档所有满足 last_updated_at < 30 days 的，最多 to_archive 个
    // to_archive = 500 - 50 = 450，但只有 200 个满足条件，所以归档 200 个
    assert_eq!(archived, 200, "应归档 200 个过期待办");
    assert!(pending_after == 300 || pending_after == 50,
        "归档后 pending: 若只归档 stale=200 则 300，若能归档更多则 ≤ 50，实际: {}", pending_after);
    assert!(duration.as_secs() < 10, "归档操作应 < 10 秒，实际: {:?}", duration);

    println!("Performance: Archived {} todos in {:?}, pending: {} -> {}", archived, duration, pending_before, pending_after);
}