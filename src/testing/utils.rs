//! 测试工具函数
//!
//! 提供常用的测试辅助函数

use crate::{LlmConfig, Message, OllamaLlmClient};
use std::time::{Duration, Instant};

/// 创建测试客户端
pub fn create_test_client() -> OllamaLlmClient {
    OllamaLlmClient::new(super::config::get_test_model_name())
}

/// 创建带配置的测试客户端
pub fn create_test_client_with_config(config: LlmConfig) -> OllamaLlmClient {
    OllamaLlmClient::with_config(config)
}

/// 创建用户消息
pub fn user_msg(content: &str) -> Message {
    Message::user(content).expect("Failed to create user message")
}

/// 创建助手消息
pub fn assistant_msg(content: &str) -> Message {
    Message::assistant(content).expect("Failed to create assistant message")
}

/// 创建系统消息
pub fn system_msg(content: &str) -> Message {
    Message::system(content).expect("Failed to create system message")
}

/// 测量函数执行时间
pub fn measure_time<F, T>(f: F) -> (T, Duration)
where
    F: FnOnce() -> T,
{
    let start = Instant::now();
    let result = f();
    let duration = start.elapsed();
    (result, duration)
}

/// 异步测量函数执行时间
pub async fn measure_time_async<F, Fut, T>(f: F) -> (T, Duration)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let start = Instant::now();
    let result = f().await;
    let duration = start.elapsed();
    (result, duration)
}

/// 生成随机字符串（用于测试）
pub fn random_string(len: usize) -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// 生成随机中文文本（用于测试）
pub fn random_chinese_text(len: usize) -> String {
    use rand::Rng;
    const CHINESE_CHARS: &[&str] = &[
        "的", "一", "是", "在", "不", "了", "有", "和", "人", "这", "中", "大", "为", "上", "个",
        "国", "我", "以", "要", "他", "时", "来", "用", "们", "生", "到", "作", "地", "于", "出",
    ];
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let idx = rng.gen_range(0..CHINESE_CHARS.len());
            CHINESE_CHARS[idx]
        })
        .collect()
}

/// 生成超长文本（用于压力测试）
pub fn generate_long_text(repeat_times: usize) -> String {
    "测试内容".repeat(repeat_times)
}

/// 生成特殊字符文本（用于异常输入测试）
pub fn generate_special_chars() -> String {
    "!@#$%^&*()_+-=[]{}|;':\",./<>?~`".to_string()
}

/// 生成 Unicode 表情符号文本（用于异常输入测试）
pub fn generate_emoji_text() -> String {
    "😀🎉🚀💻🔥💯👍🌟🎯💡🎁🌈🎨".to_string()
}
