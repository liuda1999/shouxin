//! 虚拟角色（历史人物 / 神话 / 动漫 / 小说 / 游戏角色）独立存储
//!
//! 为什么**不**和真人联系人放在一张表（`contact_index`）：
//!
//! 1. **建模不同**：真人联系人的画像是"从对话里推断、会过时、要迭代"的；
//!    虚拟角色的设定来自原著/典籍，是**事实**，不该被模型改写，也谈不上"更新频率"。
//! 2. **基数不同**：一个用户的常联系人通常几十个，而角色可能上千（神话→历史→番剧）。
//!    共用一张表意味着它们会挤进为"熟人"设计的热表裁剪（50 条）、归档衰减、同名去重规则里。
//! 3. **同名合法**：`contact_index` 对 (name, owner) 有唯一约束，而"雅典娜"（希腊神话）
//!    和"雅典娜"（圣斗士）必须能共存——唯一键必须带 `source_work`。
//!
//! 因此这里用独立的 `virtual_character` + `virtual_character_alias`，
//! 事件侧用 `event_character_link` 关联（不复用 `event_index.contact_ids`）。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Row, Sqlite};
use uuid::Uuid;

use crate::core::contact::OwnerType;
use crate::error::ShouxinError;

/// 虚拟角色
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCharacter {
    pub id: String,
    pub name: String,
    /// 出自哪部作品 / 哪个体系（历史人物可填朝代或史书）
    pub source_work: Option<String>,
    /// 原著设定（写死的事实）：身份、能力、关系、世界观，JSON
    pub canon: Option<String>,
    /// 扮演用的人设（说话风格等），JSON
    pub persona: Option<String>,
    /// myth / history / anime / game / novel / other
    pub category: String,
    /// 内置角色（随程序分发，不允许删除）
    pub is_builtin: bool,
    pub is_archived: bool,
    pub owner_type: OwnerType,
    pub owner_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCharacterAlias {
    pub id: i64,
    pub character_id: String,
    pub alias_name: String,
    pub source_work: Option<String>,
    pub alias_type: String,
    pub language: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl VirtualCharacterAlias {
    pub fn new(
        character_id: String,
        alias_name: String,
        source_work: Option<String>,
        alias_type: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            character_id,
            alias_name,
            source_work,
            alias_type: alias_type.unwrap_or_else(|| "nickname".to_string()),
            language: None,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualCharacterInfo {
    pub character: VirtualCharacter,
    pub aliases: Vec<VirtualCharacterAlias>,
}

/// 新建角色的入参
#[derive(Debug, Clone, Default)]
pub struct NewVirtualCharacter {
    pub name: String,
    pub source_work: Option<String>,
    pub canon: Option<String>,
    pub persona: Option<String>,
    pub category: Option<String>,
    pub owner_id: Option<String>,
    pub is_builtin: bool,
    pub aliases: Vec<VirtualCharacterAlias>,
}

pub struct VirtualEntityStore {
    pool: Pool<Sqlite>,
}

impl VirtualEntityStore {
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self { pool }
    }

    /// 建表（幂等）
    pub async fn init_schema(&self) -> Result<(), ShouxinError> {
        let stmts = [
            r#"CREATE TABLE IF NOT EXISTS virtual_character (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_work TEXT,
                canon TEXT,
                persona TEXT,
                category TEXT NOT NULL DEFAULT 'other',
                is_builtin INTEGER NOT NULL DEFAULT 0,
                is_archived INTEGER NOT NULL DEFAULT 0,
                owner_type TEXT NOT NULL DEFAULT 'User',
                owner_id TEXT NOT NULL DEFAULT 'default_user',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )"#,
            // 同名不同作品必须能共存 → 唯一键带上 source_work
            r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_unique_virtual_name_work_owner
               ON virtual_character(name, IFNULL(source_work, ''), owner_id, owner_type)"#,
            r#"CREATE INDEX IF NOT EXISTS idx_virtual_character_name ON virtual_character(name)"#,
            r#"CREATE TABLE IF NOT EXISTS virtual_character_alias (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                character_id TEXT NOT NULL,
                alias_name TEXT NOT NULL,
                source_work TEXT,
                alias_type TEXT DEFAULT 'nickname',
                language TEXT,
                created_at TEXT NOT NULL,
                UNIQUE(character_id, alias_name)
            )"#,
            r#"CREATE INDEX IF NOT EXISTS idx_virtual_alias_name ON virtual_character_alias(alias_name)"#,
            // 事件 ↔ 角色：不复用 event_index.contact_ids
            r#"CREATE TABLE IF NOT EXISTS event_character_link (
                event_id TEXT NOT NULL,
                character_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (event_id, character_id)
            )"#,
        ];
        for s in stmts {
            query(s).execute(&self.pool).await?;
        }
        Ok(())
    }

    /// 按姓名或别名匹配虚拟角色（大小写不敏感）
    ///
    /// 用途：对话里提到"孙悟空"时，不要让抽取链路把他当真人建联系人。
    pub async fn check_virtual_match(
        &self,
        name: &str,
    ) -> Result<Option<VirtualCharacterInfo>, ShouxinError> {
        let lowered = name.trim().to_lowercase();
        if lowered.is_empty() {
            return Ok(None);
        }

        // 先按别名
        if let Some(row) = query(
            r#"SELECT character_id FROM virtual_character_alias
               WHERE LOWER(alias_name) = ? LIMIT 1"#,
        )
        .bind(&lowered)
        .fetch_optional(&self.pool)
        .await?
        {
            let cid: String = row.get("character_id");
            return self.get_by_id(&cid).await;
        }

        // 再按本名
        if let Some(row) = query(
            r#"SELECT id FROM virtual_character
               WHERE LOWER(name) = ? AND is_archived = 0 LIMIT 1"#,
        )
        .bind(&lowered)
        .fetch_optional(&self.pool)
        .await?
        {
            let cid: String = row.get("id");
            return self.get_by_id(&cid).await;
        }

        Ok(None)
    }

    pub async fn get_by_id(&self, id: &str) -> Result<Option<VirtualCharacterInfo>, ShouxinError> {
        let row = query(
            r#"SELECT id, name, source_work, canon, persona, category, is_builtin, is_archived,
                      owner_type, owner_id, created_at, updated_at
               FROM virtual_character WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let character = Self::row_to_character(&row)?;
                let aliases = self.get_aliases(&character.id).await?;
                Ok(Some(VirtualCharacterInfo { character, aliases }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_aliases(
        &self,
        character_id: &str,
    ) -> Result<Vec<VirtualCharacterAlias>, ShouxinError> {
        let rows = query(
            r#"SELECT id, character_id, alias_name, source_work, alias_type, language, created_at
               FROM virtual_character_alias WHERE character_id = ? ORDER BY id"#,
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            out.push(VirtualCharacterAlias {
                id: row.get("id"),
                character_id: row.get("character_id"),
                alias_name: row.get("alias_name"),
                source_work: row.get("source_work"),
                alias_type: row
                    .get::<Option<String>, _>("alias_type")
                    .unwrap_or_else(|| "nickname".to_string()),
                language: row.get("language"),
                created_at: parse_ts(row.get::<String, _>("created_at").as_str()),
            });
        }
        Ok(out)
    }

    /// 列出角色。`owner_id` 为 None 表示不过滤归属（管理视图）。
    pub async fn list(
        &self,
        owner_id: Option<&str>,
        include_archived: bool,
        limit: usize,
    ) -> Result<Vec<VirtualCharacterInfo>, ShouxinError> {
        let mut sql = String::from(
            "SELECT id, name, source_work, canon, persona, category, is_builtin, is_archived, \
             owner_type, owner_id, created_at, updated_at FROM virtual_character WHERE 1=1",
        );
        if !include_archived {
            sql.push_str(" AND is_archived = 0");
        }
        let rows = match owner_id {
            Some(oid) => {
                sql.push_str(" AND owner_id = ? ORDER BY is_builtin DESC, name LIMIT ?");
                query(&sql).bind(oid).bind(limit as i64).fetch_all(&self.pool).await?
            }
            None => {
                sql.push_str(" ORDER BY is_builtin DESC, name LIMIT ?");
                query(&sql).bind(limit as i64).fetch_all(&self.pool).await?
            }
        };

        let mut out = Vec::new();
        for row in rows {
            let character = Self::row_to_character(&row)?;
            let aliases = self.get_aliases(&character.id).await?;
            out.push(VirtualCharacterInfo { character, aliases });
        }
        Ok(out)
    }

    /// 新建角色；同名同作品同 owner 视为重复，返回错误
    pub async fn create(
        &self,
        input: NewVirtualCharacter,
    ) -> Result<VirtualCharacterInfo, ShouxinError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let owner_id = input
            .owner_id
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "default_user".to_string());
        let category = input
            .category
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "other".to_string());

        query(
            r#"INSERT INTO virtual_character
               (id, name, source_work, canon, persona, category, is_builtin, is_archived,
                owner_type, owner_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, 0, 'User', ?, ?, ?)"#,
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.source_work)
        .bind(&input.canon)
        .bind(&input.persona)
        .bind(&category)
        .bind(if input.is_builtin { 1 } else { 0 })
        .bind(&owner_id)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                ShouxinError::GeneralError(format!(
                    "同名角色已存在（同一作品下不能重复）：{}",
                    input.name
                ))
            } else {
                ShouxinError::DatabaseError(e.to_string())
            }
        })?;

        // 至少把本名登记成别名，方便按名字匹配
        let mut alias_names: Vec<String> = vec![input.name.clone()];
        for a in &input.aliases {
            if !alias_names.iter().any(|n| n.eq_ignore_ascii_case(&a.alias_name)) {
                alias_names.push(a.alias_name.clone());
            }
        }
        for (idx, alias_name) in alias_names.iter().enumerate() {
            let (source, kind) = input
                .aliases
                .iter()
                .find(|a| &a.alias_name == alias_name)
                .map(|a| (a.source_work.clone(), a.alias_type.clone()))
                .unwrap_or((input.source_work.clone(), if idx == 0 { "official".into() } else { "nickname".into() }));
            let _ = query(
                r#"INSERT OR IGNORE INTO virtual_character_alias
                   (character_id, alias_name, source_work, alias_type, language, created_at)
                   VALUES (?, ?, ?, ?, NULL, ?)"#,
            )
            .bind(&id)
            .bind(alias_name)
            .bind(&source)
            .bind(&kind)
            .bind(now.to_rfc3339())
            .execute(&self.pool)
            .await;
        }

        self.get_by_id(&id)
            .await?
            .ok_or_else(|| ShouxinError::InternalError("创建角色后读取失败".to_string()))
    }

    /// 更新角色基本字段（值为 None 的字段保持不变）
    pub async fn update(
        &self,
        id: &str,
        name: Option<String>,
        source_work: Option<String>,
        canon: Option<String>,
        persona: Option<String>,
        category: Option<String>,
    ) -> Result<Option<VirtualCharacterInfo>, ShouxinError> {
        let existing = self.get_by_id(id).await?;
        let Some(info) = existing else { return Ok(None) };

        query(
            r#"UPDATE virtual_character
               SET name = ?, source_work = ?, canon = ?, persona = ?, category = ?, updated_at = ?
               WHERE id = ?"#,
        )
        .bind(name.unwrap_or(info.character.name))
        .bind(source_work.or(info.character.source_work))
        .bind(canon.or(info.character.canon))
        .bind(persona.or(info.character.persona))
        .bind(category.unwrap_or(info.character.category))
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;

        self.get_by_id(id).await
    }

    /// 删除角色（内置角色不允许删除）
    pub async fn delete(&self, id: &str) -> Result<bool, ShouxinError> {
        let info = self.get_by_id(id).await?;
        let Some(info) = info else { return Ok(false) };
        if info.character.is_builtin {
            return Err(ShouxinError::InvalidArgument(
                "内置虚拟角色不允许删除（可改为归档）".to_string(),
            ));
        }
        query("DELETE FROM virtual_character_alias WHERE character_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        query("DELETE FROM event_character_link WHERE character_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        let res = query("DELETE FROM virtual_character WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// 覆盖式替换别名
    pub async fn replace_aliases(
        &self,
        character_id: &str,
        aliases: &[VirtualCharacterAlias],
    ) -> Result<usize, ShouxinError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM virtual_character_alias WHERE character_id = ?")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        let mut inserted = 0usize;
        for a in aliases {
            let res = sqlx::query(
                r#"INSERT OR IGNORE INTO virtual_character_alias
                   (character_id, alias_name, source_work, alias_type, language, created_at)
                   VALUES (?, ?, ?, ?, ?, ?)"#,
            )
            .bind(character_id)
            .bind(&a.alias_name)
            .bind(&a.source_work)
            .bind(&a.alias_type)
            .bind(&a.language)
            .bind(a.created_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
            inserted += res.rows_affected() as usize;
        }
        tx.commit().await?;
        Ok(inserted)
    }

    /// 事件 ↔ 角色关联（幂等）
    pub async fn link_event_characters(
        &self,
        event_id: &str,
        character_ids: &[String],
    ) -> Result<usize, ShouxinError> {
        let now = Utc::now().to_rfc3339();
        let mut n = 0usize;
        for cid in character_ids {
            let res = query(
                r#"INSERT OR IGNORE INTO event_character_link (event_id, character_id, created_at)
                   VALUES (?, ?, ?)"#,
            )
            .bind(event_id)
            .bind(cid)
            .bind(&now)
            .execute(&self.pool)
            .await?;
            n += res.rows_affected() as usize;
        }
        Ok(n)
    }

    pub async fn get_event_characters(
        &self,
        event_id: &str,
    ) -> Result<Vec<String>, ShouxinError> {
        let rows = query("SELECT character_id FROM event_character_link WHERE event_id = ?")
            .bind(event_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r| r.get::<String, _>("character_id")).collect())
    }

    fn row_to_character(row: &<Sqlite as sqlx::Database>::Row) -> Result<VirtualCharacter, ShouxinError> {
        let owner_type = match row.get::<String, _>("owner_type").as_str() {
            "Agent" => OwnerType::Agent,
            _ => OwnerType::User,
        };
        Ok(VirtualCharacter {
            id: row.get("id"),
            name: row.get("name"),
            source_work: row.get("source_work"),
            canon: row.get("canon"),
            persona: row.get("persona"),
            category: row.get::<Option<String>, _>("category").unwrap_or_else(|| "other".into()),
            is_builtin: row.get::<i64, _>("is_builtin") != 0,
            is_archived: row.get::<i64, _>("is_archived") != 0,
            owner_type,
            owner_id: row.get("owner_id"),
            created_at: parse_ts(row.get::<String, _>("created_at").as_str()),
            updated_at: parse_ts(row.get::<String, _>("updated_at").as_str()),
        })
    }
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
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
