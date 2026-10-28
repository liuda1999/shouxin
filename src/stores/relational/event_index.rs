//! 事件索引表操作

use chrono::{DateTime, Duration, Utc};
use sqlx::{query, query_as, Pool, Sqlite};

use crate::error::ShouxinError;
use crate::traits::store_traits::{CleanupRule, EventIndex};

/// 事件索引存储
pub struct EventIndexStore {
    pool: Pool<Sqlite>,
    cleanup_rule: CleanupRule,
}

impl EventIndexStore {
    pub fn new(pool: Pool<Sqlite>, cleanup_rule: CleanupRule) -> Self {
        Self { pool, cleanup_rule }
    }

    /// 清理旧事件
    async fn cleanup_old_events(&self) -> Result<usize, ShouxinError> {
        let mut cleaned = 0;

        match &self.cleanup_rule {
            CleanupRule::TimeBased(days) => {
                let cutoff = Utc::now() - Duration::days(*days as i64);
                let result = query("DELETE FROM event_indices WHERE event_time < ?")
                    .bind(cutoff.to_rfc3339())
                    .execute(&self.pool)
                    .await?;
                cleaned = result.rows_affected() as usize;
            }
            CleanupRule::CountBased(max_count) => {
                let count_row: (i64,) = query_as("SELECT COUNT(*) FROM event_indices")
                    .fetch_one(&self.pool)
                    .await?;

                if count_row.0 > *max_count as i64 {
                    let to_delete = count_row.0 - *max_count as i64;
                    let result = query(
                        r#"
                        DELETE FROM event_indices
                        WHERE event_id IN (
                            SELECT event_id FROM event_indices
                            ORDER BY event_time ASC
                            LIMIT ?
                        )
                        "#,
                    )
                    .bind(to_delete)
                    .execute(&self.pool)
                    .await?;
                    cleaned = result.rows_affected() as usize;
                }
            }
            CleanupRule::Hybrid { days, max_count } => {
                let cutoff = Utc::now() - Duration::days(*days as i64);

                let result = query("DELETE FROM event_indices WHERE event_time < ?")
                    .bind(cutoff.to_rfc3339())
                    .execute(&self.pool)
                    .await?;
                cleaned = result.rows_affected() as usize;

                let count_row: (i64,) = query_as("SELECT COUNT(*) FROM event_indices")
                    .fetch_one(&self.pool)
                    .await?;

                if count_row.0 > *max_count as i64 {
                    let to_delete = count_row.0 - *max_count as i64;
                    let result = query(
                        r#"
                        DELETE FROM event_indices
                        WHERE event_id IN (
                            SELECT event_id FROM event_indices
                            ORDER BY event_time ASC
                            LIMIT ?
                        )
                        "#,
                    )
                    .bind(to_delete)
                    .execute(&self.pool)
                    .await?;
                    cleaned += result.rows_affected() as usize;
                }
            }
        }

        Ok(cleaned)
    }
}

impl EventIndexStore {
    /// 添加事件索引
    pub async fn add_event_index(&self, event: EventIndex) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT INTO event_indices 
            (event_id, event_type, user_id, agent_id, event_time, archive_id, memory_id, importance, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&event.event_id)
        .bind(&event.event_type)
        .bind(&event.user_id)
        .bind(&event.agent_id)
        .bind(event.event_time.to_rfc3339())
        .bind(&event.archive_id)
        .bind(&event.memory_id)
        .bind(event.importance)
        .bind(event.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// 查询事件索引
    pub async fn query_events(
        &self,
        user_id: Option<&str>,
        event_type: Option<&str>,
        start_time: Option<DateTime<Utc>>,
        end_time: Option<DateTime<Utc>>,
        limit: usize,
    ) -> Result<Vec<EventIndex>, ShouxinError> {
        let mut sql = String::from(
            "SELECT event_id, event_type, user_id, agent_id, event_time, archive_id, memory_id, importance, created_at 
             FROM event_indices WHERE 1=1",
        );

        if user_id.is_some() {
            sql.push_str(" AND user_id = ?");
        }

        if event_type.is_some() {
            sql.push_str(" AND event_type = ?");
        }

        if start_time.is_some() {
            sql.push_str(" AND event_time >= ?");
        }

        if end_time.is_some() {
            sql.push_str(" AND event_time <= ?");
        }

        sql.push_str(" ORDER BY event_time DESC LIMIT ?");

        let mut q = query(&sql);

        if let Some(uid) = user_id {
            q = q.bind(uid);
        }

        if let Some(et) = event_type {
            q = q.bind(et);
        }

        if let Some(st) = start_time {
            q = q.bind(st.to_rfc3339());
        }

        if let Some(et) = end_time {
            q = q.bind(et.to_rfc3339());
        }

        q = q.bind(limit as i64);

        let rows = q.fetch_all(&self.pool).await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(EventIndex::from_row(&row)?);
        }

        Ok(events)
    }

    /// 根据事件 ID 获取索引
    pub async fn get_event_index(&self, event_id: &str) -> Result<Option<EventIndex>, ShouxinError> {
        let row = query(
            "SELECT event_id, event_type, user_id, agent_id, event_time, archive_id, memory_id, importance, created_at 
             FROM event_indices WHERE event_id = ?",
        )
        .bind(event_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|r| EventIndex::from_row(&r).ok()))
    }

    /// 执行清理
    pub async fn cleanup(&self) -> Result<usize, ShouxinError> {
        self.cleanup_old_events().await
    }
}
