use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Row, Sqlite};
use uuid::Uuid;

use crate::core::contact::OwnerType;
use crate::core::ContactIndex;
use crate::error::ShouxinError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCharacterAlias {
    pub id: i64,
    pub contact_id: String,
    pub alias_name: String,
    pub source_work: Option<String>,
    pub alias_type: String,
    pub created_at: DateTime<Utc>,
}

impl VirtualCharacterAlias {
    pub fn new(
        contact_id: String,
        alias_name: String,
        source_work: Option<String>,
        alias_type: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            contact_id,
            alias_name,
            source_work,
            alias_type: alias_type.unwrap_or_else(|| "nickname".to_string()),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCharacterInfo {
    pub contact: ContactIndex,
    pub aliases: Vec<VirtualCharacterAlias>,
}

pub struct VirtualCharacterAliasStore {
    pool: Pool<Sqlite>,
}

impl VirtualCharacterAliasStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    pub async fn check_virtual_match(&self, name: &str) -> Result<Option<VirtualCharacterInfo>, ShouxinError> {
        let name_lower = name.to_lowercase();
        
        let alias_row = query(
            r#"
            SELECT contact_id, alias_name, source_work, alias_type, created_at
            FROM virtual_character_aliases
            WHERE LOWER(alias_name) = ?
            LIMIT 1
            "#,
        )
        .bind(&name_lower)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = alias_row {
            let contact_id: String = row.get("contact_id");
            return self.get_virtual_character_by_contact_id(&contact_id).await;
        }

        let contact_row = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual
            FROM contact_index
            WHERE LOWER(name) = ? AND is_virtual = 1
            LIMIT 1
            "#,
        )
        .bind(&name_lower)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = contact_row {
            let contact = self.row_to_contact(&row)?;
            let aliases = self.get_aliases_by_contact_id(&contact.id).await?;
            return Ok(Some(VirtualCharacterInfo { contact, aliases }));
        }

        Ok(None)
    }

    pub async fn get_virtual_character_by_contact_id(&self, contact_id: &str) -> Result<Option<VirtualCharacterInfo>, ShouxinError> {
        let contact_row = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual
            FROM contact_index
            WHERE id = ? AND is_virtual = 1
            "#,
        )
        .bind(contact_id)
        .fetch_optional(&self.pool)
        .await?;

        match contact_row {
            Some(row) => {
                let contact = self.row_to_contact(&row)?;
                let aliases = self.get_aliases_by_contact_id(contact_id).await?;
                Ok(Some(VirtualCharacterInfo { contact, aliases }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_aliases_by_contact_id(&self, contact_id: &str) -> Result<Vec<VirtualCharacterAlias>, ShouxinError> {
        let rows = query(
            r#"
            SELECT id, contact_id, alias_name, source_work, alias_type, created_at
            FROM virtual_character_aliases
            WHERE contact_id = ?
            ORDER BY alias_type, alias_name
            "#,
        )
        .bind(contact_id)
        .fetch_all(&self.pool)
        .await?;

        let mut aliases = Vec::new();
        for row in rows {
            aliases.push(VirtualCharacterAlias {
                id: row.get("id"),
                contact_id: row.get("contact_id"),
                alias_name: row.get("alias_name"),
                source_work: row.get("source_work"),
                alias_type: row.get("alias_type"),
                created_at: DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap_or_else(|_| Utc::now().into())
                    .with_timezone(&Utc),
            });
        }

        Ok(aliases)
    }

    pub async fn insert_alias(&self, alias: &VirtualCharacterAlias) -> Result<(), ShouxinError> {
        query(
            r#"
            INSERT INTO virtual_character_aliases (contact_id, alias_name, source_work, alias_type, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&alias.contact_id)
        .bind(&alias.alias_name)
        .bind(&alias.source_work)
        .bind(&alias.alias_type)
        .bind(alias.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_aliases_by_contact_id(&self, contact_id: &str) -> Result<(), ShouxinError> {
        query("DELETE FROM virtual_character_aliases WHERE contact_id = ?")
            .bind(contact_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 原子替换某虚拟角色的全部别名（删除 + 批量插入在同一个事务内）
    ///
    /// 旧实现是"先 delete、再逐条 insert，错误全部 `.ok()` 吞掉"：
    /// 中途失败会让别名全部丢失，而接口仍返回成功（F-V1）。
    pub async fn replace_aliases(
        &self,
        contact_id: &str,
        aliases: &[VirtualCharacterAlias],
    ) -> Result<usize, ShouxinError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM virtual_character_aliases WHERE contact_id = ?")
            .bind(contact_id)
            .execute(&mut *tx)
            .await?;

        let mut inserted = 0usize;
        for alias in aliases {
            let result = sqlx::query(
                r#"
                INSERT INTO virtual_character_aliases (contact_id, alias_name, source_work, alias_type, created_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(&alias.contact_id)
            .bind(&alias.alias_name)
            .bind(&alias.source_work)
            .bind(&alias.alias_type)
            .bind(alias.created_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
            inserted += result.rows_affected() as usize;
        }

        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn get_all_virtual_characters(&self) -> Result<Vec<VirtualCharacterInfo>, ShouxinError> {
        let rows = query(
            r#"
            SELECT id, name, aliases, is_self, created_at, last_interaction_at,
                   related_contact_ids, relationship_tags, relation_weight, background, notes,
                   owner_type, owner_id, is_agent, parent_agent_id, is_virtual
            FROM contact_index
            WHERE is_virtual = 1
            ORDER BY name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::new();
        for row in rows {
            let contact = self.row_to_contact(&row)?;
            let aliases = self.get_aliases_by_contact_id(&contact.id).await?;
            results.push(VirtualCharacterInfo { contact, aliases });
        }

        Ok(results)
    }

    fn row_to_contact(&self, row: &<Sqlite as sqlx::Database>::Row) -> Result<ContactIndex, ShouxinError> {
        let created_at_str: String = row.get("created_at");
        let last_interaction_at_str: String = row.get("last_interaction_at");
        let is_self: i64 = row.get("is_self");
        let owner_type_str: String = row.get("owner_type");
        let owner_id: String = row.get("owner_id");
        let is_agent_int: i64 = row.try_get("is_agent").unwrap_or(0);
        let parent_agent_id: Option<String> = row.try_get("parent_agent_id").ok();
        let is_virtual_int: i64 = row.try_get("is_virtual").unwrap_or(1);

        let owner_type = match owner_type_str.as_str() {
            "User" => OwnerType::User,
            "Contact" => OwnerType::Contact,
            "Agent" => OwnerType::Agent,
            "Public" => OwnerType::Public,
            _ => OwnerType::User,
        };

        Ok(ContactIndex {
            id: row.get("id"),
            name: row.get("name"),
            aliases: row.get("aliases"),
            is_self: is_self == 1,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc),
            last_interaction_at: DateTime::parse_from_rfc3339(&last_interaction_at_str)
                .map_err(|e| ShouxinError::GeneralError(e.to_string()))?
                .with_timezone(&Utc),
            related_contact_ids: row.get("related_contact_ids"),
            relationship_tags: row.get("relationship_tags"),
            relation_weight: row.get("relation_weight"),
            background: row.get("background"),
            notes: row.get("notes"),
            owner_type,
            owner_id,
            is_agent: is_agent_int != 0,
            parent_agent_id,
            is_virtual: is_virtual_int != 0,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        })
    }
}

pub struct BuiltInVirtualCharacters;

impl BuiltInVirtualCharacters {
    pub fn get_all() -> Vec<(&'static str, &'static str, Vec<(&'static str, &'static str, &'static str)>)> {
        vec![
            (
                "林克",
                "link",
                vec![
                    ("Link", "塞尔达传说", "official"),
                    ("时之勇者", "塞尔达传说", "nickname"),
                    ("塞尔达", "塞尔达传说", "abbreviated"),
                ],
            ),
            (
                "ChatGPT",
                "chatgpt",
                vec![
                    ("GPT", "AI助手", "abbreviated"),
                    ("OpenAI", "AI助手", "source"),
                ],
            ),
            (
                "马里奥",
                "mario",
                vec![
                    ("Mario", "超级马里奥", "official"),
                    ("玛丽奥", "超级马里奥", "localized"),
                ],
            ),
            (
                "皮卡丘",
                "pikachu",
                vec![
                    ("Pikachu", "宝可梦", "official"),
                    ("皮神", "宝可梦", "fan"),
                ],
            ),
            (
                "孙悟空",
                "wukong",
                vec![
                    ("齐天大圣", "西游记", "title"),
                    ("弼马温", "西游记", "nickname"),
                    ("大师兄", "西游记", "nickname"),
                    ("美猴王", "西游记", "nickname"),
                ],
            ),
            (
                "曹操",
                "caocao",
                vec![
                    ("孟德", "三国演义", "courtesy_name"),
                    ("魏武帝", "三国演义", "posthumous_title"),
                ],
            ),
            (
                "蝙蝠侠",
                "batman",
                vec![
                    ("Batman", "DC漫画", "official"),
                    ("Bruce Wayne", "DC漫画", "real_name"),
                    ("布鲁斯韦恩", "DC漫画", "localized"),
                ],
            ),
            (
                "钢铁侠",
                "ironman",
                vec![
                    ("Iron Man", "漫威漫画", "official"),
                    ("Tony Stark", "漫威漫画", "real_name"),
                    ("托尼史塔克", "漫威漫画", "localized"),
                ],
            ),
            (
                "克劳德",
                "cloud",
                vec![
                    ("Cloud", "最终幻想7", "official"),
                    ("克劳德·斯特莱夫", "最终幻想7", "full_name"),
                ],
            ),
            (
                "哆啦A梦",
                "doraemon",
                vec![
                    ("哆啦A梦", "哆啦A梦", "official"),
                    ("机器猫", "哆啦A梦", "localized"),
                    ("小叮当", "哆啦A梦", "localized"),
                ],
            ),
            (
                "柯南",
                "conan",
                vec![
                    ("柯南", "名侦探柯南", "official"),
                    ("工藤新一", "名侦探柯南", "real_name"),
                ],
            ),
        ]
    }
}
