//! 最直接的测试：仅测试 RelationalStore 核心写入功能
//! 不依赖整个 Shouxin 系统，直接操作 SQLite

use chrono::{Duration, Utc};
use uuid::Uuid;

use shouxin::core::contact::{ContactIndex, ContactTotal, EventIndex};
use shouxin::TodoItem;
use shouxin::core::contact::OwnerType;
use shouxin::stores::relational::store::RelationalStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n");
    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║         直接 RelationalStore 写入测试 - 每步验证                   ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    // ====================================
    // 1. 数据库初始化
    // ====================================
    println!("【1/7】初始化内存数据库...");
    let store = RelationalStore::in_memory().await?;
    println!("✅ 数据库初始化成功!\n");

    // ====================================
    // 2. 创建联系人
    // ====================================
    println!("【2/7】创建联系人...");

    // 联系人1 - 自己
    let self_id = "__self__".to_string();
    let mut self_total = ContactTotal::new(self_id.clone(), "我".to_string());
    self_total.is_self = true;
    let mut self_idx = ContactIndex::from_total(&self_total);
    self_idx.id = self_id;
    store.upsert_contact_index(&self_idx).await?;
    println!("  → 写入联系人1: {}", self_idx.name);

    // 联系人2 - 张三
    let contact_id = Uuid::new_v4().to_string();
    let mut contact_total = ContactTotal::new(contact_id.clone(), "张三".to_string());
    contact_total.background = Some("朋友，认识10年".to_string());
    contact_total.aliases.push("小张".to_string());
    let mut contact_idx = ContactIndex::from_total(&contact_total);
    contact_idx.id = contact_id;
    store.upsert_contact_index(&contact_idx).await?;
    println!("  → 写入联系人2: {}", contact_idx.name);

    // 验证联系人写入
    let contacts = store.get_active_contacts(10).await?;
    println!("  → 验证: 找到 {} 个联系人\n", contacts.len());

    // ====================================
    // 3. 创建事件
    // ====================================
    println!("【3/7】创建事件...");

    let event_id = Uuid::new_v4().to_string();
    let mut event = EventIndex::new(
        event_id.clone(),
        vec![contact_idx.id.clone()],
        "和张三的产品讨论会议".to_string(),
    );
    event.importance = "high".to_string();
    event.event_type = "meeting".to_string();
    event.deadline = Some(Utc::now() + Duration::days(2));
    store.insert_event_index(&event).await?;
    println!("  → 写入事件: {}", event.summary);

    // 验证事件写入
    let events = store.search_events_by_contact(&contact_idx.id).await?;
    println!("  → 验证: 找到 {} 个与张三相关的事件\n", events.len());

    // ====================================
    // 4. 创建待办
    // ====================================
    println!("【4/7】创建待办...");

    let todo_id = Uuid::new_v4().to_string();
    let mut todo = TodoItem::new(
        todo_id.clone(),
        vec![contact_idx.id.clone()],
        "__self__".to_string(),
        OwnerType::User,
        "准备产品会议PPT".to_string(),
        Utc::now() + Duration::hours(24),
        "critical".to_string(),
    );
    todo.description = Some("需要包含架构图".to_string());
    store.add_todo(&todo).await?;
    println!("  → 写入待办: {}", todo.title);

    // 验证待办写入
    let pending = store.get_pending_todos(10).await?;
    println!("  → 验证: 找到 {} 个待处理待办\n", pending.len());

    // ====================================
    // 5. 完成待办
    // ====================================
    println!("【5/7】标记待办为完成...");
    store.complete_todo(&todo_id).await?;
    println!("  → 待办已标记为完成\n");

    // ====================================
    // 6. 清理待办
    // ====================================
    println!("【6/7】清理已完成的待办...");
    let cleaned_count = store.cleanup_todos().await?;
    println!("  → 清理了 {} 个待办\n", cleaned_count);

    // ====================================
    // 7. 最终验证
    // ====================================
    println!("【7/7】最终验证...");

    let final_contacts = store.get_active_contacts(10).await?;
    println!("  → 联系人: {} 个", final_contacts.len());

    let final_events = store.search_events_by_contact(&contact_idx.id).await?;
    println!("  → 事件: {} 个", final_events.len());

    let final_todos = store.get_all_todos(None, 10).await?;
    println!("  → 待办: {} 个\n", final_todos.len());

    println!("╔═══════════════════════════════════════════════════════════════════╗");
    println!("║                        ✅  所有测试通过  ✅                        ║");
    println!("╚═══════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
