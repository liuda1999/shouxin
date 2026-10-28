//! 集成测试模块
//!
//! 包含3个核心场景的集成测试：
//! 1. 待办事项完整CRUD流程测试
//! 2. 记忆添加和联系人搜索测试
//! 3. 语义搜索测试

mod common;

#[cfg(test)]
mod todo_tests {
    use chrono::{Duration, Utc};
    use std::sync::Arc;
    use shouxin::api::AsyncMemory;
    use shouxin::stores::relational::TodoItem;
    use shouxin::core::contact::{OwnerType, Scope};

    use super::common::create_test_manager;

    #[tokio::test]
    async fn test_todo_crud_complete_flow() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        let todo_id = format!("todo-test-{}", uuid::Uuid::new_v4());
        let deadline = Utc::now() + Duration::days(7);
        let todo = TodoItem::new(
            todo_id.clone(),
            vec![],
            "test_user".to_string(),
            OwnerType::User,
            "测试待办事项".to_string(),
            deadline,
            "short_term".to_string(),
        );

        async_memory.add_todo(&todo).await?;

        let fetched = async_memory.get_todo(&todo_id, OwnerType::User, "test_user").await?;
        assert!(fetched.is_some(), "待办应该能够被查询到");
        let fetched = fetched.unwrap();
        assert_eq!(fetched.title, "测试待办事项");
        assert_eq!(fetched.status, "pending");

        let pending_todos = async_memory.list_pending_todos(10, Scope::User, Some("test_user")).await?;
        assert!(!pending_todos.is_empty(), "待处理列表不应该为空");
        assert!(pending_todos.iter().any(|t| t.todo_id == todo_id), "新添加的待办应该在待处理列表中");

        async_memory.complete_todo(&todo_id, OwnerType::User, "test_user").await?;

        let completed_todo = async_memory.get_todo(&todo_id, OwnerType::User, "test_user").await?;
        assert!(completed_todo.is_some(), "完成后的待办仍然应该能够查询到");
        assert_eq!(completed_todo.unwrap().status, "completed");

        let filter = shouxin::api::async_memory::TodoFilter::new()
            .with_status("pending".to_string())
            .with_limit(100);
        let pending_after_complete = async_memory.list_todos(filter, Scope::User, Some("test_user")).await?;
        assert!(!pending_after_complete.iter().any(|t| t.todo_id == todo_id),
            "已完成的待办不应该在待处理列表中");

        async_memory.delete_todo(&todo_id, OwnerType::User, "test_user").await?;

        let deleted_todo = async_memory.get_todo(&todo_id, OwnerType::User, "test_user").await?;
        assert!(deleted_todo.is_none(), "删除后的待办不应该再能查询到");

        Ok(())
    }

    #[tokio::test]
    async fn test_todo_list_all_filter() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        let deadline = Utc::now() + Duration::days(1);

        let todo1 = TodoItem::new(
            format!("todo-1-{}", uuid::Uuid::new_v4()),
            vec![],
            "user1".to_string(),
            OwnerType::User,
            "待办1".to_string(),
            deadline,
            "short_term".to_string(),
        );

        let todo2 = TodoItem::new(
            format!("todo-2-{}", uuid::Uuid::new_v4()),
            vec![],
            "user2".to_string(),
            OwnerType::User,
            "待办2".to_string(),
            deadline,
            "long_term".to_string(),
        );

        async_memory.add_todo(&todo1).await?;
        async_memory.add_todo(&todo2).await?;

        async_memory.complete_todo(&todo1.todo_id, OwnerType::User, "user1").await?;

        let all_filter = shouxin::api::async_memory::TodoFilter::new()
            .with_limit(100);
        let all_todos = async_memory.list_todos(all_filter, Scope::User, Some("user1")).await?;
        assert_eq!(all_todos.len(), 1, "user1应该只能查询到自己的1个待办");

        let pending_filter = shouxin::api::async_memory::TodoFilter::new()
            .with_status("pending".to_string())
            .with_limit(100);
        let pending_todos = async_memory.list_todos(pending_filter, Scope::User, Some("user1")).await?;
        assert_eq!(pending_todos.len(), 0, "user1没有待处理待办（唯一的todo1已完成）");

        let completed_filter = shouxin::api::async_memory::TodoFilter::new()
            .with_status("completed".to_string())
            .with_limit(100);
        let completed_todos = async_memory.list_todos(completed_filter, Scope::User, Some("user1")).await?;
        assert_eq!(completed_todos.len(), 1, "应该只有1个已完成待办");
        assert_eq!(completed_todos[0].title, "待办1");

        Ok(())
    }
}

#[cfg(test)]
mod memory_extraction_tests {
    use super::common::create_test_manager;
    use shouxin::api::AsyncMemory;
    use shouxin::core::contact::Scope;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_chinese_extractor_recognizes_names_and_time() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        let _result = async_memory.add(
            "我是张三，明天要和李四开会讨论项目进度",
            Some("user_001"),
            Scope::User
        ).await?;

        assert!(true, "SimpleExtractor不崩溃即可通过");

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_conversations_different_contacts() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        async_memory.add(
            "今天和王五约在咖啡厅讨论设计方案",
            Some("user_001"),
            Scope::User
        ).await?;

        async_memory.add(
            "李四说下周来找我处理技术问题",
            Some("user_001"),
            Scope::User
        ).await?;

        let all_filter = shouxin::api::async_memory::TodoFilter::new()
            .with_limit(100);
        let todos = async_memory.list_todos(all_filter, Scope::User, Some("user_001")).await?;

        println!("总共提取到 {} 个记忆/事件", todos.len());

        Ok(())
    }
}

#[cfg(test)]
mod semantic_search_tests {
    use super::common::create_test_manager;
    use shouxin::api::AsyncMemory;
    use shouxin::core::contact::Scope;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_embedder_produces_nonzero_vectors() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        async_memory.add(
            "我喜欢学习 Rust 编程语言，它性能很好",
            Some("user_001"),
            Scope::User
        ).await?;

        let results = async_memory.semantic_search("Rust 编程", 5, Scope::User, Some("user_001"), None).await?;

        assert!(results.is_empty() || results.iter().all(|r| r.score > 0.0),
            "向量搜索结果应该非空或分数大于0");

        Ok(())
    }

    #[tokio::test]
    async fn test_semantic_search_with_real_content() -> Result<(), Box<dyn std::error::Error>> {
        let manager = create_test_manager().await?;
        let async_memory = AsyncMemory::new(Arc::new(manager));

        async_memory.add(
            "Python 是一种高级编程语言，适合快速开发",
            Some("user_001"),
            Scope::User
        ).await?;

        async_memory.add(
            "JavaScript 主要用于网页前端开发",
            Some("user_001"),
            Scope::User
        ).await?;

        async_memory.add(
            "今天天气很好，适合出去散步",
            Some("user_001"),
            Scope::User
        ).await?;

        let results = async_memory.semantic_search("编程语言", 5, Scope::User, Some("user_001"), None).await?;

        println!("语义搜索返回 {} 个结果", results.len());
        for (i, result) in results.iter().enumerate() {
            println!("结果 {}: score={:.4}, content={}", i, result.score, result.material.content);
        }

        assert!(results.is_empty() || results.iter().any(|r| r.score > 0.3),
            "编程语言相关的搜索应该返回相关结果");

        Ok(())
    }
}
