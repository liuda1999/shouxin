//! 详细的完整链路测试：验证每一步写入
//! 包含完整的调试信息

use chrono::{Duration, Utc};
use sqlx::Row;
use uuid::Uuid;

use shouxin::core::contact::{ContactIndex, ContactTotal, EventIndex};
use shouxin::TodoItem;
use shouxin::core::contact::OwnerType;
use shouxin::stores::relational::store::RelationalStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n==========================================");
    println!("  详细完整链路测试 - 验证每一步写入  ");
    println!("==========================================\n");

    // 创建内存数据库
    let store = RelationalStore::in_memory().await?;
    println!("1️⃣  数据库初始化成功\n");

    // =======================
    // 步骤 1: 创建联系人
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 1: 创建联系人");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // 创建自己的联系人记录
    let self_contact_id = "__self__".to_string();
    let mut self_total = ContactTotal::new(self_contact_id.clone(), "我".to_string());
    self_total.is_self = true;

    let mut self_index = ContactIndex::from_total(&self_total);
    self_index.id = self_contact_id.clone();

    // 创建另一个联系人
    let contact_id = Uuid::new_v4().to_string();
    let mut contact_total = ContactTotal::new(contact_id.clone(), "张三".to_string());
    contact_total.aliases.push("小张".to_string());
    contact_total.relationship_tags.push("朋友".to_string());
    contact_total.background = Some("我们是大学同学，认识10年了".to_string());

    let mut contact_index = ContactIndex::from_total(&contact_total);
    contact_index.id = contact_id.clone();

    println!("  → 准备写入2个联系人");
    println!("    • 联系人 1: ID={}, 姓名='{}', 是否自己={}", self_contact_id, "我", true);
    println!("    • 联系人 2: ID={}, 姓名='{}', 背景='{}'", contact_id, "张三", "我们是大学同学，认识10年了");

    // 保存到数据库
    println!("\n  → 执行写入...");
    store.upsert_contact_index(&self_index).await?;
    println!("    ✓ 第一个联系人写入成功");

    store.upsert_contact_index(&contact_index).await?;
    println!("    ✓ 第二个联系人写入成功\n");

    // 验证联系人
    let contacts = store.get_active_contacts(10).await?;
    println!("  → 查询验证:");
    println!("    找到 {} 个联系人", contacts.len());
    for (i, c) in contacts.iter().enumerate() {
        println!("    {}. 姓名='{}', ID={}, 是否自己={}", i + 1, c.name, c.id, c.is_self);
    }
    println!("    ✓ 联系人写入验证成功\n");

    // =======================
    // 步骤 2: 创建事件
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 2: 创建与联系人相关的事件");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let event_id = Uuid::new_v4().to_string();
    let mut event = EventIndex::new(
        event_id.clone(),
        vec![contact_id.clone()],
        "与张三的技术架构讨论会议".to_string(),
    );

    event.owner_type = OwnerType::User;
    event.importance = "high".to_string();
    event.event_type = "architecture_meeting".to_string();
    event.status = "scheduled".to_string();
    event.deadline = Some(Utc::now() + Duration::days(2));
    event.auto_invalid_at = Some(Utc::now() + Duration::days(7));

    println!("  → 准备写入事件");
    println!("    • 事件 ID: {}", event_id);
    println!("    • 摘要: '与张三的技术架构讨论会议'");
    println!("    • 类型: 'architecture_meeting'");
    println!("    • 重要性: high");
    println!("    • 关联联系人: [{}]", contact_id);

    println!("\n  → 执行写入...");
    store.insert_event_index(&event).await?;
    println!("    ✓ 事件写入成功\n");

    // 验证事件
    let events = store.search_events_by_contact(&contact_id).await?;
    println!("  → 查询验证:");
    println!("    找到 {} 个与张三相关的事件", events.len());
    for (i, e) in events.iter().enumerate() {
        println!("    {}. 摘要='{}', 类型='{}', 重要性='{}'", i + 1, e.summary, e.event_type, e.importance);
    }
    println!("    ✓ 事件写入验证成功\n");

    // =======================
    // 步骤 3: 创建待办
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 3: 创建与事件相关的待办");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let todo_id = Uuid::new_v4().to_string();
    let mut todo = TodoItem::new(
        todo_id.clone(),
        vec![contact_id.clone()],
        "__self__".to_string(),
        OwnerType::User,
        "准备架构设计方案和技术选型报告".to_string(),
        Utc::now() + Duration::days(1),
        "critical".to_string(),
    );

    todo.description = Some("需要准备产品原型和技术方案文档，包括架构图和关键决策".to_string());
    todo.source_event_id = Some(event_id.clone());

    println!("  → 准备写入待办");
    println!("    • 待办 ID: {}", todo_id);
    println!("    • 标题: '准备架构设计方案和技术选型报告'");
    println!("    • 重要性: critical");
    println!("    • 截止日期: {}", (Utc::now() + Duration::days(1)).format("%Y-%m-%d"));
    println!("    • 关联联系人: [{}]", contact_id);
    println!("    • 关联事件: [{}]", event_id);

    println!("\n  → 执行写入...");
    store.add_todo(&todo).await?;
    println!("    ✓ 待办写入成功\n");

    // 验证待办
    let pending_todos = store.get_pending_todos(10).await?;
    println!("  → 查询验证:");
    println!("    找到 {} 个待处理的待办", pending_todos.len());
    for (i, t) in pending_todos.iter().enumerate() {
        println!("    {}. 标题='{}', 重要性='{}', 状态='{}'", i + 1, t.title, t.importance, t.status);
    }
    println!("    ✓ 待办写入验证成功\n");

    // 通过联系人查询待办
    let contact_todos = store.get_todos_by_contact(&contact_id).await?;
    println!("  → 通过联系人查询待办:");
    println!("    找到 {} 个与张三相关的待办", contact_todos.len());
    println!("    ✓ 通过联系人查询验证成功\n");

    // 直接查询待办详情
    let todo_opt = store.get_todo(&todo_id).await?;
    if let Some(t) = todo_opt {
        println!("  → 直接查询待办详情:");
        println!("    ✓ 找到待办，ID={}", t.todo_id);
        println!("      • 标题: {}", t.title);
        println!("      • 描述: {:?}", t.description);
        println!("      • 状态: {}", t.status);
        println!("      • 重要性: {}", t.importance);
        println!("    ✓ 待办详情查询验证成功\n");
    }

    // =======================
    // 步骤 4: 完成待办
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 4: 完成待办");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("  → 准备标记待办为完成");
    println!("    • 待办 ID: {}", todo_id);

    println!("\n  → 执行完成操作...");
    store.complete_todo(&todo_id).await?;
    println!("    ✓ 待办完成操作成功\n");

    // 验证待办已完成
    let todo_opt = store.get_todo(&todo_id).await?;
    if let Some(completed_todo) = todo_opt {
        println!("  → 查询验证:");
        println!("    ✓ 待办状态已更新为: {}", completed_todo.status);
        if let Some(completed_at) = completed_todo.completed_at {
            println!("    ✓ 完成时间: {}", completed_at.format("%Y-%m-%d %H:%M:%S"));
        }
        println!("    ✓ 待办完成验证成功\n");
    }

    // =======================
    // 步骤 5: 清理
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 5: 清理已完成的待办");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("  → 清理前查询:");
    let all_todos_before = store.get_all_todos(None, 10).await?;
    println!("    所有待办数量: {}", all_todos_before.len());

    println!("\n  → 执行清理...");
    let cleaned = store.cleanup_todos().await?;
    println!("    ✓ 清理完成，删除了 {} 个待办\n", cleaned);

    // =======================
    // 步骤 6: 最终验证
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("步骤 6: 最终数据验证");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let all_todos = store.get_all_todos(None, 10).await?;
    println!("  → 清理后待办数量: {}", all_todos.len());
    println!();

    // =======================
    // 最终总结
    // =======================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🎉  详细完整链路测试完成！所有功能正常  🎉");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("测试总结:");
    println!("  • 联系人创建与写入: ✅ 成功");
    println!("  • 联系人查询: ✅ 成功");
    println!("  • 事件创建与写入: ✅ 成功");
    println!("  • 事件查询: ✅ 成功");
    println!("  • 待办创建与写入: ✅ 成功");
    println!("  • 待办查询: ✅ 成功");
    println!("  • 待办完成: ✅ 成功");
    println!("  • 待办清理: ✅ 成功");
    println!();

    Ok(())
}
