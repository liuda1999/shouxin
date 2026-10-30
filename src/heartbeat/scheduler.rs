use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

use crate::error::ShouxinError;
use crate::heartbeat::executor::HeartbeatExecutor;
use crate::heartbeat::question_pool::QuestionPool;
use crate::heartbeat::types::HeartbeatReport;
use crate::stores::RelationalStore;

const DEFAULT_INTERVAL_SECS: u64 = 360;
const MIN_MEMORY_THRESHOLD_MB: u64 = 4096;
const MAX_CONSECUTIVE_SKIPS: u32 = 3;

/// RAII guard: ensures `executing` flag is cleared on any path (including panic)
struct ExecutingGuard {
    executing: Arc<AtomicBool>,
}

impl Drop for ExecutingGuard {
    fn drop(&mut self) {
        self.executing.store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub base_interval_secs: u64,
    pub min_interval_secs: u64,
    pub max_interval_secs: u64,
    pub adaptive: bool,
    pub idle_threshold_secs: u64,
    pub memory_threshold_mb: u64,
    pub max_consecutive_skips: u32,
    /// 是否启用后台调度（false 时 `spawn` 不会启动循环）
    pub enabled: bool,
    /// 维护阶段超时（秒）
    pub maintenance_timeout_secs: u64,
    /// LLM 阶段超时（秒）
    pub llm_timeout_secs: u64,
    /// 单次画像生成超时（秒）；整轮超时至少要覆盖它
    pub profile_timeout_secs: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            base_interval_secs: DEFAULT_INTERVAL_SECS,
            min_interval_secs: 300,
            max_interval_secs: 600,
            adaptive: false,
            idle_threshold_secs: 300,
            memory_threshold_mb: MIN_MEMORY_THRESHOLD_MB,
            max_consecutive_skips: MAX_CONSECUTIVE_SKIPS,
            enabled: true,
            maintenance_timeout_secs: 30,
            llm_timeout_secs: 120,
            profile_timeout_secs: 240,
        }
    }
}

impl SchedulerConfig {
    /// 从应用配置构造调度器配置
    pub fn from_settings(settings: &crate::config::settings::HeartbeatSettings) -> Self {
        Self {
            base_interval_secs: settings.base_interval_secs,
            min_interval_secs: settings.min_interval_secs,
            max_interval_secs: settings.max_interval_secs,
            adaptive: settings.adaptive,
            idle_threshold_secs: settings.idle_threshold_secs,
            enabled: settings.enabled,
            maintenance_timeout_secs: settings.maintenance_timeout_secs,
            llm_timeout_secs: settings.llm_timeout_secs,
            profile_timeout_secs: settings.profile_timeout_secs,
            memory_threshold_mb: settings.memory_threshold_mb,
            max_consecutive_skips: settings.max_consecutive_skips,
            ..Self::default()
        }
    }

    /// 计算下一次心跳的间隔（秒）
    ///
    /// 自适应语义（`adaptive = true` 时生效）：
    /// - 最近有新记忆写入 → 用 `min_interval_secs`，尽快把新记忆梳理进画像/待办；
    /// - 空闲超过 `idle_threshold_secs` → 用 `max_interval_secs`，避免空转；
    /// - 其余情况 → `base_interval_secs`。
    ///
    /// 结果始终被夹到 `[min, max]` 区间，避免配置写反导致异常节奏。
    pub fn next_interval_secs(&self, secs_since_last_memory_write: u64) -> u64 {
        let min = self.min_interval_secs.max(1);
        let max = self.max_interval_secs.max(min);

        let raw = if !self.adaptive {
            self.base_interval_secs
        } else if secs_since_last_memory_write >= self.idle_threshold_secs {
            max
        } else if secs_since_last_memory_write <= min {
            min
        } else {
            self.base_interval_secs
        };

        raw.clamp(min, max)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatStatus {
    pub is_running: bool,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub last_memory_write: DateTime<Utc>,
    pub pending_questions_count: usize,
    pub next_heartbeat_in_secs: Option<u64>,
    pub consecutive_skips: u32,
    pub last_skipped: bool,
    pub available_memory_mb: Option<u64>,
    pub maintenance_only: bool,
}

pub struct HeartbeatScheduler {
    executor: Arc<HeartbeatExecutor>,
    question_pool: Arc<QuestionPool>,
    config: SchedulerConfig,
    last_heartbeat: Arc<Mutex<Option<DateTime<Utc>>>>,
    last_memory_write: Arc<Mutex<DateTime<Utc>>>,
    is_running: Arc<Mutex<bool>>,
    should_stop: Arc<Mutex<bool>>,
    consecutive_skips: Arc<Mutex<u32>>,
    executing: Arc<AtomicBool>,
    last_maintenance_only: Arc<Mutex<bool>>,
}

impl HeartbeatScheduler {
    pub fn question_pool(&self) -> Arc<QuestionPool> {
        self.question_pool.clone()
    }

    pub fn rel_store(&self) -> Arc<RelationalStore> {
        self.executor.rel_store()
    }

    pub fn new(
        executor: Arc<HeartbeatExecutor>,
        question_pool: Arc<QuestionPool>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            executor,
            question_pool,
            config,
            last_heartbeat: Arc::new(Mutex::new(None)),
            last_memory_write: Arc::new(Mutex::new(Utc::now())),
            is_running: Arc::new(Mutex::new(false)),
            should_stop: Arc::new(Mutex::new(false)),
            consecutive_skips: Arc::new(Mutex::new(0)),
            executing: Arc::new(AtomicBool::new(false)),
            last_maintenance_only: Arc::new(Mutex::new(false)),
        }
    }

    pub fn spawn(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let scheduler = self.clone();
        tokio::spawn(async move {
            // F-H0: 配置里的 [heartbeat].enabled 必须真正生效
            if !scheduler.config.enabled {
                warn!("Heartbeat scheduler disabled by configuration ([heartbeat].enabled = false)");
                return;
            }
            scheduler.run().await;
        })
    }

    async fn run(&self) {
        *self.is_running.lock().await = true;
        *self.should_stop.lock().await = false;

        info!(
            "Heartbeat scheduler started (base={}s, min={}s, max={}s, adaptive={})",
            self.config.base_interval_secs,
            self.config.min_interval_secs,
            self.config.max_interval_secs,
            self.config.adaptive
        );

        while !*self.should_stop.lock().await {
            // F-H1: 自适应间隔——按"距上次记忆写入的时间"决定下一次心跳节奏
            let interval_secs = {
                let last_write = *self.last_memory_write.lock().await;
                let elapsed = (Utc::now() - last_write).num_seconds().max(0) as u64;
                self.config.next_interval_secs(elapsed)
            };
            let wait_duration = Duration::from_secs(interval_secs);
            info!(
                "Next heartbeat in {}s (adaptive={})",
                interval_secs, self.config.adaptive
            );

            tokio::select! {
                _ = sleep(wait_duration) => {
                    match self.check_and_execute().await {
                        Ok(_) => {}
                        Err(e) => {
                            warn!("Heartbeat execution failed: {:?}", e);
                        }
                    }
                }
                _ = async {
                    while !*self.should_stop.lock().await {
                        sleep(Duration::from_millis(100)).await;
                    }
                } => {
                    break;
                }
            }
        }

        *self.is_running.lock().await = false;
        info!("Heartbeat scheduler stopped");
    }

    async fn check_and_execute(&self) -> Result<(), ShouxinError> {
        let available_memory_mb = get_available_memory_mb();

        info!(
            "Heartbeat check: available memory = {} MB (threshold: {} MB)",
            available_memory_mb, self.config.memory_threshold_mb
        );

        let consecutive_skips = *self.consecutive_skips.lock().await;

        if available_memory_mb < self.config.memory_threshold_mb {
            let mut skips = self.consecutive_skips.lock().await;
            *skips += 1;

            if *skips < self.config.max_consecutive_skips {
                warn!(
                    "Skipping heartbeat due to low memory (available: {} MB, threshold: {} MB). \
                     Consecutive skip #{}/{}",
                    available_memory_mb,
                    self.config.memory_threshold_mb,
                    *skips,
                    self.config.max_consecutive_skips
                );
                return Ok(());
            } else {
                warn!(
                    "Memory still low but reached max consecutive skips ({}). \
                     Forcing heartbeat execution.",
                    self.config.max_consecutive_skips
                );
                *skips = 0;
            }
        } else {
            let mut skips = self.consecutive_skips.lock().await;
            if *skips > 0 {
                info!("Memory available again, resetting skip counter");
                *skips = 0;
            }
        }

        if let Err(e) = self.trigger_now(false).await {
            warn!("Heartbeat execution failed: {:?}", e);
            return Err(e);
        }

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), ShouxinError> {
        info!("Shutting down heartbeat scheduler");
        *self.should_stop.lock().await = true;
        Ok(())
    }

    pub async fn notify_memory_written(&self) {
        *self.last_memory_write.lock().await = Utc::now();
    }

    pub async fn trigger_now(&self, maintenance_only: bool) -> Result<HeartbeatReport, ShouxinError> {
        if self.executing.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return Err(ShouxinError::ConcurrencyError(
                "Heartbeat is already executing".to_string(),
            ));
        }

        // RAII guard: ensures executing flag is cleared on any path (including panic/timeout)
        let _guard = ExecutingGuard { executing: self.executing.clone() };

        // P2-6: 记录本次心跳模式
        *self.last_maintenance_only.lock().await = maintenance_only;

        if maintenance_only {
            info!("Manually triggering maintenance-only heartbeat (no LLM)");

            // 仅执行维护心跳（纯 SQL，永不调用 LLM；超时可配置）
            let maintenance_timeout = Duration::from_secs(self.config.maintenance_timeout_secs);
            let maintenance_future = self.executor.run_maintenance_heartbeat();
            let maintenance_result = tokio::time::timeout(maintenance_timeout, maintenance_future).await;

            match maintenance_result {
                Ok(Ok(report)) => {
                    info!("Maintenance-only heartbeat completed in {}ms", report.duration_ms);
                    let mut hb = self.last_heartbeat.lock().await;
                    *hb = Some(report.completed_at);
                    Ok(report)
                }
                Ok(Err(e)) => {
                    warn!("Maintenance-only heartbeat failed: {:?}", e);
                    Err(e)
                }
                Err(_elapsed) => {
                    warn!(
                        "Maintenance-only heartbeat timed out after {}s",
                        self.config.maintenance_timeout_secs
                    );
                    Err(ShouxinError::LLMError(
                        "Maintenance heartbeat timed out".to_string(),
                    ))
                }
            }
        } else {
            info!("Manually triggering full heartbeat (maintenance + LLM)");

            // Step 1: 维护心跳（纯 SQL，永不调用 LLM；超时可配置）
            let maintenance_timeout = Duration::from_secs(self.config.maintenance_timeout_secs);
            let maintenance_future = self.executor.run_maintenance_heartbeat();
            let maintenance_result = tokio::time::timeout(maintenance_timeout, maintenance_future).await;

            let maintenance_report = match maintenance_result {
                Ok(Ok(report)) => {
                    info!("Maintenance heartbeat completed in {}ms", report.duration_ms);
                    // 更新 last_heartbeat
                    let mut hb = self.last_heartbeat.lock().await;
                    *hb = Some(report.completed_at);
                    Some(report)
                }
                Ok(Err(e)) => {
                    warn!("Maintenance heartbeat failed: {:?}", e);
                    None
                }
                Err(_elapsed) => {
                    warn!(
                        "Maintenance heartbeat timed out after {}s",
                        self.config.maintenance_timeout_secs
                    );
                    None
                }
            };

            // Step 2: LLM 心跳（超时可配置）
            //
            // 整轮超时不能小于"单次画像超时"，否则画像刚跑到一半就被整轮掐断：
            // 旧配置 120s 整轮 + 90s 单次，导致画像永远无法落库。
            let round_secs = self
                .config
                .llm_timeout_secs
                .max(self.config.profile_timeout_secs + 60);
            let llm_timeout = Duration::from_secs(round_secs);
            let llm_future = self.executor.run_llm_heartbeat();
            let llm_result = tokio::time::timeout(llm_timeout, llm_future).await;

            let llm_report = match llm_result {
                Ok(Ok(report)) => {
                    info!("LLM heartbeat completed in {}ms", report.duration_ms);
                    // 更新 last_heartbeat
                    let mut hb = self.last_heartbeat.lock().await;
                    *hb = Some(report.completed_at);
                    Some(report)
                }
                Ok(Err(e)) => {
                    warn!("LLM heartbeat failed: {:?}", e);
                    None
                }
                Err(_elapsed) => {
                    warn!("LLM heartbeat timed out after {}s", round_secs);
                    None
                }
            };

            // executing flag is cleared by _guard on drop

            // 返回合并报告
            match (maintenance_report, llm_report) {
                (Some(mut m), Some(l)) => {
                    m.duration_ms += l.duration_ms;
                    m.actions_generated = l.actions_generated;
                    m.questions_created += l.questions_created;
                    m.contacts_updated += l.contacts_updated;
                    m.events_cleaned += l.events_cleaned;
                    m.events_merged += l.events_merged;
                    m.contacts_merged += l.contacts_merged;
                    m.ambiguities_detected += l.ambiguities_detected;
                    m.profiles_updated += l.profiles_updated;
                    m.maintenance_only = false;
                    Ok(m)
                }
                (Some(m), None) => Ok(m),
                (None, Some(l)) => Ok(l),
                (None, None) => Err(ShouxinError::LLMError(
                    "Both maintenance and LLM heartbeat failed".to_string(),
                )),
            }
        }
    }

    pub async fn get_status(&self) -> HeartbeatStatus {
        let is_running = *self.is_running.lock().await;
        let last_heartbeat = *self.last_heartbeat.lock().await;
        let last_memory_write = *self.last_memory_write.lock().await;
        let consecutive_skips = *self.consecutive_skips.lock().await;
        let maintenance_only = *self.last_maintenance_only.lock().await;

        let pending_questions_count = self
            .question_pool
            .get_pending_questions(usize::MAX)
            .await
            .unwrap_or_default()
            .len();

        let available_memory_mb = get_available_memory_mb();

        HeartbeatStatus {
            is_running,
            last_heartbeat,
            last_memory_write,
            pending_questions_count,
            next_heartbeat_in_secs: if is_running {
                Some(self.config.base_interval_secs)
            } else {
                None
            },
            consecutive_skips,
            last_skipped: consecutive_skips > 0,
            available_memory_mb: Some(available_memory_mb),
            maintenance_only,
        }
    }
}

pub fn get_available_memory_mb() -> u64 {
    let mut sys = System::new_all();
    sys.refresh_memory();
    sys.available_memory() / (1024 * 1024)
}

pub fn get_total_memory_mb() -> u64 {
    let mut sys = System::new_all();
    sys.refresh_memory();
    sys.total_memory() / (1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(adaptive: bool) -> SchedulerConfig {
        SchedulerConfig {
            base_interval_secs: 600,
            min_interval_secs: 120,
            max_interval_secs: 1800,
            adaptive,
            idle_threshold_secs: 300,
            ..SchedulerConfig::default()
        }
    }

    #[test]
    fn test_fixed_interval_when_not_adaptive() {
        let cfg = config(false);
        // 非自适应：无论空闲多久都用 base
        assert_eq!(cfg.next_interval_secs(0), 600);
        assert_eq!(cfg.next_interval_secs(10_000), 600);
    }

    #[test]
    fn test_adaptive_interval_reacts_to_activity() {
        let cfg = config(true);
        // 刚写过记忆 -> 用最小间隔尽快梳理
        assert_eq!(cfg.next_interval_secs(0), 120);
        // 长时间空闲 -> 拉长到最大间隔
        assert_eq!(cfg.next_interval_secs(400), 1800);
        // 介于两者之间 -> base
        assert_eq!(cfg.next_interval_secs(200), 600);
    }

    #[test]
    fn test_interval_is_clamped() {
        let cfg = SchedulerConfig {
            base_interval_secs: 10,
            min_interval_secs: 120,
            max_interval_secs: 180,
            adaptive: false,
            ..SchedulerConfig::default()
        };
        // 配置写反时结果仍被夹在 [min, max]
        assert_eq!(cfg.next_interval_secs(0), 120);
    }
}
