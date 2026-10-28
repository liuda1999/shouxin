//! LLM 客户端管理器
//!
//! 管理多源 LLM 客户端，支持主备切换和健康检查。
//! 使用 AtomicUsize 实现无锁切换，Arc 共享客户端。

use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

use crate::core::{Fact, Memory, MemoryAction, Message};
use crate::error::{Result, ShouxinError};
use crate::traits::llm::LlmClient;

const CURRENT_PRIMARY: usize = 0;
const CURRENT_SECONDARY: usize = 1;

/// 模型源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmSourceType {
    Ollama,
    Gguf,
    Api,
}

/// 管理器状态
#[derive(Debug, Clone)]
pub struct ManagerStatus {
    pub active_source: String,
    pub is_primary: bool,
    pub total_requests: u64,
    pub total_failures: u64,
    pub health_check_interval_secs: u64,
    pub primary_healthy: bool,
    pub secondary_healthy: Option<bool>,
    pub auto_switch_enabled: bool,
}

/// LLM 客户端管理器
pub struct LlmClientManager {
    primary: Box<dyn LlmClient>,
    primary_source_type: LlmSourceType,
    secondary: Option<Box<dyn LlmClient>>,
    secondary_source_type: Option<LlmSourceType>,
    /// 0 = primary, 1 = secondary
    current: AtomicUsize,
    total_requests: AtomicUsize,
    total_failures: AtomicUsize,
    auto_switch: AtomicUsize, // 0=disabled, 1=enabled
    consecutive_primary_failures: AtomicUsize,
    max_consecutive_failures: usize,
    health_interval_secs: u64,
    /// 健康状态（可被后台健康检查更新）
    status: RwLock<ManagerStatus>,
}

impl LlmClientManager {
    /// 创建管理器
    /// 
    /// `primary` - 主客户端
    /// `primary_source` - 主客户端来源类型
    /// `secondary` - 可选备客户端
    /// `secondary_source` - 备客户端来源类型
    /// `auto_switch` - 是否启用自动主备切换
    /// `max_consecutive_failures` - 连续失败多少次后自动切换
    /// `health_interval_secs` - 健康检查间隔（秒），0 表示不启用
    pub fn new(
        primary: Box<dyn LlmClient>,
        primary_source: LlmSourceType,
        secondary: Option<Box<dyn LlmClient>>,
        secondary_source: Option<LlmSourceType>,
        auto_switch: bool,
        max_consecutive_failures: usize,
        health_interval_secs: u64,
    ) -> Self {
        let secondary_healthy = secondary.is_some().then_some(true);

        let status = ManagerStatus {
            active_source: format!("{:?}", primary_source).to_lowercase(),
            is_primary: true,
            total_requests: 0,
            total_failures: 0,
            health_check_interval_secs: health_interval_secs,
            primary_healthy: true,
            secondary_healthy,
            auto_switch_enabled: auto_switch,
        };

        Self {
            primary,
            primary_source_type: primary_source,
            secondary,
            secondary_source_type: secondary_source,
            current: AtomicUsize::new(CURRENT_PRIMARY),
            total_requests: AtomicUsize::new(0),
            total_failures: AtomicUsize::new(0),
            auto_switch: AtomicUsize::new(if auto_switch { 1 } else { 0 }),
            consecutive_primary_failures: AtomicUsize::new(0),
            max_consecutive_failures,
            health_interval_secs,
            status: RwLock::new(status),
        }
    }

    /// 当前活动的客户端
    fn active_client(&self) -> &dyn LlmClient {
        if self.current.load(Ordering::Relaxed) == CURRENT_SECONDARY {
            if let Some(sec) = &self.secondary {
                return sec.as_ref();
            }
        }
        self.primary.as_ref()
    }

    fn current_is_primary(&self) -> bool {
        self.current.load(Ordering::Relaxed) == CURRENT_PRIMARY
    }

    /// 切换到备模型
    pub async fn switch_to_secondary(&self) -> Result<()> {
        if self.secondary.is_none() {
            return Err(ShouxinError::LLMError("未配置备模型".to_string()));
        }

        self.current.store(CURRENT_SECONDARY, Ordering::SeqCst);
        let source_name = self.secondary_source_type
            .map(|s| format!("{:?}", s).to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        tracing::warn!("已切换到备模型: {}", source_name);

        let mut status = self.status.write().await;
        status.active_source = source_name;
        status.is_primary = false;

        Ok(())
    }

    /// 切换回主模型
    pub async fn switch_to_primary(&self) -> Result<()> {
        self.current.store(CURRENT_PRIMARY, Ordering::SeqCst);
        self.consecutive_primary_failures.store(0, Ordering::SeqCst);

        let source_name = format!("{:?}", self.primary_source_type).to_lowercase();
        tracing::info!("已切换回主模型: {}", source_name);

        let mut status = self.status.write().await;
        status.active_source = source_name;
        status.is_primary = true;
        status.primary_healthy = true;

        Ok(())
    }

    /// 手动切换到指定源
    pub async fn switch_to(&self, target: &str) -> Result<String> {
        match target.to_lowercase().as_str() {
            "primary" | "main" => {
                self.switch_to_primary().await?;
                Ok("primary".to_string())
            }
            "secondary" | "backup" | "fallback" => {
                self.switch_to_secondary().await?;
                Ok("secondary".to_string())
            }
            _ => Err(ShouxinError::LLMError(format!(
                "未知的模型目标: {}。可选: primary, secondary", target
            ))),
        }
    }

    /// 记录请求成功
    fn record_success(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if self.current_is_primary() {
            self.consecutive_primary_failures.store(0, Ordering::Relaxed);
        }
    }

    /// 记录请求失败，必要时自动切换
    async fn record_failure(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.total_failures.fetch_add(1, Ordering::Relaxed);

        if self.current_is_primary()
            && self.auto_switch.load(Ordering::Relaxed) == 1
            && self.secondary.is_some()
        {
            let failures = self.consecutive_primary_failures.fetch_add(1, Ordering::Relaxed) + 1;
            if failures >= self.max_consecutive_failures {
                tracing::warn!(
                    "主模型连续失败 {} 次，自动切换到备模型",
                    failures
                );
                let _ = self.switch_to_secondary().await;
            }
        }
    }

    /// 获取当前状态
    pub async fn get_status(&self) -> ManagerStatus {
        let status = self.status.read().await;
        ManagerStatus {
            active_source: status.active_source.clone(),
            is_primary: status.is_primary,
            total_requests: self.total_requests.load(Ordering::Relaxed) as u64,
            total_failures: self.total_failures.load(Ordering::Relaxed) as u64,
            health_check_interval_secs: status.health_check_interval_secs,
            primary_healthy: status.primary_healthy,
            secondary_healthy: status.secondary_healthy,
            auto_switch_enabled: status.auto_switch_enabled,
        }
    }

    /// 启动后台健康检查
    pub fn start_health_check(self: &Arc<Self>) {
        if self.health_interval_secs == 0 {
            return;
        }

        let manager = Arc::clone(self);
        let interval_dur = Duration::from_secs(self.health_interval_secs);

        tokio::spawn(async move {
            let mut ticker = interval(interval_dur);

            loop {
                ticker.tick().await;
                manager.perform_health_check().await;
            }
        });
    }

    async fn perform_health_check(&self) {
        // 检查主模型
        let primary_healthy = self.primary.complete("ping").await.is_ok();

        let mut status = self.status.write().await;
        status.primary_healthy = primary_healthy;

        if primary_healthy && !self.current_is_primary() {
            tracing::info!("主模型已恢复，自动切回");
            drop(status);
            let _ = self.switch_to_primary().await;
        } else if !primary_healthy && self.current_is_primary() {
            tracing::warn!("主模型健康检查失败");
            if self.secondary.is_some() && self.auto_switch.load(Ordering::Relaxed) == 1 {
                drop(status);
                let _ = self.switch_to_secondary().await;
                return;
            }
        }

        // 检查备模型（如果已切换到备模型）
        if !self.current_is_primary() {
            if let Some(sec) = &self.secondary {
                let sec_healthy = sec.complete("ping").await.is_ok();
                let mut status = self.status.write().await;
                status.secondary_healthy = Some(sec_healthy);
            }
        }
    }

    /// 获取当前使用的客户端引用（用于 Arc<Self> 场景）
    pub fn client(&self) -> &dyn LlmClient {
        self.active_client()
    }

    /// 带指数退避的重试执行器
    ///
    /// 对 LLM 调用的临时错误（超时、panic、连接断开）自动重试
    async fn retry_with_backoff<F, Fut, T>(
        &self,
        max_attempts: u32,
        f: F,
    ) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_err = None;
        for attempt in 0..max_attempts {
            if attempt > 0 {
                let delay_ms = 2u64.pow(attempt) * 1000;
                tracing::debug!("LLM retry attempt {}/{} after {}ms", attempt + 1, max_attempts, delay_ms);
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            match f().await {
                Ok(result) => {
                    if attempt > 0 {
                        tracing::info!("LLM call succeeded on retry attempt {}", attempt + 1);
                    }
                    self.record_success();
                    return Ok(result);
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < max_attempts {
                        tracing::warn!("LLM call failed, will retry: {}", last_err.as_ref().unwrap());
                    }
                }
            }
        }
        self.record_failure().await;
        Err(last_err.unwrap())
    }
}

/// 为 LlmClientManager 实现 LlmClient trait（代理模式）
///
/// 所有请求通过当前活动的客户端执行，自动记录成功/失败
/// 临时错误（超时/panic/连接断开）自动重试1次
#[async_trait]
impl LlmClient for LlmClientManager {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        self.retry_with_backoff(2, || {
            let msgs = messages.clone();
            async move { self.active_client().chat(msgs).await }
        }).await
    }

    async fn extract_facts(&self, conversation: &str) -> Result<Vec<Fact>> {
        self.retry_with_backoff(2, || {
            let conv = conversation.to_string();
            async move { self.active_client().extract_facts(&conv).await }
        }).await
    }

    async fn decide_memory_action(
        &self,
        fact: &Fact,
        existing_memories: Vec<&Memory>,
    ) -> Result<MemoryAction> {
        self.retry_with_backoff(2, || {
            let f = fact.clone();
            let em: Vec<Memory> = existing_memories.iter().map(|m| (*m).clone()).collect();
            async move { self.active_client().decide_memory_action(&f, em.iter().collect()).await }
        }).await
    }

    fn model_name(&self) -> &str {
        self.active_client().model_name()
    }

    async fn preload(&self) -> Result<()> {
        self.primary.preload().await?;
        if let Some(ref sec) = self.secondary {
            if let Err(e) = sec.preload().await {
                tracing::warn!("Failed to preload secondary model: {}", e);
            }
        }
        Ok(())
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        self.retry_with_backoff(2, || {
            let p = prompt.to_string();
            async move { self.active_client().complete(&p).await }
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockClient {
        name: String,
        should_fail: bool,
    }

    #[async_trait]
    impl LlmClient for MockClient {
        async fn chat(&self, _: Vec<Message>) -> Result<String> {
            if self.should_fail {
                Err(ShouxinError::LLMError("mock failure".to_string()))
            } else {
                Ok("mock response".to_string())
            }
        }

        async fn extract_facts(&self, _: &str) -> Result<Vec<Fact>> {
            Ok(Vec::new())
        }

        async fn decide_memory_action(&self, _: &Fact, _: Vec<&Memory>) -> Result<MemoryAction> {
            Ok(MemoryAction::Add)
        }

        fn model_name(&self) -> &str {
            &self.name
        }

        async fn complete(&self, _: &str) -> Result<String> {
            if self.should_fail {
                Err(ShouxinError::LLMError("mock complete failure".to_string()))
            } else {
                Ok("pong".to_string())
            }
        }
    }

    #[tokio::test]
    async fn test_switch_to_secondary() {
        let primary = Box::new(MockClient { name: "primary".to_string(), should_fail: false });
        let secondary = Box::new(MockClient { name: "secondary".to_string(), should_fail: false });

        let manager = Arc::new(LlmClientManager::new(
            primary, LlmSourceType::Ollama,
            Some(secondary), Some(LlmSourceType::Api),
            false, 3, 0,
        ));

        assert_eq!(manager.model_name(), "primary");

        manager.switch_to_secondary().await.unwrap();
        assert_eq!(manager.model_name(), "secondary");

        manager.switch_to_primary().await.unwrap();
        assert_eq!(manager.model_name(), "primary");
    }

    #[tokio::test]
    async fn test_auto_switch_on_failure() {
        let primary = Box::new(MockClient { name: "primary".to_string(), should_fail: true });
        let secondary = Box::new(MockClient { name: "secondary".to_string(), should_fail: false });

        let manager = Arc::new(LlmClientManager::new(
            primary, LlmSourceType::Ollama,
            Some(secondary), Some(LlmSourceType::Api),
            true, 1, 0,
        ));

        // 第一次请求失败 → 自动切换到 secondary
        let result = manager.chat(vec![]).await;
        assert!(result.is_err()); // 第一次仍然失败
        assert_eq!(manager.model_name(), "secondary"); // 但已切换
    }
}