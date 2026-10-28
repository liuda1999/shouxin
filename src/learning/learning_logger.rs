#![allow(clippy::should_implement_trait)]

//! 学习日志系统
//!
//! 记录学习过程、错误、特性请求的标准化机制

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::error::ShouxinError;

/// 日志类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogType {
    /// 学习日志
    Learning,
    /// 错误日志
    Error,
    /// 特性请求日志
    FeatureRequest,
}

impl LogType {
    /// 获取日志类型对应的文件名
    pub fn filename(&self) -> &'static str {
        match self {
            Self::Learning => "LEARNINGS.md",
            Self::Error => "ERRORS.md",
            Self::FeatureRequest => "FEATURE_REQUESTS.md",
        }
    }

    /// 获取日志类型的标题
    pub fn title(&self) -> &'static str {
        match self {
            Self::Learning => "学习日志",
            Self::Error => "错误记录",
            Self::FeatureRequest => "特性请求",
        }
    }
}

/// 学习状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LearningStatus {
    /// 待处理
    Pending,
    /// 进行中
    InProgress,
    /// 已完成
    Completed,
}

impl LearningStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "InProgress",
            Self::Completed => "Completed",
        }
    }
}

/// 学习日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEntry {
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 学习主题
    pub topic: String,
    /// 优先级
    pub priority: Priority,
    /// 状态
    pub status: LearningStatus,
    /// 领域
    pub area: String,
    /// 学习内容摘要
    pub summary: String,
    /// 关键知识点
    pub key_points: Vec<String>,
    /// 建议行动
    pub suggested_action: Option<String>,
    /// 元数据
    pub metadata: Vec<String>,
    /// 反思与总结
    pub reflection: Option<String>,
    /// 相关资源链接
    pub resources: Vec<String>,
    /// 标签
    pub tags: Vec<String>,
}

impl LearningEntry {
    /// 创建新的学习日志条目
    pub fn new(topic: String, summary: String) -> Self {
        Self {
            timestamp: Utc::now(),
            topic,
            priority: Priority::Medium,
            status: LearningStatus::InProgress,
            area: "Core".to_string(),
            summary,
            key_points: Vec::new(),
            suggested_action: None,
            metadata: Vec::new(),
            reflection: None,
            resources: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// 设置优先级
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = priority;
        self
    }

    /// 设置状态
    pub fn with_status(mut self, status: LearningStatus) -> Self {
        self.status = status;
        self
    }

    /// 设置领域
    pub fn with_area(mut self, area: String) -> Self {
        self.area = area;
        self
    }

    /// 添加关键知识点
    pub fn with_key_point(mut self, point: String) -> Self {
        self.key_points.push(point);
        self
    }

    /// 添加建议行动
    pub fn with_suggested_action(mut self, action: String) -> Self {
        self.suggested_action = Some(action);
        self
    }

    /// 添加元数据
    pub fn with_metadata(mut self, metadata: String) -> Self {
        self.metadata.push(metadata);
        self
    }

    /// 添加反思
    pub fn with_reflection(mut self, reflection: String) -> Self {
        self.reflection = Some(reflection);
        self
    }

    /// 添加资源链接
    pub fn with_resource(mut self, resource: String) -> Self {
        self.resources.push(resource);
        self
    }

    /// 添加标签
    pub fn with_tag(mut self, tag: String) -> Self {
        self.tags.push(tag);
        self
    }
}

/// 错误日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEntry {
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 错误类型
    pub error_type: String,
    /// 错误消息
    pub message: String,
    /// 错误上下文/场景
    pub context: String,
    /// 堆栈跟踪（如果有）
    pub stack_trace: Option<String>,
    /// 解决方案（如果已知）
    pub solution: Option<String>,
    /// 相关错误ID
    pub related_errors: Vec<String>,
}

impl ErrorEntry {
    /// 创建新的错误日志条目
    pub fn new(error_type: String, message: String, context: String) -> Self {
        Self {
            timestamp: Utc::now(),
            error_type,
            message,
            context,
            stack_trace: None,
            solution: None,
            related_errors: Vec::new(),
        }
    }

    /// 添加堆栈跟踪
    pub fn with_stack_trace(mut self, stack_trace: String) -> Self {
        self.stack_trace = Some(stack_trace);
        self
    }

    /// 添加解决方案
    pub fn with_solution(mut self, solution: String) -> Self {
        self.solution = Some(solution);
        self
    }
}

/// 特性请求日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRequestEntry {
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 特性名称
    pub feature_name: String,
    /// 特性描述
    pub description: String,
    /// 使用场景
    pub use_case: String,
    /// 优先级
    pub priority: Priority,
    /// 预期效果
    pub expected_impact: String,
    /// 状态
    pub status: FeatureStatus,
    /// 相关特性
    pub related_features: Vec<String>,
}

/// 优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Priority {
    /// 低优先级
    Low,
    /// 中优先级
    Medium,
    /// 高优先级
    High,
    /// 紧急
    Critical,
}

impl Priority {
    /// 从字符串转换为 Priority
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }
}

/// 特性状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureStatus {
    /// 待处理
    Pending,
    /// 进行中
    InProgress,
    /// 已完成
    Completed,
    /// 已拒绝
    Rejected,
}

impl FeatureStatus {
    /// 从字符串转换为 FeatureStatus
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "in_progress" | "inprogress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }

    /// 转换为字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::InProgress => "In Progress",
            Self::Completed => "Completed",
            Self::Rejected => "Rejected",
        }
    }
}

/// 学习日志记录器
///
/// 线程安全的日志写入工具
pub struct LearningLogger {
    /// 日志目录路径
    log_dir: PathBuf,
    /// 单个日志文件最大大小（字节）
    max_file_size: u64,
}

impl LearningLogger {
    /// 创建新的学习日志记录器
    ///
    /// # 参数
    /// - `log_dir`: 日志目录路径
    ///
    /// # 返回
    /// 新的学习日志记录器实例
    pub fn new(log_dir: PathBuf) -> Self {
        Self {
            log_dir,
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
    }

    /// 创建带有自定义配置的学习日志记录器
    ///
    /// # 参数
    /// - `log_dir`: 日志目录路径
    /// - `max_file_size`: 单个日志文件最大大小（字节）
    ///
    /// # 返回
    /// 配置后的学习日志记录器实例
    pub fn with_config(log_dir: PathBuf, max_file_size: u64) -> Self {
        Self { log_dir, max_file_size }
    }

    /// 初始化日志目录
    pub async fn init(&self) -> Result<(), ShouxinError> {
        fs::create_dir_all(&self.log_dir).await?;

        for log_type in &[LogType::Learning, LogType::Error, LogType::FeatureRequest] {
            let file_path = self.log_dir.join(log_type.filename());
            if !file_path.exists() {
                let mut file = fs::File::create(&file_path).await?;
                let header = format!("# {}\n\n<!-- 自动生成，请勿修改 -->\n\n", log_type.title());
                file.write_all(header.as_bytes()).await?;
            }
        }

        Ok(())
    }

    /// 写入学习日志
    pub async fn log_learning(&self, entry: &LearningEntry) -> Result<(), ShouxinError> {
        let content = self.format_learning_entry(entry);
        self.append_to_log(LogType::Learning, &content).await
    }

    /// 写入错误日志
    pub async fn log_error(&self, entry: &ErrorEntry) -> Result<(), ShouxinError> {
        let content = self.format_error_entry(entry);
        self.append_to_log(LogType::Error, &content).await
    }

    /// 写入特性请求日志
    pub async fn log_feature_request(&self, entry: &FeatureRequestEntry) -> Result<(), ShouxinError> {
        let content = self.format_feature_request_entry(entry);
        self.append_to_log(LogType::FeatureRequest, &content).await
    }

    /// 追加内容到日志文件
    async fn append_to_log(&self, log_type: LogType, content: &str) -> Result<(), ShouxinError> {
        let file_path = self.log_dir.join(log_type.filename());

        if self.should_rotate(&file_path).await? {
            self.rotate_log(&file_path).await?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .await?;

        file.write_all(content.as_bytes()).await?;
        Ok(())
    }

    /// 检查是否需要日志轮转
    async fn should_rotate(&self, file_path: &PathBuf) -> Result<bool, ShouxinError> {
        let metadata = fs::metadata(file_path).await?;
        Ok(metadata.len() > self.max_file_size)
    }

    /// 执行日志轮转
    async fn rotate_log(&self, file_path: &PathBuf) -> Result<(), ShouxinError> {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let rotated_name = format!(
            "{}.{}",
            file_path.file_stem().unwrap().to_string_lossy(),
            timestamp
        );
        let rotated_path = self.log_dir.join(format!(
            "{}_{}.log",
            rotated_name,
            file_path.extension().unwrap().to_string_lossy()
        ));

        fs::rename(file_path, &rotated_path).await?;
        Ok(())
    }

    /// 格式化学习日志条目
    fn format_learning_entry(&self, entry: &LearningEntry) -> String {
        let mut content = String::new();
        content.push_str(&format!("## {}\n\n", entry.topic));
        content.push_str(&format!("**Logged**: {}\n\n", entry.timestamp.format("%Y-%m-%d %H:%M:%S")));
        content.push_str(&format!("**Priority**: {}\n\n", entry.priority.as_str()));
        content.push_str(&format!("**Status**: {}\n\n", entry.status.as_str()));
        content.push_str(&format!("**Area**: {}\n\n", entry.area));
        content.push_str(&format!("**Summary**: {}\n\n", entry.summary));

        if !entry.key_points.is_empty() {
            content.push_str("**Details**:\n");
            for point in &entry.key_points {
                content.push_str(&format!("- {}\n", point));
            }
            content.push('\n');
        }

        if let Some(suggested_action) = &entry.suggested_action {
            content.push_str(&format!("**Suggested Action**: {}\n\n", suggested_action));
        }

        if !entry.metadata.is_empty() {
            content.push_str("**Metadata**:\n");
            for meta in &entry.metadata {
                content.push_str(&format!("- {}\n", meta));
            }
            content.push('\n');
        }

        if let Some(reflection) = &entry.reflection {
            content.push_str(&format!("**反思**: {}\n\n", reflection));
        }

        if !entry.resources.is_empty() {
            content.push_str("**相关资源**:\n");
            for resource in &entry.resources {
                content.push_str(&format!("- {}\n", resource));
            }
            content.push('\n');
        }

        if !entry.tags.is_empty() {
            content.push_str(&format!("**标签**: {}\n\n", entry.tags.join(", ")));
        }

        content.push_str("---\n\n");
        content
    }

    /// 格式化错误日志条目
    fn format_error_entry(&self, entry: &ErrorEntry) -> String {
        let mut content = String::new();
        content.push_str(&format!("## {} - {}\n\n", entry.error_type, entry.timestamp.format("%Y-%m-%d %H:%M:%S")));
        content.push_str(&format!("**消息**: {}\n\n", entry.message));
        content.push_str(&format!("**上下文**: {}\n\n", entry.context));

        if let Some(ref stack_trace) = entry.stack_trace {
            content.push_str(&format!("**堆栈跟踪**:\n```\n{}\n```\n\n", stack_trace));
        }

        if let Some(ref solution) = entry.solution {
            content.push_str(&format!("**解决方案**: {}\n\n", solution));
        }

        if !entry.related_errors.is_empty() {
            content.push_str(&format!("**相关错误**: {}\n\n", entry.related_errors.join(", ")));
        }

        content.push_str("---\n\n");
        content
    }

    /// 格式化特性请求日志条目
    fn format_feature_request_entry(&self, entry: &FeatureRequestEntry) -> String {
        let mut content = String::new();
        content.push_str(&format!("## {}\n\n", entry.feature_name));
        content.push_str(&format!("**时间**: {}\n\n", entry.timestamp.format("%Y-%m-%d %H:%M:%S")));
        content.push_str(&format!("**描述**: {}\n\n", entry.description));
        content.push_str(&format!("**使用场景**: {}\n\n", entry.use_case));
        content.push_str(&format!("**优先级**: {}\n\n", entry.priority.as_str()));
        content.push_str(&format!("**预期效果**: {}\n\n", entry.expected_impact));
        content.push_str(&format!("**状态**: {}\n\n", entry.status.as_str()));

        if !entry.related_features.is_empty() {
            content.push_str(&format!("**相关特性**: {}\n\n", entry.related_features.join(", ")));
        }

        content.push_str("---\n\n");
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_type_filename() {
        assert_eq!(LogType::Learning.filename(), "LEARNINGS.md");
        assert_eq!(LogType::Error.filename(), "ERRORS.md");
        assert_eq!(LogType::FeatureRequest.filename(), "FEATURE_REQUESTS.md");
    }

    #[test]
    fn test_learning_entry_creation() {
        let entry = LearningEntry::new(
            "Rust 异步编程".to_string(),
            "学习了 Rust 的 async/await 语法".to_string(),
        )
        .with_key_point("Future 是惰性的".to_string())
        .with_key_point("async 函数返回 impl Future".to_string())
        .with_tag("Rust".to_string())
        .with_tag("异步".to_string());

        assert_eq!(entry.topic, "Rust 异步编程");
        assert_eq!(entry.key_points.len(), 2);
        assert_eq!(entry.tags.len(), 2);
    }

    #[test]
    fn test_error_entry_creation() {
        let entry = ErrorEntry::new(
            "NullPointerError".to_string(),
            "尝试访问空指针".to_string(),
            "在处理用户请求时".to_string(),
        )
        .with_solution("添加空值检查".to_string());

        assert_eq!(entry.error_type, "NullPointerError");
        assert!(entry.solution.is_some());
    }

    #[test]
    fn test_priority_conversion() {
        assert_eq!(Priority::from_str("high"), Some(Priority::High));
        assert_eq!(Priority::High.as_str(), "High");
    }

    #[test]
    fn test_feature_status_conversion() {
        assert_eq!(FeatureStatus::from_str("pending"), Some(FeatureStatus::Pending));
        assert_eq!(FeatureStatus::InProgress.as_str(), "In Progress");
    }
}
