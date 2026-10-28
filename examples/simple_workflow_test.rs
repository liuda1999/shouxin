//! 简化的链路测试

use chrono::{Duration, Utc};
use uuid::Uuid;
use tempfile::tempdir;

use shouxin::core::contact::{ContactTotal, ContactIndex, EventIndex, OwnerType};
use shouxin::TodoItem;
use shouxin::stores::relational::store::RelationalStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==========================================");
    println!("简化链路测试");
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
    
    // 创建联系人记录
    let contact_id = Uuid::new_v4().to_string();
    let mut contact_total = ContactTotal::new(contact_id.clone(), "张三".to_string());
    contact_total.aliases.push("小张".to_string());
    contact_total.background = Some("我们是大学同学".to_string());
    
    // 创建 ContactIndex
    let mut contact_index = ContactIndex::from_total(&contact_total);
    contact_index.id = contact_id.clone();
    
    // 保存到数据库
    store.upsert_contact_index(&contact_index).await?;
    println!("✓ 创建了联系人：张三\n");
    
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
    
    store.insert_event_index(&event).await?;
    println!("✓ 创建了事件：与张三的技术会议\n");

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
    println!("✓ 创建了待办：准备与张三的会议资料\n");
    
    // =======================
    // 5. 查询验证
    // =======================
    println!("5. 查询验证数据...");
    
    // 查询联系人
    let contacts = store.get_active_contacts(10).await?;
    println!("  - 找到 {} 个联系人", contacts.len());
    for contact in &contacts {
        println!("    * {}", contact.name);
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
    }
    println!();

    // =======================
    // 7. 清理
    // =======================
    println!("7. 清理已完成的待办...");
    let cleaned = store.cleanup_todos().await?;
    println!("✓ 清理了 {} 个已完成的待办", cleaned);
    
    // 最终验证
    let all_todos = store.get_all_todos(None, 10).await?;
    println!("\n8. 最终验证: 剩余待办 {} 个", all_todos.len());
    
    println!("\n==========================================");
    println!("测试完成！✅");
    println!("==========================================");

    Ok(())
}
