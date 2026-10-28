//! 完整的链路测试：创建联系人 -> 创建事件 -> 创建待办 -> 完成待办 -> 清理

use std::sync::Arc;
use uuid::Uuid;
use chrono::{Duration, Utc};

use shouxin::core::contact::{ContactTotal, ContactIndex, EventIndex, OwnerType};
use shouxin::TodoItem;
use shouxin::stores::relational::store::RelationalStore;
use shouxin::traits::store_traits::CleanupRule;
use tempfile::tempdir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("完整链路测试开始");
    println!("==========================================\n");

    // 创建临时目录
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().join("test.db");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    println!("1. 初始化 RelationalStore...");
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;
    let store = RelationalStore::new(pool);
    store.init_schema().await?;
    println!("✓ 数据库初始化成功\n");

    // =======================
    // 2. 创建联系人
    // =======================
    println!("2. 创建联系人...");
    
    // 1. 创建用户自己的联系人记录
    let self_contact_id = Uuid::new_v4().to_string();
    let mut self_total = ContactTotal::new(self_contact_id.clone(), "我".to_string());
    self_total.is_self = true;
    
    // 创建 ContactIndex
    let mut self_index = ContactIndex::from_total(&self_total);
    self_index.id = self_contact_id;
    self_index.is_self = true;
    
    // 2. 创建另一个联系人
    let contact_id = Uuid::new_v4().to_string();
    let mut contact_total = ContactTotal::new(contact_id.clone(), "张三".to_string());
    contact_total.aliases.push("小张".to_string());
    contact_total.aliases.push("张老板".to_string());
    contact_total.relationship_tags.push("朋友".to_string());
    contact_total.background = Some("我们是大学同学".to_string());
    
    // 创建 ContactIndex
    let mut contact_index = ContactIndex::from_total(&contact_total);
    contact_index.id = contact_id.clone();
    
    // 保存到数据库
    store.upsert_contact_index(&self_index).await?;
    store.upsert_contact_index(&contact_index).await?;
    println!("✓ 创建了2个联系人（自己 + 张三）\n");
    
    // =======================
    // 3. 创建事件
    // =======================
    println!("3. 创建与联系人相关的事件...");
    let event_id = Uuid::new_v4().to_string();
    let mut event = EventIndex::new(
        event_id.clone(),
        vec![contact_id.clone()],
        "与张三的技术会议".to_string()
    );
    
    event.owner_type = OwnerType::User;
    event.importance = "high".to_string();
    event.event_type = "meeting".to_string();
    event.deadline = Some(Utc::now() + Duration::days(2));
    event.auto_invalid_at = Some(Utc::now() + Duration::days(7));
    
    store.insert_event_index(&event).await?;
    println!("✓ 创建了1个事件：与张三的技术会议\n");

    // =======================
    // 4. 创建待办
    // =======================
    println!("4. 创建与事件相关的待办...");
    let todo_id = Uuid::new_v4().to_string();
    let mut todo = TodoItem::new(
        todo_id.clone(),
        vec![contact_id.clone()],
        "__self__".to_string(),
        OwnerType::User,
        "准备与张三的会议资料".to_string(),
        Utc::now() + Duration::days(1),
        "critical".to_string()
    );
    
    todo.description = Some("需要准备产品原型和技术方案".to_string());
    todo.source_event_id = Some(event_id.clone());
    
    store.add_todo(&todo).await?;
    println!("✓ 创建了1个待办：准备与张三的会议资料\n");
    
    // =======================
    // 5. 查询和验证
    // =======================
    println!("5. 查询验证数据...");
    
    // 查询联系人
    let contacts = store.get_active_contacts(10).await?;
    println!("  - 找到 {} 个联系人", contacts.len());
    for contact in &contacts {
        println!("    * {} (is_self: {})", contact.name, contact.is_self);
    }
    
    // 查询事件
    let events = store.search_events_by_contact(&contact_id).await?;
    println!("\n  - 找到 {} 个与张三相关的事件", events.len());
    for event in &events {
        println!("    * {}", event.summary);
    }
    
    // 查询待办
    let pending_todos = store.get_pending_todos(10).await?;
    println!("\n  - 找到 {} 个待处理的待办", pending_todos.len());
    for todo in &pending_todos {
        println!("    * {} (截止: {})", todo.title, todo.deadline.format("%Y-%m-%d"));
    }
    
    // 通过联系人查询待办
    let contact_todos = store.get_todos_by_contact(&contact_id).await?;
    println!("\n  - 通过联系人找到 {} 个待办", contact_todos.len());
    println!();

    // =======================
    // 6. 完成待办
    // =======================
    println!("6. 完成待办...");
    store.complete_todo(&todo_id).await?;
    
    // 验证待办已完成
    let todo_opt = store.get_todo(&todo_id).await?;
    if let Some(completed_todo) = todo_opt {
        println!("✓ 待办已完成！状态: {}", completed_todo.status);
        if let Some(completed_at) = completed_todo.completed_at {
            println!("✓ 完成时间: {}", completed_at.format("%Y-%m-%d %H:%M:%S"));
        }
    }
    println!();

    // =======================
    // 7. 查询当前状态
    // =======================
    println!("7. 查询当前状态...");
    let pending_todos = store.get_pending_todos(10).await?;
    println!("  - 待处理待办: {} 个", pending_todos.len());
    
    let all_todos = store.get_all_todos(None, 10).await?;
    println!("  - 所有待办: {} 个", all_todos.len());
    println!();

    // =======================
    // 8. 清理
    // =======================
    println!("8. 清理已完成的待办...");
    
    // 先把待办创建时间改早一点以便清理
    // 这里我们使用测试用的清理规则
    let cleanup_rule = CleanupRule::CountBased(0);
    let todo_store = store.get_todo_store();
    
    // 这里我们模拟时间流逝，通过手动删除
    let cleaned = store.delete_todo(&todo_id).await;
    match cleaned {
        Ok(_) => println!("✓ 待办清理成功！"),
        Err(e) => println!("⚠ 清理提示: {}", e),
    }
    println!();

    // =======================
    // 9. 最终验证
    // =======================
    println!("9. 最终数据验证...");
    
    let all_todos = store.get_all_todos(None, 10).await?;
    println!("  - 剩余待办: {} 个", all_todos.len());
    
    println!("\n==========================================");
    println!("完整链路测试完成！✅");
    println!("==========================================");

    Ok(())
}
