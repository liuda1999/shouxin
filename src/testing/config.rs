//! 测试配置模块
//!
//! 统一管理测试配置，包括模型名称、超时时间等

use std::time::Duration;

/// 测试配置结构体
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// 模型名称
    pub model_name: String,
    /// 请求超时时间
    pub timeout: Duration,
    /// 最大重试次数
    pub max_retries: u32,
    /// 是否启用详细日志
    pub verbose: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            model_name: "llama3.2:3b".to_string(),
            timeout: Duration::from_secs(120),
            max_retries: 3,
            verbose: false,
        }
    }
}

impl TestConfig {
    /// 创建新的测试配置
    pub fn new(model_name: &str) -> Self {
        Self {
            model_name: model_name.to_string(),
            ..Default::default()
        }
    }

    /// 设置模型名称
    pub fn with_model(mut self, model_name: &str) -> Self {
        self.model_name = model_name.to_string();
        self
    }

    /// 设置超时时间
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 设置最大重试次数
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// 设置详细日志
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }
}

/// 全局测试配置
pub static TEST_CONFIG: once_cell::sync::Lazy<TestConfig> = once_cell::sync::Lazy::new(|| {
    TestConfig::new(&std::env::var("TEST_MODEL_NAME").unwrap_or_else(|_| "llama3.2:3b".to_string()))
});

/// 获取当前测试模型名称
pub fn get_test_model_name() -> &'static str {
    &TEST_CONFIG.model_name
}

/// 获取测试配置
pub fn get_test_config() -> &'static TestConfig {
    &TEST_CONFIG
}
