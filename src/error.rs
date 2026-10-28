//! 错误处理模块
//! 
//! 使用 thiserror 库定义结构化的错误类型，便于错误处理和调试

use thiserror::Error;

/// 守心（Shouxin）错误类型
#[derive(Error, Debug)]
pub enum ShouxinError {
    /// LLM 相关错误
    #[error("LLM错误: {0}")]
    LLMError(String),

    /// Embedding 相关错误
    #[error("Embedding错误: {0}")]
    EmbeddingError(String),

    /// 向量存储错误
    #[error("向量存储错误: {0}")]
    VectorStoreError(String),

    /// 图数据库错误
    #[error("图数据库错误: {0}")]
    GraphError(String),

    /// 存储错误
    #[error("存储错误: {0}")]
    StorageError(String),

    /// 配置错误
    #[error("配置错误: {0}")]
    ConfigError(String),

    /// 通用错误
    #[error("通用错误: {0}")]
    GeneralError(String),

    /// 功能未实现
    #[error("功能未实现: {0}")]
    NotImplemented(String),

    /// 记忆未找到
    #[error("记忆未找到: {0}")]
    MemoryNotFound(String),

    /// 实体未找到
    #[error("实体未找到: {0}")]
    EntityNotFound(String),

    /// 关系未找到
    #[error("关系未找到: {0}")]
    RelationNotFound(String),

    /// 无效参数
    #[error("无效参数: {0}")]
    InvalidArgument(String),

    /// 序列化错误
    #[error("序列化错误: {0}")]
    SerializationError(String),

    /// 反序列化错误
    #[error("反序列化错误: {0}")]
    DeserializationError(String),

    /// IO 错误
    #[error("IO错误: {0}")]
    IOError(String),

    /// 数据库错误
    #[error("数据库错误: {0}")]
    DatabaseError(String),

    /// 超时错误
    #[error("操作超时: {0}")]
    TimeoutError(String),

    /// 并发错误
    #[error("并发错误: {0}")]
    ConcurrencyError(String),

    /// 冲突错误（资源已存在）
    #[error("资源冲突: {0}")]
    ConflictError(String),

    /// 内部错误
    #[error("内部错误: {0}")]
    InternalError(String),
}

impl From<serde_json::Error> for ShouxinError {
    fn from(err: serde_json::Error) -> Self {
        ShouxinError::SerializationError(err.to_string())
    }
}

impl From<std::io::Error> for ShouxinError {
    fn from(err: std::io::Error) -> Self {
        ShouxinError::IOError(err.to_string())
    }
}

impl From<anyhow::Error> for ShouxinError {
    fn from(err: anyhow::Error) -> Self {
        ShouxinError::GeneralError(err.to_string())
    }
}

impl From<sled::Error> for ShouxinError {
    fn from(err: sled::Error) -> Self {
        ShouxinError::StorageError(err.to_string())
    }
}

impl From<sqlx::Error> for ShouxinError {
    fn from(err: sqlx::Error) -> Self {
        ShouxinError::DatabaseError(err.to_string())
    }
}

/// 结果类型别名
pub type Result<T> = std::result::Result<T, ShouxinError>;
