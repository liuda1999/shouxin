//! 所有者身份解析
//!
//! 系统里所有 `owner_type` / `owner_id` 必须来自**唯一来源**：
//! - 用户：`default_user`（或请求显式传入的 user_id）
//! - 智能体：`settings.agent.primary_agent_id`（默认 `primary_agent_default`）
//!
//! 历史上代码里散落着 `"primary_agent"` 字面量，与配置默认值
//! `"primary_agent_default"` 不一致，导致 Agent 作用域的数据"各写各的"、
//! 读写对不上。这里用进程级一次性初始化的方式收敛为单一来源，
//! 同时保留 serde 默认值场景（无法访问配置）下的回退常量。

use std::sync::OnceLock;

/// 用户默认 ID（用户作用域，且请求未显式提供 user_id 时使用）
pub const DEFAULT_USER_ID: &str = "default_user";

/// 主智能体 ID 的回退值（与 `AgentSettings::default()` 保持一致）
pub const FALLBACK_PRIMARY_AGENT_ID: &str = "primary_agent_default";

static PRIMARY_AGENT_ID: OnceLock<String> = OnceLock::new();

/// 在服务启动时注入配置中的主智能体 ID
///
/// 允许被调用多次（后续调用忽略），便于测试与嵌入式场景。
pub fn init_primary_agent_id(agent_id: impl Into<String>) {
    let agent_id = agent_id.into();
    if agent_id.trim().is_empty() {
        return;
    }
    let _ = PRIMARY_AGENT_ID.set(agent_id);
}

/// 获取主智能体 ID
///
/// 未初始化时（例如单元测试、工具函数）回退到 [`FALLBACK_PRIMARY_AGENT_ID`]。
pub fn primary_agent_id() -> &'static str {
    PRIMARY_AGENT_ID
        .get()
        .map(String::as_str)
        .unwrap_or(FALLBACK_PRIMARY_AGENT_ID)
}

/// 获取默认用户 ID
pub fn default_user_id() -> &'static str {
    DEFAULT_USER_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primary_agent_id_falls_back() {
        // 该测试可能与其他用例共享进程，因此只断言"非空且符合两种合法值之一"
        let id = primary_agent_id();
        assert!(!id.is_empty());
        assert!(id == FALLBACK_PRIMARY_AGENT_ID || id.starts_with("primary_agent"));
    }
}
