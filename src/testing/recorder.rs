//! 测试结果记录器
//!
//! 用于记录测试过程中的所有交互

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// 测试交互记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionRecord {
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 模型名称
    pub model: String,
    /// 测试类别
    pub test_category: String,
    /// 测试名称
    pub test_name: String,
    /// 输入内容
    pub input: Vec<InputMessage>,
    /// 输出内容
    pub output: String,
    /// 执行时间（毫秒）
    pub duration_ms: Option<u64>,
    /// 是否成功
    pub success: bool,
    /// 错误信息
    pub error: Option<String>,
}

/// 输入消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    pub role: String,
    pub content: String,
}

/// 测试记录器
pub struct TestRecorder {
    /// 输出文件路径
    output_file: PathBuf,
    /// 模型名称
    model_name: String,
}

impl TestRecorder {
    /// 创建新的测试记录器
    pub fn new(output_dir: &str, model_name: &str) -> Self {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!(
            "{}_interactions_{}.jsonl",
            model_name.replace(':', "_"),
            timestamp
        );
        let output_file = PathBuf::from(output_dir).join(filename);

        // 确保目录存在
        if let Some(parent) = output_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        Self {
            output_file,
            model_name: model_name.to_string(),
        }
    }

    /// 记录交互
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        test_category: &str,
        test_name: &str,
        input: Vec<InputMessage>,
        output: &str,
        duration_ms: Option<u64>,
        success: bool,
        error: Option<String>,
    ) -> std::io::Result<()> {
        let record = InteractionRecord {
            timestamp: Utc::now(),
            model: self.model_name.clone(),
            test_category: test_category.to_string(),
            test_name: test_name.to_string(),
            input,
            output: output.to_string(),
            duration_ms,
            success,
            error,
        };

        let json = serde_json::to_string(&record)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.output_file)?;

        writeln!(file, "{}", json)?;

        Ok(())
    }

    /// 快速记录成功交互
    pub fn record_success(
        &self,
        test_category: &str,
        test_name: &str,
        input: Vec<InputMessage>,
        output: &str,
        duration_ms: u64,
    ) -> std::io::Result<()> {
        self.record(
            test_category,
            test_name,
            input,
            output,
            Some(duration_ms),
            true,
            None,
        )
    }

    /// 快速记录失败交互
    pub fn record_failure(
        &self,
        test_category: &str,
        test_name: &str,
        input: Vec<InputMessage>,
        error: &str,
    ) -> std::io::Result<()> {
        self.record(
            test_category,
            test_name,
            input,
            "",
            None,
            false,
            Some(error.to_string()),
        )
    }

    /// 获取输出文件路径
    pub fn output_path(&self) -> &PathBuf {
        &self.output_file
    }
}

/// 创建默认测试记录器
pub fn create_default_recorder() -> TestRecorder {
    TestRecorder::new(
        "test_results/interactions",
        super::config::get_test_model_name(),
    )
}
