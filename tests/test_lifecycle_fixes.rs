//! 针对模拟用户测试（S4 / S6 / S7）暴露问题的回归测试
//!
//! - S4：无证据联系人被反复选中，占住每轮 3 个画像名额
//! - S6：待确认合并只有硬删除、且 48h 判断受时间字符串格式影响
//! - S7：没有任何在线备份能力

use chrono::{Duration, Utc};
use shouxin::stores::relational::RelationalStore;
use sqlx::SqlitePool;
use tempfile::TempDir;

async fn store() -> (RelationalStore, TempDir) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fixes.db");
    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    let s = RelationalStore::new(pool);
    s.init_schema().await.unwrap();
    (s, dir)
}

async fn insert_contact(store: &RelationalStore, id: &str, name: &str, last_interaction: chrono::DateTime<Utc>) {
    sqlx::query(
        "INSERT INTO contact_index
         (id, name, aliases, is_self, created_at, last_interaction_at, owner_type, owner_id,
          is_archived, background, activity_score)
         VALUES (?, ?, '[]', 0, ?, ?, 'User', 'u1', 0, NULL, 0.0)",
    )
    .bind(id)
    .bind(name)
    .bind(last_interaction.to_rfc3339())
    .bind(last_interaction.to_rfc3339())
    .execute(store.pool())
    .await
    .unwrap();
}

/// S4：跳过时写一条 0 额度记录，联系人退出候选队列；有新互动后重新入队
#[tokio::test]
async fn test_profile_skip_stops_reselection() {
    let (s, _tmp) = store().await;
    let now = Utc::now();
    insert_contact(&s, "c1", "张三", now - Duration::seconds(30)).await;

    let first = s.get_contacts_needing_profile(2).await.unwrap();
    assert_eq!(first.len(), 1, "从未检查过 → 应入队");

    s.record_profile_skip("c1").await.unwrap();

    let second = s.get_contacts_needing_profile(2).await.unwrap();
    assert!(second.is_empty(), "跳过并记账后应退出队列，不再每轮占名额");

    // 额度不被消耗：仍可查（这里只验证日志计数是 0）
    let log: (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(update_count_today),0) FROM contact_profile_update_log WHERE contact_id='c1'",
    )
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(log.0, 0, "跳过不应消耗每日 2 次额度");

    // 有新互动 → 重新入队
    sqlx::query("UPDATE contact_index SET last_interaction_at=? WHERE id='c1'")
        .bind((now + Duration::seconds(5)).to_rfc3339())
        .execute(s.pool())
        .await
        .unwrap();
    let third = s.get_contacts_needing_profile(2).await.unwrap();
    assert_eq!(third.len(), 1, "有新互动后应重新入队");
}

/// S6：48h 到期标记为 expired（不再硬删），且 49 小时的行也能被正确判定
#[tokio::test]
async fn test_pending_merge_expires_instead_of_delete() {
    let (s, _tmp) = store().await;
    let now = Utc::now();

    for (id, hours) in [("m49", 49i64), ("m1", 1)] {
        sqlx::query(
            "INSERT INTO pending_event_merge
             (source_event_id, target_event_id, source_event_summary, target_event_summary,
              similarity_score, status, created_at, owner_type, owner_id)
             VALUES (?, 'tgt', 'a', 'b', 0.85, 'pending', ?, 'User', 'u1')",
        )
        .bind(id)
        .bind((now - Duration::hours(hours)).to_rfc3339())
        .execute(s.pool())
        .await
        .unwrap();
    }

    let n = s.cleanup_expired_pending_merges().await.unwrap();
    assert_eq!(n, 1, "只应处理 49 小时那条");

    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT source_event_id, status, resolution_action FROM pending_event_merge ORDER BY source_event_id",
    )
    .fetch_all(s.pool())
    .await
    .unwrap();

    assert_eq!(rows[0].0, "m1");
    assert_eq!(rows[0].1, "pending", "未到期的不能动");
    assert_eq!(rows[1].0, "m49");
    assert_eq!(rows[1].1, "expired", "到期应标记 expired 而不是删除");
    assert_eq!(rows[1].2.as_deref(), Some("timeout"));
}

/// S7：在线快照可用且完整（WAL 下不丢最近写入）
#[tokio::test]
async fn test_snapshot_is_valid() {
    let (s, tmp) = store().await;
    insert_contact(&s, "c1", "李四", Utc::now()).await;

    let snap = tmp.path().join("snapshot.db");
    let size = s
        .snapshot_to(snap.to_str().unwrap())
        .await
        .expect("快照应成功");
    assert!(size > 0, "快照文件不应为空");

    let pool = SqlitePool::connect(&format!("sqlite://{}?mode=ro", snap.display()))
        .await
        .unwrap();
    let integrity: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(integrity.0, "ok");

    let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM contact_index WHERE id='c1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n.0, 1, "快照应包含最近写入的数据");
}
