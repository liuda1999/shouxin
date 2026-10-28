mod common;

#[cfg(test)]
mod file_upload_tests {
    use shouxin::stores::relational::FileIndex;
    use super::common::{create_test_file_components, make_file_processor};

    #[tokio::test]
    async fn test_file_upload_and_process() {
        let comps = create_test_file_components().await.unwrap();
        let file_store = comps.file_store.clone();
        let dictionary = comps.dictionary.clone();
        let processor = make_file_processor(&comps);

        let content = "这是ShouXin系统的文件上传测试。\n\nShouXin是一个三库架构的个人记忆管理系统。\n\n本文件用于测试v0.3.0新增的多媒体文件文本化存储功能。";
        let file_name = "test_upload.txt";

        let temp_path = std::env::temp_dir().join(format!("test_file_{}", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, content).unwrap();

        let file_id = uuid::Uuid::new_v4().to_string();
        let md5_hash = format!("{:x}", md5::compute(content.as_bytes()));
        let file_index = FileIndex {
            file_id: file_id.clone(),
            file_name: file_name.to_string(),
            file_type: "txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            file_size: Some(content.len() as i64),
            md5_hash: Some(md5_hash.clone()),
            owner_type: "agent".to_string(),
            owner_id: "test_agent".to_string(),
            uploaded_at: Some(chrono::Utc::now().to_rfc3339()),
            processed_at: None,
            status: "pending".to_string(),
            error_message: None,
            original_path: Some(temp_path.to_string_lossy().to_string()),
            extracted_text_summary: None,
        };

        file_store.insert_file(&file_index).await.unwrap();

        processor.process_file(&file_id).await.unwrap();

        let result = file_store.get_file(&file_id).await.unwrap();
        assert!(result.is_some(), "文件记录应该存在");
        let processed = result.unwrap();
        assert_eq!(processed.status, "completed", "处理状态应为completed");
        assert!(processed.extracted_text_summary.is_some(), "应该有文本摘要");
        assert!(processed.processed_at.is_some(), "应该有处理时间");

        let text_data = dictionary.get_file_text(&file_id).unwrap();
        assert!(text_data.is_some(), "字典库中应该有文件文本");
        let doc: serde_json::Value = serde_json::from_slice(&text_data.unwrap()).unwrap();
        let full_text = doc["full_text"].as_str().unwrap();
        assert!(full_text.contains("ShouXin"), "完整文本应包含文件内容");
        assert!(doc["chunks"].as_array().unwrap().len() > 0, "应该有至少一个chunk");

        println!("TEST PASSED: file upload and process - file_id={}", file_id);
    }

    #[tokio::test]
    async fn test_file_md5_dedup() {
        let comps = create_test_file_components().await.unwrap();
        let file_store = comps.file_store.clone();
        let processor = make_file_processor(&comps);

        let content = "去重测试内容。用于测试MD5去重功能。";

        let temp_path1 = std::env::temp_dir().join(format!("test_dedup_1_{}", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path1, content).unwrap();

        let file_id1 = uuid::Uuid::new_v4().to_string();
        let md5_hash = format!("{:x}", md5::compute(content.as_bytes()));
        let file_index1 = FileIndex {
            file_id: file_id1.clone(),
            file_name: "dedup_test_1.txt".to_string(),
            file_type: "txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            file_size: Some(content.len() as i64),
            md5_hash: Some(md5_hash.clone()),
            owner_type: "agent".to_string(),
            owner_id: "test_agent".to_string(),
            uploaded_at: Some(chrono::Utc::now().to_rfc3339()),
            processed_at: None,
            status: "pending".to_string(),
            error_message: None,
            original_path: Some(temp_path1.to_string_lossy().to_string()),
            extracted_text_summary: None,
        };

        file_store.insert_file(&file_index1).await.unwrap();
        processor.process_file(&file_id1).await.unwrap();

        let existing = file_store.get_file_by_md5_and_owner(&md5_hash, "agent", "test_agent").await.unwrap();
        assert!(existing.is_some(), "应该能找到已存在的文件");
        assert_eq!(existing.unwrap().file_id, file_id1, "MD5去重应返回已有file_id");

        println!("TEST PASSED: MD5 dedup returns existing file_id");
    }

    #[tokio::test]
    async fn test_file_delete() {
        let comps = create_test_file_components().await.unwrap();
        let file_store = comps.file_store.clone();
        let dictionary = comps.dictionary.clone();
        let processor = make_file_processor(&comps);

        let content = "删除测试内容。用于测试文件删除功能。";

        let temp_path = std::env::temp_dir().join(format!("test_delete_{}", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, content).unwrap();

        let file_id = uuid::Uuid::new_v4().to_string();
        let md5_hash = format!("{:x}", md5::compute(content.as_bytes()));
        let file_index = FileIndex {
            file_id: file_id.clone(),
            file_name: "delete_test.txt".to_string(),
            file_type: "txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            file_size: Some(content.len() as i64),
            md5_hash: Some(md5_hash),
            owner_type: "agent".to_string(),
            owner_id: "test_agent".to_string(),
            uploaded_at: Some(chrono::Utc::now().to_rfc3339()),
            processed_at: None,
            status: "pending".to_string(),
            error_message: None,
            original_path: Some(temp_path.to_string_lossy().to_string()),
            extracted_text_summary: None,
        };

        file_store.insert_file(&file_index).await.unwrap();
        processor.process_file(&file_id).await.unwrap();

        let before = file_store.get_file(&file_id).await.unwrap();
        assert!(before.is_some(), "删除前文件应该存在");
        assert_eq!(before.unwrap().status, "completed");

        file_store.delete_file(&file_id).await.unwrap();
        dictionary.delete_file_text(&file_id).unwrap();

        let after = file_store.get_file(&file_id).await.unwrap();
        assert!(after.is_none(), "删除后文件记录应该不存在");

        let text_after = dictionary.get_file_text(&file_id).unwrap();
        assert!(text_after.is_none(), "删除后字典库文本应该不存在");

        println!("TEST PASSED: file delete");
    }

    #[tokio::test]
    async fn test_file_ownership_isolation() {
        let comps = create_test_file_components().await.unwrap();
        let file_store = comps.file_store.clone();
        let processor = make_file_processor(&comps);

        let content = "所有权隔离测试内容。用户A的文件内容。";

        let temp_path = std::env::temp_dir().join(format!("test_ownership_{}", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, content).unwrap();

        let file_id_a = uuid::Uuid::new_v4().to_string();
        let md5_hash_a = format!("{:x}", md5::compute(content.as_bytes()));
        let file_index_a = FileIndex {
            file_id: file_id_a.clone(),
            file_name: "ownership_test.txt".to_string(),
            file_type: "txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            file_size: Some(content.len() as i64),
            md5_hash: Some(md5_hash_a.clone()),
            owner_type: "user".to_string(),
            owner_id: "user_a".to_string(),
            uploaded_at: Some(chrono::Utc::now().to_rfc3339()),
            processed_at: None,
            status: "pending".to_string(),
            error_message: None,
            original_path: Some(temp_path.to_string_lossy().to_string()),
            extracted_text_summary: None,
        };

        file_store.insert_file(&file_index_a).await.unwrap();
        processor.process_file(&file_id_a).await.unwrap();

        let user_a_result = file_store.get_file_by_md5_and_owner(&md5_hash_a, "user", "user_a").await.unwrap();
        assert!(user_a_result.is_some(), "用户A应该能找到自己的文件");

        let user_b_result = file_store.get_file_by_md5_and_owner(&md5_hash_a, "user", "user_b").await.unwrap();
        assert!(user_b_result.is_none(), "用户B不应该找到用户A的文件");

        let user_a_files = file_store.list_files_by_owner("user", "user_a", 10).await.unwrap();
        assert!(!user_a_files.is_empty(), "用户A的文件列表不应为空");

        let user_b_files = file_store.list_files_by_owner("user", "user_b", 10).await.unwrap();
        let has_user_a_file = user_b_files.iter().any(|f| f.file_id == file_id_a);
        assert!(!has_user_a_file, "用户B的文件列表不应包含用户A的文件");

        println!("TEST PASSED: ownership isolation");
    }

    #[tokio::test]
    async fn test_file_empty_content() {
        let comps = create_test_file_components().await.unwrap();
        let file_store = comps.file_store.clone();
        let processor = make_file_processor(&comps);

        let temp_path = std::env::temp_dir().join(format!("test_empty_{}", uuid::Uuid::new_v4()));
        std::fs::write(&temp_path, "").unwrap();

        let file_id = uuid::Uuid::new_v4().to_string();
        let file_index = FileIndex {
            file_id: file_id.clone(),
            file_name: "empty.txt".to_string(),
            file_type: "txt".to_string(),
            mime_type: Some("text/plain".to_string()),
            file_size: Some(0),
            md5_hash: Some(format!("{:x}", md5::compute(b""))),
            owner_type: "agent".to_string(),
            owner_id: "test_agent".to_string(),
            uploaded_at: Some(chrono::Utc::now().to_rfc3339()),
            processed_at: None,
            status: "pending".to_string(),
            error_message: None,
            original_path: Some(temp_path.to_string_lossy().to_string()),
            extracted_text_summary: None,
        };

        file_store.insert_file(&file_index).await.unwrap();

        let result = processor.process_file(&file_id).await;
        assert!(result.is_ok(), "处理空文件应该不panic");

        let processed = file_store.get_file(&file_id).await.unwrap();
        assert!(processed.is_some());
        assert_eq!(processed.unwrap().status, "failed", "空文件状态应为failed");

        println!("TEST PASSED: empty file handling");
    }
}