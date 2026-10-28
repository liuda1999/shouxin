//! 向量存储配置

use serde::Deserialize;

/// 向量存储配置
#[derive(Debug, Clone, Deserialize)]
pub struct VectorStoreConfig {
    /// 存储路径
    ///
    /// 向量索引文件的存储位置
    pub path: String,

    /// 嵌入维度
    ///
    /// BAAI/bge-base-en-v1.5 模型的维度为 768
    pub dimension: usize,

    /// 是否启用持久化
    ///
    /// 启用后，向量数据会存储到磁盘，重启后仍然保留
    pub persistence: bool,

    /// 搜索限制
    ///
    /// 每次搜索返回的最大结果数
    pub search_limit: usize,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            path: "./data/vectors".to_string(),
            dimension: 768,
            persistence: true,
            search_limit: 10,
        }
    }
}
