//! 内置 GGUF 客户端实现
//!
//! 使用 llama-cpp-2 v0.1.146 加载本地 GGUF 模型文件，通过 spawn_blocking 隔离阻塞推理
//! 支持 GPU/CPU 智能切换：有 CUDA 显卡时优先使用 GPU 加速，否则使用 CPU
//! 需要启用 `gguf` feature flag
//!
//! 显存优化策略：
//! - n_gpu_layers: 只卸载模型层到 GPU，不卸载 KV Cache
//! - offload_kv_cache: KV Cache 保留在 CPU，避免显存泄漏
//! - 合理的 context_size: 3B 模型建议 2048

#![cfg(feature = "gguf")]

use async_trait::async_trait;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;

use crate::core::{Fact, FactSource, Memory, MemoryAction, Message, MessageRole};
use crate::error::{Result, ShouxinError};
use crate::traits::llm::{LlmClient, LlmConfig};

static GPU_CHECKED: AtomicBool = AtomicBool::new(false);
static GPU_AVAILABLE: AtomicBool = AtomicBool::new(false);
static GLOBAL_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn check_nvidia_gpu() -> bool {
    if std::env::var("CUDA_PATH").is_ok() {
        tracing::info!("[GPU] 检测到 CUDA_PATH 环境变量，启用 GPU 加速");
        return true;
    }
    
    #[cfg(windows)]
    {
        let cuda_in_program_files = std::path::Path::new(
            "C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA"
        );
        if cuda_in_program_files.exists() {
            tracing::info!("[GPU] 检测到 NVIDIA CUDA 安装目录，启用 GPU 加速");
            return true;
        }
    }
    
    tracing::info!("[GPU] 未检测到 NVIDIA GPU，使用 CPU 推理");
    false
}

fn is_gpu_available() -> bool {
    if !GPU_CHECKED.load(Ordering::SeqCst) {
        let available = check_nvidia_gpu();
        GPU_AVAILABLE.store(available, Ordering::SeqCst);
        GPU_CHECKED.store(true, Ordering::SeqCst);
    }
    GPU_AVAILABLE.load(Ordering::SeqCst)
}

pub struct GgufClient {
    model_path: PathBuf,
    context_size: usize,
    threads: usize,
    batch_size: usize,
    n_gpu_layers: usize,
    offload_kv_cache: bool,
    temperature: f32,
    max_tokens: Option<u32>,
    model_handle: Arc<tokio::sync::RwLock<Option<GgufModelHandle>>>,
    inference_semaphore: Arc<Semaphore>,
    gpu_enabled: bool,
}

struct GgufModelHandle {
    model: Arc<LlamaModel>,
}

unsafe impl Send for GgufModelHandle {}
unsafe impl Sync for GgufModelHandle {}

impl GgufClient {
    pub fn new(config: LlmConfig) -> Self {
        let model_path = config
            .extra_params
            .get("model_path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./models/model.gguf"));

        let context_size = config
            .extra_params
            .get("context_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(2048) as usize;

        let threads = config
            .extra_params
            .get("threads")
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize;

        let batch_size = config
            .extra_params
            .get("batch_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(512) as usize;

        let configured_gpu_layers = config
            .extra_params
            .get("n_gpu_layers")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let offload_kv_cache = config
            .extra_params
            .get("offload_kv_cache")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 是否允许 GPU 推理（use_gpu=false 时纯 CPU，模型全部驻留内存）
        let use_gpu = config
            .extra_params
            .get("use_gpu")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let gpu_enabled = use_gpu && is_gpu_available();
        let n_gpu_layers = if gpu_enabled {
            if configured_gpu_layers > 0 {
                configured_gpu_layers.min(40)
            } else {
                28
            }
        } else {
            0
        };

        tracing::info!(
            "[GGUF] 初始化: GPU={}, n_gpu_layers={}, offload_kv_cache={}, context={}, threads={}",
            gpu_enabled,
            n_gpu_layers,
            offload_kv_cache,
            context_size,
            threads
        );

        Self {
            model_path,
            context_size,
            threads,
            batch_size,
            n_gpu_layers,
            offload_kv_cache,
            temperature: config.temperature.clamp(0.0, 2.0),
            max_tokens: config.max_tokens,
            model_handle: Arc::new(tokio::sync::RwLock::new(None)),
            inference_semaphore: Arc::new(Semaphore::new(1)),
            gpu_enabled,
        }
    }

    pub fn model_path(&self) -> &PathBuf {
        &self.model_path
    }

    /// 按模型自带的对话模板构造提示词
    ///
    /// 旧实现把 `<|begin_of_text|><|start_header_id|>…` 这套 **Llama-3 模板写死**，
    /// 换成 Gemma / Qwen 等模型后模板不匹配，模型会把输入当续写而**复述原文**，
    /// 既浪费 token 又拿不到结构化输出。
    ///
    /// 正确做法是读 GGUF 元数据里的 `tokenizer.chat_template`（`model.chat_template(None)`）：
    /// 每个模型自带自己的模板，代码无需按模型名分支。取不到模板时退回 Llama-3 格式。
    async fn build_chat_prompt(&self, system_text: &str, user_text: &str) -> String {
        let model = {
            let guard = self.model_handle.read().await;
            guard.as_ref().map(|h| h.model.clone())
        };

        if let Some(model) = model {
            if let Ok(tmpl) = model.chat_template(None) {
                let mut msgs: Vec<llama_cpp_2::model::LlamaChatMessage> = Vec::new();
                if !system_text.is_empty() {
                    if let Ok(m) = llama_cpp_2::model::LlamaChatMessage::new(
                        "system".to_string(),
                        system_text.to_string(),
                    ) {
                        msgs.push(m);
                    }
                }
                if let Ok(m) = llama_cpp_2::model::LlamaChatMessage::new(
                    "user".to_string(),
                    user_text.to_string(),
                ) {
                    msgs.push(m);
                }
                if !msgs.is_empty() {
                    match model.apply_chat_template(&tmpl, &msgs, true) {
                        Ok(p) if !p.is_empty() => return p,
                        Ok(_) => {
                            tracing::warn!("[GGUF] 对话模板返回空串，回退到内置模板");
                        }
                        Err(e) => {
                            tracing::warn!("[GGUF] 应用模型对话模板失败({})，回退到内置模板", e);
                        }
                    }
                }
            }
        }

        // 兜底 1：按模型家族内置模板
        //
        // 为什么需要它：`apply_chat_template` 内部按「消息长度 × 2」分配缓冲区，
        // 遇到 Gemma 4 这类模板（含大量 Jinja 宏、tools 定义）会溢出并返回 `ffi error -1`。
        // 这里按 `general.architecture` 走内置模板，保证新模型也能拿到正确格式。
        if let Some(model) = {
            let guard = self.model_handle.read().await;
            guard.as_ref().map(|h| h.model.clone())
        } {
            let arch = model
                .meta_val_str("general.architecture")
                .unwrap_or_default()
                .to_lowercase();

            // Gemma 没有 system 角色，惯例是把 system 内容并入第一条 user 消息
            if arch.contains("gemma") {
                let content = if system_text.is_empty() {
                    user_text.to_string()
                } else {
                    format!("{}\n\n{}", system_text, user_text)
                };
                return format!(
                    "<start_of_turn>user\n{}<end_of_turn>\n<start_of_turn>model\n",
                    content
                );
            }

            if arch.contains("qwen") {
                let mut p = String::new();
                if !system_text.is_empty() {
                    p.push_str(&format!("<|im_start|>system\n{}<|im_end|>\n", system_text));
                }
                p.push_str(&format!(
                    "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                    user_text
                ));
                return p;
            }
        }

        // 兜底 2：Llama-3 风格
        if system_text.is_empty() {
            format!(
                "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                user_text
            )
        } else {
            format!(
                "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n",
                system_text, user_text
            )
        }
    }

    async fn ensure_loaded(&self) -> Result<()> {
        {
            let handle = self.model_handle.read().await;
            if handle.is_some() {
                return Ok(());
            }
        }

        let model_path = self.model_path.clone();
        let context_size = self.context_size;
        let threads = self.threads;
        let batch_size = self.batch_size;
        let n_gpu_layers = self.n_gpu_layers;
        let offload_kv_cache = self.offload_kv_cache;

        tracing::info!("Loading GGUF model: {:?}", model_path);

        let model_handle = tokio::task::spawn_blocking(move || -> Result<GgufModelHandle> {
            if !model_path.exists() {
                return Err(ShouxinError::LLMError(format!(
                    "GGUF 模型文件不存在: {:?}",
                    model_path
                )));
            }

            let file_size = std::fs::metadata(&model_path)
                .map(|m| m.len())
                .unwrap_or(0);
            tracing::info!(
                "Loading GGUF model from {:?} ({} bytes)",
                model_path,
                file_size
            );

            let backend = GLOBAL_BACKEND.get_or_init(|| {
                LlamaBackend::init().unwrap_or_else(|e| {
                    panic!("Failed to initialize LlamaBackend: {}", e);
                })
            });

            let model_params =
                LlamaModelParams::default()
                    .with_n_gpu_layers(n_gpu_layers as u32);

            let model = LlamaModel::load_from_file(backend, &model_path, &model_params)
                .map_err(|e| ShouxinError::LLMError(format!("加载 GGUF 模型失败: {}", e)))?;

            tracing::info!("GGUF model loaded successfully, n_gpu_layers={}", n_gpu_layers);

            Ok(GgufModelHandle {
                model: Arc::new(model),
            })
        })
        .await
        .map_err(|e| ShouxinError::LLMError(format!("spawn_blocking 失败: {}", e)))??;

        let mut handle = self.model_handle.write().await;
        if handle.is_none() {
            *handle = Some(model_handle);
            tracing::info!("GGUF model handle stored: {:?}", self.model_path);
        }
        Ok(())
    }

    async fn infer(&self, prompt: &str) -> Result<String> {
        self.ensure_loaded().await?;

        let prompt_str = prompt.to_string();
        let max_tokens = self.max_tokens.unwrap_or(512);
        let temperature = self.temperature;
        let context_size = self.context_size;
        let batch_size = self.batch_size;
        let threads = self.threads;
        let offload_kv_cache = self.offload_kv_cache;

        tracing::info!(
            "GGUF inference start: {} chars, max_tokens={}, temp={}",
            prompt_str.len(),
            max_tokens,
            temperature
        );

        let model_handle = self.model_handle.clone();
        let start = std::time::Instant::now();

        let _permit = self.inference_semaphore.acquire().await.map_err(|e| {
            ShouxinError::LLMError(format!("推理信号量获取失败: {}", e))
        })?;

        let guard = model_handle.read().await;
        let model_ref = guard
            .as_ref()
            .ok_or_else(|| ShouxinError::LLMError("GGUF 模型未加载".to_string()))?;

        let backend = GLOBAL_BACKEND.get().expect("backend must be initialized");
        let model_arc = model_ref.model.clone();

        drop(guard);

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel_flag.clone();

        let inference_join = tokio::task::spawn_blocking(move || -> Result<String> {
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(context_size as u32))
                .with_n_batch(batch_size as u32)
                .with_n_threads(threads as i32)
                .with_n_threads_batch(threads as i32)
                .with_op_offload(offload_kv_cache);

            let model = unsafe { &*(&*model_arc as *const LlamaModel) };

            let mut context = model
                .new_context(backend, ctx_params)
                .map_err(|e| ShouxinError::LLMError(format!("创建推理上下文失败: {}", e)))?;

            context.clear_kv_cache();

            let tokens = model
                .str_to_token(&prompt_str, AddBos::Never)
                .map_err(|e| ShouxinError::LLMError(format!("分词失败: {}", e)))?;

            let n_ctx = context.n_ctx() as usize;
            let effective_max_tokens = if tokens.len() + max_tokens as usize > n_ctx {
                let adjusted = n_ctx.saturating_sub(tokens.len());
                tracing::warn!(
                    "Prompt is {} tokens (ctx={}), reducing max_tokens from {} to {}",
                    tokens.len(), n_ctx, max_tokens, adjusted
                );
                adjusted as u32
            } else {
                max_tokens
            };

            if tokens.len() >= n_ctx {
                drop(context);
                return Err(ShouxinError::LLMError(format!(
                    "输入过长: {} tokens >= {} ctx",
                    tokens.len(),
                    n_ctx
                )));
            }

            // 分块预填充（chunked prefill）
            //
            // 修复：旧实现把全部 prompt token 一次性塞进一个 n_batch 大小的 batch，
            // 而抽取用的提示词有 8000+ 字符（约 2500+ token），远超 n_batch=512，
            // 于是 llama.cpp 直接报 `Insufficient Space of 512`，
            // 整个 LLM 抽取链路静默降级为正则提取（质量明显下降且不易察觉）。
            //
            // 正确做法是按 n_batch 分块 decode，只有最后一个 token 需要输出 logits
            // （其余 token 不取 logits，顺带省掉一次全量词表计算）。
            let batch_sz = context.n_batch() as usize;
            let mut batch = LlamaBatch::new(batch_sz, 1);

            let last_index = tokens.len().saturating_sub(1);
            let mut pos = 0usize;
            while pos < tokens.len() {
                let chunk_end = (pos + batch_sz).min(tokens.len());
                batch.clear();
                for (offset, &token) in tokens[pos..chunk_end].iter().enumerate() {
                    let abs = pos + offset;
                    batch
                        .add(token, abs as i32, &[0], abs == last_index)
                        .map_err(|e| ShouxinError::LLMError(format!("batch add 失败: {}", e)))?;
                }
                context
                    .decode(&mut batch)
                    .map_err(|e| ShouxinError::LLMError(format!("解码失败: {}", e)))?;
                pos = chunk_end;
            }

            // 采样器：用 llama.cpp 原生采样链（top-k → top-p → temp → dist）。
            // 旧实现每个 token 都把 128k 词表全量拷进 Vec 再排序，单 token 额外开销 ~100ms，
            // 是 CPU 推理只有 5 tok/s 的主因之一。
            let mut sampler = llama_cpp_2::sampling::LlamaSampler::chain_simple([
                llama_cpp_2::sampling::LlamaSampler::top_k(40),
                llama_cpp_2::sampling::LlamaSampler::top_p(0.95, 1),
                llama_cpp_2::sampling::LlamaSampler::temp(if temperature > 0.0 { temperature } else { 0.8 }),
                llama_cpp_2::sampling::LlamaSampler::dist(1234),
            ]);

            let mut raw_bytes: Vec<u8> = Vec::new();
            let mut gen_pos = tokens.len() as i32;
            // 性能埋点：把生成阶段的吞吐打出来（tok/s），便于判断 CPU 推理是否达标
            let __gen_start = std::time::Instant::now();
            let mut __gen_tokens = 0usize;

            for _ in 0..effective_max_tokens {
                if cancel_clone.load(Ordering::Relaxed) {
                    tracing::warn!("GGUF inference cancelled by timeout signal after {} bytes generated", raw_bytes.len());
                    drop(context);
                    return Err(ShouxinError::LLMError(
                        "GGUF推理被取消(超时信号)".to_string()
                    ));
                }

                let token_id = sampler.sample(&context, -1);

                // 停止判定要用「任意结束 token」，不能只比 token_eos()。
                // Gemma 系列的对话结束标记是 <end_of_turn>，它不是 vocab 里唯一的 EOS，
                // 只比 token_eos() 会导致模型一直生成到 max_tokens 才停（复述原文、浪费时间）。
                if model.is_eog_token(token_id) || token_id == model.token_eos() {
                    break;
                }

                match model.token_to_piece_bytes(token_id, 256, false, None) {
                    Ok(bytes) => {
                        raw_bytes.extend_from_slice(&bytes);
                    }
                    Err(e) => {
                        tracing::warn!("GGUF token_to_piece_bytes failed: {:?}", e);
                        continue;
                    }
                }

                if raw_bytes.len() >= 5000 {
                    break;
                }

                sampler.accept(token_id);

                batch.clear();
                batch
                    .add(token_id, gen_pos, &[0], true)
                    .map_err(|e| ShouxinError::LLMError(format!("gen batch add: {}", e)))?;

                if context.decode(&mut batch).is_err() {
                    break;
                }

                __gen_tokens += 1;
                gen_pos += 1;
            }

            let __elapsed = __gen_start.elapsed();
            tracing::info!(
                "[GGUF] 生成 {} tokens / {:.2}s = {:.1} tok/s (context={})",
                __gen_tokens,
                __elapsed.as_secs_f64(),
                __gen_tokens as f64 / __elapsed.as_secs_f64().max(0.001),
                n_ctx
            );

            drop(context);

            let generated = String::from_utf8_lossy(&raw_bytes).trim().to_string();
            Ok(generated)
        });

        let timeout_duration = std::time::Duration::from_secs(180);
        let join_result = tokio::time::timeout(timeout_duration, inference_join).await;

        let generated = match join_result {
            Ok(Ok(Ok(text))) => text,
            Ok(Ok(Err(e))) => {
                cancel_flag.store(true, Ordering::SeqCst);
                return Err(e);
            }
            Ok(Err(_join_error)) => {
                tracing::error!("GGUF inference spawn_blocking join error (worker may have panicked)");
                return Err(ShouxinError::LLMError(
                    "GGUF推理panic: tokio worker线程崩溃".to_string()
                ));
            }
            Err(_elapsed) => {
                cancel_flag.store(true, Ordering::SeqCst);
                tracing::error!("GGUF inference timed out after 180s, cancellation signal sent to worker thread");
                return Err(ShouxinError::LLMError(
                    "GGUF推理超时(180s)，已发送取消信号，worker将清理GPU资源".to_string()
                ));
            }
        };

        let elapsed = start.elapsed();
        let truncated = if generated.len() > 500 {
            generated.chars().take(150).collect::<String>()
        } else {
            generated.clone()
        };
        tracing::info!(
            "GGUF inference completed in {:.2}s ({} chars): {}",
            elapsed.as_secs_f64(),
            generated.len(),
            truncated
        );
        Ok(generated)
    }
}

#[async_trait]
impl LlmClient for GgufClient {
    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let system_text = messages.iter()
            .find(|m| m.role == MessageRole::System)
            .map(|m| m.content.trim().to_string())
            .unwrap_or_default();
        let user_text = messages.iter()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.content.trim().to_string())
            .unwrap_or_default();

        let prompt = self.build_chat_prompt(&system_text, &user_text).await;

        let prompt_preview: String = prompt.chars().take(200).collect();
        tracing::info!("[GGUF] Constructed prompt ({} chars): {}", prompt.len(), prompt_preview);

        self.infer(&prompt).await
    }

    async fn extract_facts(&self, conversation: &str) -> Result<Vec<Fact>> {
        let prompt = format!(
            r#"请从以下对话中提取关于用户的事实陈述，返回 JSON 数组。
格式：[{{"statement": "事实描述", "confidence": 0.9}}]
只提取明确的事实，置信度 >= 0.7。如无事实返回空数组。

**重要语言约束：statement 字段必须使用中文，禁止使用英语、西班牙语或其他语言。**

对话：
{}"#,
            conversation
        );

        let response = self.infer(&prompt).await?;
        Self::parse_fact_json(&response)
    }

    async fn decide_memory_action(
        &self,
        new_fact: &Fact,
        existing_memories: Vec<&Memory>,
    ) -> Result<MemoryAction> {
        let mut prompt = format!("新事实: {}\n现有记忆: ", new_fact.statement);
        if existing_memories.is_empty() {
            prompt.push_str("无\n");
        } else {
            for (i, mem) in existing_memories.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", i + 1, mem.content));
            }
        }
        prompt.push_str("操作(ADD/UPDATE/DELETE/NOOP):");

        let response = self.infer(&prompt).await?;
        let action = response.trim().to_uppercase();

        Ok(match action.as_str() {
            "ADD" => MemoryAction::Add,
            "UPDATE" => MemoryAction::Update,
            "DELETE" => MemoryAction::Delete,
            _ => MemoryAction::Add,
        })
    }

    fn model_name(&self) -> &str {
        "gguf-local"
    }

    async fn preload(&self) -> Result<()> {
        self.ensure_loaded().await
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        self.infer(prompt).await
    }
}

impl GgufClient {
    fn parse_fact_json(response: &str) -> Result<Vec<Fact>> {
        #[derive(Debug, serde::Deserialize)]
        struct RawFact {
            statement: String,
            #[serde(default)]
            confidence: f32,
        }

        let json_str = response
            .strip_prefix("```json")
            .or_else(|| response.strip_prefix("```"))
            .unwrap_or(response);
        let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

        if json_str.is_empty() {
            return Ok(Vec::new());
        }

        let facts: Vec<RawFact> =
            if let (Some(s), Some(e)) = (json_str.find('['), json_str.rfind(']')) {
                serde_json::from_str(&json_str[s..=e]).unwrap_or_default()
            } else {
                serde_json::from_str(json_str).unwrap_or_default()
            };

        Ok(facts
            .into_iter()
            .filter(|f| !f.statement.trim().is_empty() && f.confidence >= 0.7)
            .map(|f| {
                let mut fact = Fact::new(f.statement, f.confidence)
                    .unwrap_or_else(|_| Fact::new("unknown".to_string(), 0.5).unwrap());
                fact.source = FactSource::UserMessage;
                fact
            })
            .collect())
    }
}
