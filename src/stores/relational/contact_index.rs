//! 联系人索引表操作

use chrono::Utc;
use sqlx::{query, Pool, Row, Sqlite};

use crate::error::ShouxinError;
use crate::traits::store_traits::ContactIndex;

/// 联系人索引存储
pub struct ContactIndexStore {
    pool: Pool<Sqlite>,
    max_active_contacts: usize,
}

impl ContactIndexStore {
    pub fn new(pool: Pool<Sqlite>, max_active_contacts: usize) -> Self {
        Self {
            pool,
            max_active_contacts,
        }
    }

    /// 更新联系人排名
    async fn update_contact_ranks(&self, user_id: Option<&str>) -> Result<(), ShouxinError> {
        let mut conn = self.pool.acquire().await?;

        if let Some(uid) = user_id {
            let rows = query(
                r#"
                SELECT contact_id
                FROM contact_indices
                WHERE user_id = ?
                ORDER BY activity_score DESC
                "#,
            )
            .bind(uid)
            .fetch_all(&mut *conn)
            .await?;

            for (i, row) in rows.iter().enumerate() {
                let contact_id: String = row.get("contact_id");
                query("UPDATE contact_indices SET rank = ? WHERE contact_id = ?")
                    .bind((i + 1) as i32)
                    .bind(contact_id)
                    .execute(&mut *conn)
                    .await?;
            }
        } else {
            let rows = query(
                r#"
                SELECT contact_id
                FROM contact_indices
                ORDER BY activity_score DESC
                "#,
            )
            .fetch_all(&mut *conn)
            .await?;

            for (i, row) in rows.iter().enumerate() {
                let contact_id: String = row.get("contact_id");
                query("UPDATE contact_indices SET rank = ? WHERE contact_id = ?")
                    .bind((i + 1) as i32)
                    .bind(contact_id)
                    .execute(&mut *conn)
                    .await?;
            }
        }

        Ok(())
    }

    /// 清理超出限制的联系人
    async fn cleanup_excess_contacts(&self) -> Result<usize, ShouxinError> {
        let max_rank = self.max_active_contacts as i32;

        let result = query("DELETE FROM contact_indices WHERE rank > ?")
            .bind(max_rank)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() as usize)
    }
}

impl ContactIndexStore {
    /// 添加联系人索引
    pub async fn add_contact_index(&self, contact: ContactIndex) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT OR REPLACE INTO contact_indices 
            (contact_id, name, user_id, activity_score, last_activity, rank, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&contact.contact_id)
        .bind(&contact.name)
        .bind(&contact.user_id)
        .bind(contact.activity_score)
        .bind(contact.last_activity.to_rfc3339())
        .bind(contact.rank)
        .bind(contact.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.update_contact_ranks(contact.user_id.as_deref()).await?;
        self.cleanup_excess_contacts().await?;

        Ok(())
    }

    /// 更新联系人活跃度
    pub async fn update_contact_activity(
        &self,
        contact_id: &str,
        delta: f32,
    ) -> Result<(), ShouxinError> {
        let mut conn = self.pool.acquire().await?;

        let row = query("SELECT user_id, activity_score FROM contact_indices WHERE contact_id = ?")
            .bind(contact_id)
            .fetch_optional(&mut *conn)
            .await?;

        if let Some(row) = row {
            let user_id: Option<String> = row.get("user_id");
            let current_score: f32 = row.get("activity_score");
            let new_score = current_score + delta;
            let now = Utc::now();

            query("UPDATE contact_indices SET activity_score = ?, last_activity = ? WHERE contact_id = ?")
                .bind(new_score)
                .bind(now.to_rfc3339())
                .bind(contact_id)
                .execute(&mut *conn)
                .await?;

            self.update_contact_ranks(user_id.as_deref()).await?;
        }

        Ok(())
    }

    /// 获取活跃联系人
    pub async fn get_active_contacts(
        &self,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ContactIndex>, ShouxinError> {
        let rows = if let Some(uid) = user_id {
            query(
                r#"
                SELECT contact_id, name, user_id, activity_score, last_activity, rank, created_at
                FROM contact_indices
                WHERE user_id = ?
                ORDER BY rank ASC
                LIMIT ?
                "#,
            )
            .bind(uid)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query(
                r#"
                SELECT contact_id, name, user_id, activity_score, last_activity, rank, created_at
                FROM contact_indices
                ORDER BY rank ASC
                LIMIT ?
                "#,
            )
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        let mut contacts = Vec::new();
        for row in rows {
            contacts.push(ContactIndex::from_row(&row)?);
        }

        Ok(contacts)
    }

    /// 删除联系人索引
    pub async fn remove_contact_index(&self, contact_id: &str) -> Result<(), ShouxinError> {
        query("DELETE FROM contact_indices WHERE contact_id = ?")
            .bind(contact_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
