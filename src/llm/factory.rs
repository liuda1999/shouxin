//! LLM 客户端工厂
//!
//! 根据配置创建对应的 LLM 客户端，支持 Ollama / GGUF / API 三源

use crate::config::settings::{HeartbeatLlmSettings, LlmSettings};
use crate::error::{Result, ShouxinError};
use crate::llm::api::ApiClient;
#[cfg(feature = "gguf")]
use crate::llm::gguf::GgufClient;
use crate::llm::manager::{LlmClientManager, LlmSourceType};
use crate::llm::ollama::OllamaLlmClient;
use crate::traits::llm::LlmConfig;
use std::sync::Arc;

/// 根据配置构建单个客户端
fn build_client(
    source: &str,
    settings: &LlmSettings,
) -> Result<Box<dyn crate::traits::llm::LlmClient>> {
    match source {
        "ollama" => {
            let ollama = settings.ollama.as_ref().ok_or_else(|| {
                ShouxinError::LLMError("Ollama 后端配置缺失".to_string())
            })?;

            let config = LlmConfig {
                model: ollama.model.clone(),
                endpoint: Some(ollama.endpoint.clone()),
                temperature: settings.temperature,
                max_tokens: Some(settings.max_tokens),
                system_prompt: None,
                extra_params: {
                    let mut params = std::collections::HashMap::new();
                    params.insert(
                        "timeout_secs".to_string(),
                        serde_json::Value::Number(ollama.timeout_secs.into()),
                    );
                    params.insert(
                        "max_retries".to_string(),
                        serde_json::Value::Number(ollama.max_retries.into()),
                    );
                    params
                },
            };

            let client = OllamaLlmClient::with_config(config);
            Ok(Box::new(client))
        }
        "gguf" => {
            #[cfg(not(feature = "gguf"))]
            {
                Err(ShouxinError::LLMError(
                    "GGUF 后端未编译。请使用 cargo build --features gguf 启用".to_string(),
                ))
            }
            #[cfg(feature = "gguf")]
            {
                let gguf = settings.gguf.as_ref().ok_or_else(|| {
                    ShouxinError::LLMError("GGUF 后端配置缺失".to_string())
                })?;

            let config = LlmConfig {
                model: "gguf-local".to_string(),
                endpoint: None,
                temperature: settings.temperature,
                max_tokens: Some(settings.max_tokens),
                system_prompt: None,
                extra_params: {
                    let mut params = std::collections::HashMap::new();
                    params.insert(
                        "model_path".to_string(),
                        serde_json::Value::String(gguf.model_path.clone()),
                    );
                    params.insert(
                        "context_size".to_string(),
                        serde_json::Value::Number(gguf.context_size.into()),
                    );
                    params.insert(
                        "threads".to_string(),
                        serde_json::Value::Number(gguf.threads.into()),
                    );
                    params.insert(
                        "batch_size".to_string(),
                        serde_json::Value::Number(gguf.batch_size.into()),
                    );
                    params.insert(
                        "n_gpu_layers".to_string(),
                        serde_json::Value::Number(gguf.n_gpu_layers.into()),
                    );
                    params.insert(
                        "use_gpu".to_string(),
                        serde_json::Value::Bool(gguf.use_gpu),
                    );
                    // GPU 模式下的 CPU 核心占用上限（默认 60%）
                    params.insert(
                        "gpu_max_cpu_percent".to_string(),
                        serde_json::Value::Number(gguf.gpu_max_cpu_percent.into()),
                    );
                    // KV Cache 是否随模型一起卸载到 GPU（默认 false：留在内存）
                    params.insert(
                        "offload_kv_cache".to_string(),
                        serde_json::Value::Bool(gguf.offload_kv_cache),
                    );
                    params
                },
            };

            let client = GgufClient::new(config);
            Ok(Box::new(client))
            }
        }
        "api" => {
            let api = settings.api.as_ref().ok_or_else(|| {
                ShouxinError::LLMError("API 后端配置缺失".to_string())
            })?;

            let config = LlmConfig {
                model: api.model.clone(),
                endpoint: Some(api.endpoint.clone()),
                temperature: settings.temperature,
                max_tokens: Some(settings.max_tokens),
                system_prompt: None,
                extra_params: {
                    let mut params = std::collections::HashMap::new();
                    params.insert(
                        "api_key".to_string(),
                        serde_json::Value::String(api.api_key.clone()),
                    );
                    params.insert(
                        "timeout_secs".to_string(),
                        serde_json::Value::Number(api.timeout_secs.into()),
                    );
                    params.insert(
                        "max_retries".to_string(),
                        serde_json::Value::Number(api.max_retries.into()),
                    );
                    params
                },
            };

            let mut client = ApiClient::new(config);
            if !api.headers.is_empty() {
                client = client.with_extra_headers(api.headers.clone());
            }
            Ok(Box::new(client))
        }
        unknown => Err(ShouxinError::LLMError(format!(
            "未知的 LLM 来源: {}。可选: ollama, gguf, api",
            unknown
        ))),
    }
}

fn source_to_type(source: &str) -> LlmSourceType {
    match source {
        "ollama" => LlmSourceType::Ollama,
        "gguf" => LlmSourceType::Gguf,
        "api" => LlmSourceType::Api,
        _ => LlmSourceType::Ollama,
    }
}

/// 构建 LlmClientManager
///
/// 根据 LlmSettings 创建管理器，包含主客户端和可选备客户端
pub async fn build_llm_manager(settings: &LlmSettings) -> Result<LlmClientManager> {
    tracing::info!(
        "Building LLM client manager: primary={}, secondary={:?}",
        settings.primary,
        settings.secondary
    );

    let primary = build_client(&settings.primary, settings)?;
    let primary_type = source_to_type(&settings.primary);

    // 兜底归一：即便调用方绕过配置层直接构造 settings，"none"/空串也不应去建客户端
    let secondary_source = settings
        .secondary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none"));

    let (secondary, secondary_type) = if let Some(sec_source) = secondary_source {
        tracing::info!("Building secondary LLM client: {}", sec_source);
        match build_client(sec_source, settings) {
            Ok(client) => (Some(client), Some(source_to_type(sec_source))),
            Err(e) => {
                tracing::warn!("备模型客户端创建失败: {}，将以无备模式运行", e);
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    let auto_switch = secondary.is_some();
    let max_consecutive_failures = 3;
    let health_interval_secs = 30;

    let manager = LlmClientManager::new(
        primary,
        primary_type,
        secondary,
        secondary_type,
        auto_switch,
        max_consecutive_failures,
        health_interval_secs,
    );

    Ok(manager)
}

/// 为心跳模块构建独立的 Ollama 客户端（兼容旧接口）
pub fn build_ollama_client(settings: &LlmSettings) -> Result<Arc<OllamaLlmClient>> {
    let ollama = settings.ollama.as_ref().ok_or_else(|| {
        ShouxinError::LLMError("心跳模块：Ollama 后端配置缺失".to_string())
    })?;

    let config = LlmConfig {
        model: ollama.model.clone(),
        endpoint: Some(ollama.endpoint.clone()),
        temperature: settings.temperature,
        max_tokens: Some(settings.max_tokens),
        system_prompt: None,
        extra_params: {
            let mut params = std::collections::HashMap::new();
            params.insert(
                "timeout_secs".to_string(),
                serde_json::Value::Number(ollama.timeout_secs.into()),
            );
            params
        },
    };

    let client = OllamaLlmClient::with_config(config);
    Ok(Arc::new(client))
}

/// 为心跳模块构建独立的 LLM 客户端
///
/// 从 [llm.heartbeat] 配置创建客户端，使用心跳专用的模型、temperature、max_tokens。
/// 支持 ollama / gguf / api 三种后端。
pub fn build_heartbeat_client(
    hb_settings: &HeartbeatLlmSettings,
) -> Result<Box<dyn crate::traits::llm::LlmClient>> {
    match hb_settings.backend.as_str() {
        "ollama" => {
            let config = LlmConfig {
                model: hb_settings.model.clone(),
                endpoint: Some(hb_settings.endpoint.clone()),
                temperature: hb_settings.temperature,
                max_tokens: Some(hb_settings.max_tokens),
                system_prompt: None,
                extra_params: {
                    let mut params = std::collections::HashMap::new();
                    params.insert(
                        "timeout_secs".to_string(),
                        serde_json::Value::Number(300.into()),
                    );
                    params.insert(
                        "max_retries".to_string(),
                        serde_json::Value::Number(2.into()),
                    );
                    params
                },
            };
            let client = OllamaLlmClient::with_config(config);
            Ok(Box::new(client))
        }
        "gguf" => {
            #[cfg(not(feature = "gguf"))]
            {
                Err(ShouxinError::LLMError(
                    "心跳模块：GGUF 后端未编译。请使用 cargo build --features gguf 启用".to_string(),
                ))
            }
            #[cfg(feature = "gguf")]
            {
                let config = LlmConfig {
                    model: "gguf-local".to_string(),
                    endpoint: None,
                    temperature: hb_settings.temperature,
                    max_tokens: Some(hb_settings.max_tokens),
                    system_prompt: None,
                    extra_params: {
                        let mut params = std::collections::HashMap::new();
                        params.insert(
                            "model_path".to_string(),
                            serde_json::Value::String(hb_settings.model.clone()),
                        );
                        // CPU 推理：批大小与上下文都要收敛，否则一次调用会慢到分钟级
                        params.insert(
                            "context_size".to_string(),
                            serde_json::Value::Number(4096.into()),
                        );
                        params.insert(
                            "threads".to_string(),
                            serde_json::Value::Number(16.into()),
                        );
                        params.insert(
                            "batch_size".to_string(),
                            serde_json::Value::Number(512.into()),
                        );
                        params.insert(
                            "n_gpu_layers".to_string(),
                            serde_json::Value::Number(0.into()),
                        );
                        // 心跳专用 LLM 同样遵循"推理不使用 GPU"的要求：
                        // 模型加载到内存，纯 CPU 推理。
                        params.insert(
                            "use_gpu".to_string(),
                            serde_json::Value::Bool(false),
                        );
                        // 心跳客户端强制纯 CPU（不占显存），GPU 上限参数用默认 60 即可
                        params.insert(
                            "gpu_max_cpu_percent".to_string(),
                            serde_json::Value::Number(60.into()),
                        );
                        params
                    },
                };
                let client = GgufClient::new(config);
                Ok(Box::new(client))
            }
        }
        "api" => {
            let config = LlmConfig {
                model: hb_settings.model.clone(),
                endpoint: Some(hb_settings.endpoint.clone()),
                temperature: hb_settings.temperature,
                max_tokens: Some(hb_settings.max_tokens),
                system_prompt: None,
                extra_params: {
                    let mut params = std::collections::HashMap::new();
                    params.insert(
                        "api_key".to_string(),
                        serde_json::Value::String(String::new()),
                    );
                    params.insert(
                        "timeout_secs".to_string(),
                        serde_json::Value::Number(60.into()),
                    );
                    params.insert(
                        "max_retries".to_string(),
                        serde_json::Value::Number(2.into()),
                    );
                    params
                },
            };
            let client = ApiClient::new(config);
            Ok(Box::new(client))
        }
        unknown => Err(ShouxinError::LLMError(format!(
            "心跳模块：未知的 LLM 后端: {}。可选: ollama, gguf, api",
            unknown
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_to_type() {
        assert!(matches!(source_to_type("ollama"), LlmSourceType::Ollama));
        assert!(matches!(source_to_type("gguf"), LlmSourceType::Gguf));
        assert!(matches!(source_to_type("api"), LlmSourceType::Api));
    }
}