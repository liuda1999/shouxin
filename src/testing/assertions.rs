//! 测试断言工具
//!
//! 提供自定义断言函数

use crate::{Fact, MemoryAction};

/// 断言事实有效
pub fn assert_valid_fact(fact: &Fact) {
    assert!(
        !fact.statement.is_empty(),
        "Fact statement should not be empty"
    );
    assert!(
        fact.confidence >= 0.0 && fact.confidence <= 1.0,
        "Fact confidence should be between 0.0 and 1.0, got {}",
        fact.confidence
    );
}

/// 断言事实列表有效
pub fn assert_valid_facts(facts: &[Fact]) {
    for fact in facts {
        assert_valid_fact(fact);
    }
}

/// 断言记忆动作有效
pub fn assert_valid_memory_action(action: &MemoryAction) {
    match action {
        MemoryAction::Add => {}
        MemoryAction::Update => {}
        MemoryAction::Delete => {}
        MemoryAction::Noop => {}
    }
}

/// 断言响应时间在合理范围内
pub fn assert_reasonable_response_time(duration: std::time::Duration, max_seconds: u64) {
    assert!(
        duration.as_secs() <= max_seconds,
        "Response time {}s exceeds maximum {}s",
        duration.as_secs(),
        max_seconds
    );
}

/// 断言字符串包含预期内容
pub fn assert_contains(haystack: &str, needle: &str) {
    assert!(
        haystack.contains(needle),
        "Expected '{}' to contain '{}'",
        haystack,
        needle
    );
}

/// 断言字符串不包含内容
pub fn assert_not_contains(haystack: &str, needle: &str) {
    assert!(
        !haystack.contains(needle),
        "Expected '{}' to NOT contain '{}'",
        haystack,
        needle
    );
}

/// 断言 JSON 有效
pub fn assert_valid_json(json_str: &str) {
    let _: serde_json::Value = serde_json::from_str(json_str).expect("Invalid JSON format");
}

/// 断言响应非空
pub fn assert_non_empty_response(response: &str) {
    assert!(!response.trim().is_empty(), "Response should not be empty");
}

/// 断言响应长度在范围内
pub fn assert_response_length(response: &str, min: usize, max: usize) {
    let len = response.len();
    assert!(
        len >= min && len <= max,
        "Response length {} not in range [{}, {}]",
        len,
        min,
        max
    );
}
