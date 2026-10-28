//! 联系人总表存储
//!
//! 存储所有联系人的完整信息，永不删除

use sled::Db;

use crate::error::ShouxinError;
use crate::traits::store_traits::ContactRecord;

/// 联系人总表存储
#[allow(dead_code)]
pub struct ContactsTotalStore {
    /// sled 数据库
    db: Db,
    /// 联系人树
    contact_tree: sled::Tree,
    /// 用户索引树
    user_index: sled::Tree,
}

impl ContactsTotalStore {
    /// 创建或打开联系人总表存储
    pub fn new(db: Db) -> Result<Self, ShouxinError> {
        let contact_tree = db.open_tree("contacts_total")?;
        let user_index = db.open_tree("contacts_user_index")?;

        Ok(Self {
            db,
            contact_tree,
            user_index,
        })
    }

    /// 生成用户索引键
    fn user_key(user_id: &str, contact_id: &str) -> String {
        format!("{}:{}", user_id, contact_id)
    }
}

impl ContactsTotalStore {
    /// 写入联系人记录
    pub async fn write_contact(&self, contact: ContactRecord) -> Result<(), ShouxinError> {
        let contact_id = contact.contact_id.clone();
        let bytes = serde_json::to_vec(&contact)?;

        self.contact_tree.insert(contact_id.as_bytes(), bytes)?;

        if let Some(user_id) = contact.user_id.as_ref() {
            let user_key = Self::user_key(user_id, &contact_id);
            self.user_index.insert(user_key.as_bytes(), &[1])?;
        }

        Ok(())
    }

    /// 获取联系人记录
    pub async fn get_contact(&self, contact_id: &str) -> Result<Option<ContactRecord>, ShouxinError> {
        if let Some(bytes) = self.contact_tree.get(contact_id.as_bytes())? {
            let contact: ContactRecord = serde_json::from_slice(&bytes)?;
            Ok(Some(contact))
        } else {
            Ok(None)
        }
    }

    /// 获取所有联系人
    pub async fn get_all_contacts(
        &self,
        user_id: Option<&str>,
    ) -> Result<Vec<ContactRecord>, ShouxinError> {
        let mut contacts = Vec::new();

        if let Some(uid) = user_id {
            let prefix = format!("{}:", uid);
            for item in self.user_index.scan_prefix(prefix.as_bytes()) {
                let (key, _) = item?;
                let key_str = String::from_utf8_lossy(&key).to_string();
                if let Some(contact_id) = key_str.split_once(':').map(|x| x.1) {
                    if let Some(contact) = self.get_contact(contact_id).await? {
                        contacts.push(contact);
                    }
                }
            }
        } else {
            for item in self.contact_tree.iter() {
                let (_, bytes) = item?;
                let contact: ContactRecord = serde_json::from_slice(&bytes)?;
                contacts.push(contact);
            }
        }

        Ok(contacts)
    }
}
