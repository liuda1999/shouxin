//! 定时清理任务
//!
//! 实现事件分层清理逻辑和超时自动失效机制

use chrono::{Duration, Utc};
use sqlx::{query, Pool, Sqlite};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration as TokioDuration};

use crate::error::ShouxinError;

/// 清理服务状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceState {
    Idle,
    Running,
    Paused,
}

/// 短期闲聊保留天数
const SHORT_TERM_CHAT_DAYS: i64 = 3;

/// 长期事件保留天数
const LONG_TERM_EVENT_DAYS: i64 = 30;

/// 清理间隔（小时）
const CLEANUP_INTERVAL_HOURS: u64 = 1;

/// 定时清理服务
pub struct MaintenanceService {
    /// SQLite 连接池
    pool: Pool<Sqlite>,
    /// 清理间隔（小时）
    cleanup_interval_hours: u64,
    /// 是否启用自动清理
    auto_cleanup_enabled: bool,
    /// 当前状态
    state: Arc<Mutex<MaintenanceState>>,
}

impl MaintenanceService {
    /// 创建新的维护服务
    pub fn new(pool: Pool<Sqlite>) -> Self {
        Self {
            pool,
            cleanup_interval_hours: CLEANUP_INTERVAL_HOURS,
            auto_cleanup_enabled: false,
            state: Arc::new(Mutex::new(MaintenanceState::Idle)),
        }
    }

    /// 设置清理间隔
    pub fn with_cleanup_interval(mut self, hours: u64) -> Self {
        self.cleanup_interval_hours = hours;
        self
    }

    /// 启动自动清理
    pub async fn start_auto_cleanup(&mut self) -> Result<(), ShouxinError> {
        if self.auto_cleanup_enabled {
            return Ok(());
        }

        self.auto_cleanup_enabled = true;
        let state = self.state.clone();
        let pool = self.pool.clone();
        let interval_hours = self.cleanup_interval_hours;

        tokio::spawn(async move {
            let mut interval_timer = interval(TokioDuration::from_secs(interval_hours * 3600));

            let mut state_guard = state.lock().await;
            *state_guard = MaintenanceState::Running;
            drop(state_guard);

            loop {
                interval_timer.tick().await;

                let current_state = state.lock().await;
                if *current_state != MaintenanceState::Running {
                    break;
                }
                drop(current_state);

                let _ = Self::execute_cleanup(&pool).await;
            }
        });

        Ok(())
    }

    /// 手动执行清理
    pub async fn run_cleanup(&self) -> Result<CleanupResult, ShouxinError> {
        Self::execute_cleanup(&self.pool).await
    }

    /// 执行清理逻辑
    ///
    /// # 说明
    /// 按顺序执行以下清理任务：
    /// 1. 事件索引表超时处理
    /// 2. 待办表超时处理
    /// 3. 短期闲聊清理
    /// 4. 长期事件清理
    async fn execute_cleanup(pool: &Pool<Sqlite>) -> Result<CleanupResult, ShouxinError> {
        let mut result = CleanupResult::default();

        result.events_timed_out = Self::handle_event_timeouts(pool).await?;
        result.todos_timed_out = Self::handle_todo_timeouts(pool).await?;
        result.short_term_chats_removed = Self::handle_short_term_cleanup(pool).await?;
        result.long_term_events_removed = Self::handle_long_term_cleanup(pool).await?;

        Ok(result)
    }

    async fn handle_event_timeouts(pool: &Pool<Sqlite>) -> Result<usize, ShouxinError> {
        let mut tx = pool.begin().await?;
        let now = Utc::now();

        let result = query(
            r#"
            UPDATE event_index 
            SET status = 'completed',
                completed_at = ?,
                is_auto_invalid = 1
            WHERE deadline IS NOT NULL
            AND deadline < ?
            AND status = 'ongoing'
            AND is_auto_invalid = 0
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        let count = result.rows_affected() as usize;
        tx.commit().await?;
        Ok(count)
    }

    async fn handle_todo_timeouts(pool: &Pool<Sqlite>) -> Result<usize, ShouxinError> {
        let mut tx = pool.begin().await?;
        let now = Utc::now();

        let result = query(
            r#"
            UPDATE todo_list 
            SET status = 'auto_invalid'
            WHERE deadline < ?
            AND status = 'pending'
            "#,
        )
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        let count = result.rows_affected() as usize;
        tx.commit().await?;
        Ok(count)
    }

    async fn handle_short_term_cleanup(pool: &Pool<Sqlite>) -> Result<usize, ShouxinError> {
        let mut tx = pool.begin().await?;
        let short_term_cutoff = Utc::now() - Duration::days(SHORT_TERM_CHAT_DAYS);
        let result = query(
            r#"
            DELETE FROM event_index 
            WHERE importance = 'short_term' 
            AND created_at < ?
            "#,
        )
        .bind(short_term_cutoff.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        let count = result.rows_affected() as usize;
        tx.commit().await?;
        Ok(count)
    }

    async fn handle_long_term_cleanup(pool: &Pool<Sqlite>) -> Result<usize, ShouxinError> {
        let mut tx = pool.begin().await?;
        let long_term_cutoff = Utc::now() - Duration::days(LONG_TERM_EVENT_DAYS);
        let result = query(
            r#"
            DELETE FROM event_index 
            WHERE status = 'completed' 
            AND completed_at IS NOT NULL
            AND completed_at < ?
            AND importance != 'critical'
            "#,
        )
        .bind(long_term_cutoff.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        let count = result.rows_affected() as usize;
        tx.commit().await?;
        Ok(count)
    }

    /// 暂停自动清理
    pub async fn pause_auto_cleanup(&self) -> Result<(), ShouxinError> {
        let mut state = self.state.lock().await;
        if *state == MaintenanceState::Running {
            *state = MaintenanceState::Paused;
        }
        Ok(())
    }

    /// 恢复自动清理
    pub async fn resume_auto_cleanup(&self) -> Result<(), ShouxinError> {
        let mut state = self.state.lock().await;
        if *state == MaintenanceState::Paused {
            *state = MaintenanceState::Running;
        }
        Ok(())
    }

    /// 停止自动清理
    pub async fn stop_auto_cleanup(&mut self) -> Result<(), ShouxinError> {
        self.auto_cleanup_enabled = false;
        let mut state = self.state.lock().await;
        *state = MaintenanceState::Idle;
        Ok(())
    }

    /// 获取当前状态
    pub async fn get_state(&self) -> MaintenanceState {
        let state = self.state.lock().await;
        state.clone()
    }
}

impl Default for MaintenanceService {
    fn default() -> Self {
        Self::new(Pool::<Sqlite>::connect_lazy("sqlite::memory:").unwrap())
    }
}

/// 清理结果
#[derive(Debug, Clone, Default)]
pub struct CleanupResult {
    /// 超时自动失效的事件数量
    pub events_timed_out: usize,
    /// 超时自动失效的待办数量
    pub todos_timed_out: usize,
    /// 删除的短期闲聊数量
    pub short_term_chats_removed: usize,
    /// 删除的长期事件数量
    pub long_term_events_removed: usize,
}

impl CleanupResult {
    /// 获取总清理数量
    pub fn total_removed(&self) -> usize {
        self.short_term_chats_removed + self.long_term_events_removed
    }

    /// 获取总超时数量
    pub fn total_timed_out(&self) -> usize {
        self.events_timed_out + self.todos_timed_out
    }
}
