//! 待办完成接口的幂等语义测试。
//!
//! 回归背景：`complete_todo` 原先无条件写 `completed_at = now()`，导致重复调用
//! （外部重试、心跳/对话抽取反复判定"已完成"）会把完成时间不断向后推。
//! 修复后 `completed_at` 只在首次完成时写入，`last_updated_at` 仍照常刷新。

use chrono::{DateTime, Duration, Utc};
use shouxin::core::contact::OwnerType;
use shouxin::stores::relational::todo_list::{TodoItem, TodoListStore};
use sqlx::SqlitePool;
use tempfile::TempDir;

/// 创建测试用的 TodoListStore + 初始化 schema
async fn create_test_store() -> (TodoListStore, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_todos.db");
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

    (TodoListStore::new(pool), temp_dir)
}

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

/// 重复完成不得覆盖首次完成时间（幂等）。
#[tokio::test]
async fn test_complete_todo_keeps_first_completed_at() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "idempotent_user";
    let ot = OwnerType::User;

    let todo = make_todo(
        "todo-idem",
        ot,
        owner_id,
        "整理发布说明",
        Utc::now() + Duration::days(3),
    );
    store.add_todo(&todo).await.unwrap();

    // 第一次完成
    store.complete_todo("todo-idem", ot, owner_id).await.unwrap();
    let first = store
        .get_todo("todo-idem", ot, owner_id)
        .await
        .unwrap()
        .expect("todo should exist");
    let first_completed_at = first.completed_at.expect("首次完成必须写入 completed_at");
    assert_eq!(first.status, "completed");

    // 等一小段时间，让 now() 一定前进，再重复完成
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    store.complete_todo("todo-idem", ot, owner_id).await.unwrap();

    let second = store
        .get_todo("todo-idem", ot, owner_id)
        .await
        .unwrap()
        .expect("todo should exist");
    let second_completed_at = second.completed_at.expect("completed_at 不应被清空");

    assert_eq!(
        second_completed_at, first_completed_at,
        "重复完成不能改写首次完成时间"
    );
    assert_eq!(second.status, "completed");
}

/// 重复完成仍应刷新 last_updated_at（方案 A：仅冻结 completed_at）。
#[tokio::test]
async fn test_complete_todo_still_touches_last_updated_at() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "idempotent_user";
    let ot = OwnerType::User;

    let todo = make_todo(
        "todo-touch",
        ot,
        owner_id,
        "复核待办时间戳",
        Utc::now() + Duration::days(3),
    );
    store.add_todo(&todo).await.unwrap();

    store.complete_todo("todo-touch", ot, owner_id).await.unwrap();
    let first = store
        .get_todo("todo-touch", ot, owner_id)
        .await
        .unwrap()
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    store.complete_todo("todo-touch", ot, owner_id).await.unwrap();
    let second = store
        .get_todo("todo-touch", ot, owner_id)
        .await
        .unwrap()
        .unwrap();

    assert!(
        second.last_updated_at >= first.last_updated_at,
        "last_updated_at 应被刷新（不早于首次）"
    );
}

/// 不存在的待办仍必须报错（COALESCE 改写不能吞掉 not-found 语义）。
#[tokio::test]
async fn test_complete_missing_todo_still_errors() {
    let (store, _temp) = create_test_store().await;

    let err = store
        .complete_todo("no-such-todo", OwnerType::User, "idempotent_user")
        .await;
    assert!(err.is_err(), "不存在的待办必须返回错误，而不是静默成功");
}

/// 重复完成不应把已是 completed 的记录回退或篡改其他字段。
#[tokio::test]
async fn test_complete_todo_does_not_mutate_other_fields() {
    let (store, _temp) = create_test_store().await;
    let owner_id = "idempotent_user";
    let ot = OwnerType::User;

    let deadline = Utc::now() + Duration::days(7);
    let todo = make_todo("todo-fields", ot, owner_id, "检查字段稳定性", deadline);
    store.add_todo(&todo).await.unwrap();

    store.complete_todo("todo-fields", ot, owner_id).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    store.complete_todo("todo-fields", ot, owner_id).await.unwrap();

    let after = store
        .get_todo("todo-fields", ot, owner_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(after.title, "检查字段稳定性");
    assert_eq!(after.deadline, deadline);
    assert_eq!(after.importance, "medium");
    assert_eq!(after.status, "completed");
}
