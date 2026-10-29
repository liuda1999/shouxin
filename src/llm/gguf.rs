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
        // 用 ProgramFiles 环境变量拼路径，避免写死盘符/安装位置
        let cuda_dir = std::env::var("ProgramFiles")
            .map(|pf| std::path::Path::new(&pf).join("NVIDIA GPU Computing Toolkit").join("CUDA"))
            .ok();
        if let Some(dir) = cuda_dir {
            if dir.exists() {
                tracing::info!("[GPU] 检测到 CUDA 安装目录（{}），可能可启用 GPU", dir.display());
                return true;
            }
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

        let mut threads = config
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

        // 是否允许 GPU 推理。
        // 缺省为 **false**：本项目是后台服务，显存是稀缺资源，必须显式 opt-in 才用 GPU。
        let use_gpu = config
            .extra_params
            .get("use_gpu")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 未编译 CUDA 后端时，llama.cpp 无法真正卸载层到 GPU。
        // 此时即使配置写了 use_gpu=true，也只能跑在 CPU 上，必须明确告警，
        // 否则用户会误以为已经用上 GPU（而实际上既没加速，也没占显存）。
        if use_gpu && !cfg!(feature = "gguf-cuda") {
            tracing::warn!(
                "[GGUF] 配置要求 GPU 推理（use_gpu=true），但当前二进制未编译 CUDA 后端\
                 （需 cargo build --features gguf-cuda），本次将回退纯 CPU 运行"
            );
        }

        let gpu_enabled = use_gpu && cfg!(feature = "gguf-cuda") && is_gpu_available();
        let n_gpu_layers = if gpu_enabled {
            if configured_gpu_layers > 0 {
                configured_gpu_layers.min(40)
            } else {
                28
            }
        } else {
            0
        };

        // GPU 模式下的 CPU 占用上限
        //
        // 原则：本项目是后台服务，不能把 CPU 核心吃满。
        // 即使推理主要跑在 GPU 上，llama.cpp 仍会开多个线程做数据搬运/采样，
        // 因此这里把线程数硬性压到"核心数 × gpu_max_cpu_percent%"以内。
        let gpu_max_cpu_percent = config
            .extra_params
            .get("gpu_max_cpu_percent")
            .and_then(|v| v.as_u64())
            .unwrap_or(60) as u32;

        // 用**物理核心数**做配额基准。
        // 若按逻辑核（含超线程）算，32 线程 × 60% = 19 会超过 16 个物理核，
        // 反而造成超载 —— 与"不吃满 CPU"的初衷相反。
        let cores = physical_cores();

        // 注意：这里用 `use_gpu`（配置意图）而不是 `gpu_enabled`（硬件探测结果）。
        // 只要配置声明要用 GPU，就按"GPU 模式"限制线程数；
        // 即使本机没装 CUDA（gpu_enabled=false），保守收紧也不会造成问题。
        let effective_threads =
            resolve_threads(threads, cores, use_gpu, gpu_max_cpu_percent);
        if use_gpu && effective_threads != threads {
            tracing::info!(
                "[GGUF] GPU 模式限制 CPU 占用：threads {} -> {}（核心 {}，上限 {}%）",
                threads,
                effective_threads,
                cores,
                gpu_max_cpu_percent
            );
        }
        threads = effective_threads;

        tracing::info!(
            "[GGUF] 初始化: GPU={}, n_gpu_layers={}, offload_kv_cache={}, context={}, threads={}（物理核 {}，GPU 上限 {}%）",
            gpu_enabled,
            n_gpu_layers,
            offload_kv_cache,
            context_size,
            threads,
            cores,
            gpu_max_cpu_percent
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
        // 必须先确保模型已加载：对话模板来自 GGUF 元数据，
        // 句柄为空时会拿不到模板而退回内置的 Llama-3 格式。
        // 曾经的表现：主抽取客户端启动时预加载过模型，一切正常；
        // 而心跳/画像这类"首次调用"的客户端拿到的是错误模板，
        // 模型于是把提示词原样复述，画像 JSON 永远解析失败。
        if let Err(e) = self.ensure_loaded().await {
            tracing::warn!("[GGUF] 构造提示词前加载模型失败: {}", e);
        }

        let model = {
            let guard = self.model_handle.read().await;
            guard.as_ref().map(|h| h.model.clone())
        };

        // 紧急开关：跳过模型的 Jinja 对话模板，直接用内置模板。
        //
        // 为什么需要：llama.cpp 的 `apply_chat_template` 内部会对**整段渲染结果**
        // 再调一次 `llama_tokenize`（用于识别特殊 token），这条路径绕过了
        // `infer()` 里的分段分词，于是又会踩到 MSVC std::regex 的递归上限，
        // 导致 `regex_error(error_stack)` → 进程 abort。
        // 242 条压测里"抽取全过、画像必崩"就是这个差异造成的。
        // 打开该开关后，提示词只由 `infer()` 分词（已分段），可规避。
        let skip_template = std::env::var("SHOUXIN_SKIP_CHAT_TEMPLATE")
            .map(|v| v != "0" && !v.is_empty())
            .unwrap_or(false);

        if let Some(model) = model {
            if skip_template {
                tracing::debug!("[GGUF] SHOUXIN_SKIP_CHAT_TEMPLATE 已开启，跳过 Jinja 模板");
            } else if let Ok(tmpl) = model.chat_template(None) {
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

        // 先做分词器安全化处理，再用处理后的文本。
        //
        // 背景（真实崩溃）：llama.cpp 的 Mistral 预分词正则形如
        //   [^\r\n\p{L}\p{N}]?((?=[\p{L}])([^a-z]))*((?=[\p{L}])([^A-Z]))+...
        // MSVC 的 std::regex 是递归实现，遇到上标/下标/组合符时会在长文本上
        // 触发 regex_error(error_stack)，而 C++ 异常跨 FFI 抛回 Rust 会直接
        // `fatal runtime error: Rust cannot catch foreign exceptions` —— **整个进程 abort**。
        // 200 条压测实测：一条含 ∑/ᵢ/² 的数学公式就让服务崩了，
        // 队列里 52 条待抽取任务全部中断。
        // 这里把这类字符替换成 ASCII 等价形式，既不丢语义，也让分词器回到安全路径。
        let prompt_owned = sanitize_for_tokenizer(prompt);
        let prompt_str = prompt_owned;
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

            let tokens = tokenize_chunked(model, &prompt_str)?;

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
        // 净化必须发生在**模板渲染之前**。
        //
        // `build_chat_prompt` 会调用 llama.cpp 的 `apply_chat_template`，
        // 它内部同样要分词（用于校验特殊 token）。之前只在 `infer()` 里净化，
        // 于是模板渲染这一步仍然拿着原始文本去分词 —— 含 emoji/数学符号/超长串时
        // 依旧会触发 `regex_error(error_stack)` 并 abort 整个进程。
        // 压测里"人设生成阶段反复崩溃"就是这个漏点。
        let system_text = sanitize_for_tokenizer(
            &messages.iter()
                .find(|m| m.role == MessageRole::System)
                .map(|m| m.content.trim().to_string())
                .unwrap_or_default(),
        );
        let user_text = sanitize_for_tokenizer(
            &messages.iter()
                .find(|m| m.role == MessageRole::User)
                .map(|m| m.content.trim().to_string())
                .unwrap_or_default(),
        );

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

/// 分段分词：把长文本切成小段分别 tokenize，再拼接结果
///
/// 为什么需要：llama.cpp 的预分词用的是 `std::regex`（MSVC 实现）。
/// 它有一个与**单次输入长度**相关的匹配预算，超了会抛
/// `regex_error(error_stack): insufficient memory to determine whether the regular expression could match`。
/// 这个 C++ 异常跨 FFI 抛回 Rust 无法捕获，直接 `fatal runtime error: Rust cannot catch foreign exceptions`
/// —— **整个服务进程 abort**。
///
/// 实测：约 8900 字符的抽取提示（其中含 1100 字符纯数字串）100% 崩溃；
/// 同样的提示去掉那串数字就正常。把 `thread_stack_size` 提到 32 MB 也无效，
/// 说明不是 OS 栈，而是 STL 内部的递归上限。
///
/// 按 512 字符分段后，单次 regex 输入恒定在 512 以内，
/// 边界处最多损失一两个 BPE 合并（对提示词无实质影响）。
fn tokenize_chunked(
    model: &LlamaModel,
    text: &str,
) -> Result<Vec<llama_cpp_2::token::LlamaToken>> {
    use llama_cpp_2::token::LlamaToken;

    const CHUNK_CHARS: usize = 512;

    // 短文本直接走原路径，避免任何行为变化
    if text.chars().count() <= CHUNK_CHARS {
        return model
            .str_to_token(text, AddBos::Never)
            .map_err(|e| ShouxinError::LLMError(format!("分词失败: {}", e)));
    }

    let mut all: Vec<LlamaToken> = Vec::new();
    let mut buf = String::new();
    let mut buf_len = 0usize;

    let flush = |buf: &mut String, buf_len: &mut usize, all: &mut Vec<LlamaToken>| -> Result<()> {
        if buf.is_empty() {
            return Ok(());
        }
        let toks = model
            .str_to_token(buf, AddBos::Never)
            .map_err(|e| ShouxinError::LLMError(format!("分词失败: {}", e)))?;
        all.extend(toks);
        buf.clear();
        *buf_len = 0;
        Ok(())
    };

    for ch in text.chars() {
        buf.push(ch);
        buf_len += 1;
        // 优先在换行处切，减少跨段合并损失
        if buf_len >= CHUNK_CHARS {
            flush(&mut buf, &mut buf_len, &mut all)?;
        }
        let _ = ch;
    }
    flush(&mut buf, &mut buf_len, &mut all)?;

    Ok(all)
}

/// 把会触发 llama.cpp 分词正则崩溃的字符替换成 ASCII 等价形式
///
/// 只处理"数学/排版装饰字符"，不动中日韩文字与常规标点，
/// 因此不会影响语义，也不会改变入库的原文（本函数只作用于**提示词**）。
pub(crate) fn sanitize_for_tokenizer(input: &str) -> String {
    /// 允许进入分词器的字符（白名单）
    ///
    /// 为什么用白名单而不是黑名单：触发 llama.cpp 分词正则崩溃的字符集无法穷举
    /// （emoji、杂项符号、组合符、上下标、区域指示符都试过，仍有漏网），
    /// 而 LLM 提示词只需要"正常语言文字"。白名单之外一律丢弃，
    /// 从源头保证分词器只看到它稳定处理过的字符。
    fn keep_char(c: char) -> bool {
        let u = c as u32;
        if u == 0x0A || u == 0x09 {
            return true;
        }
        if (0x20..=0x7E).contains(&u) {
            return true; // ASCII 可打印
        }
        (0x4E00..=0x9FFF).contains(&u)      // CJK 统一表意
            || (0x3400..=0x4DBF).contains(&u)   // CJK 扩展 A
            || (0x3000..=0x303F).contains(&u)   // CJK 标点
            || (0xFF00..=0xFFEF).contains(&u)   // 全角字符
            || (0x3040..=0x30FF).contains(&u)   // 日文假名
            || (0xAC00..=0xD7AF).contains(&u)   // 韩文
            || (0x0370..=0x03FF).contains(&u)   // 希腊字母
            || (0x0400..=0x04FF).contains(&u)   // 西里尔字母
            || ((0x2000..=0x206F).contains(&u) && !(0x200B..=0x200F).contains(&u)) // 通用标点
    }

    let mut out = String::with_capacity(input.len());
    let mut chars_since_newline = 0usize;
    let mut prev: Option<char> = None;
    let mut run_len = 0usize;

    for ch in input.chars() {
        // 1) 数学符号映射成 ASCII（保留语义）
        let mapped: Option<&str> = match ch {
            '⁰' => Some("0"), '¹' => Some("1"), '²' => Some("2"), '³' => Some("3"),
            '⁴' => Some("4"), '⁵' => Some("5"), '⁶' => Some("6"), '⁷' => Some("7"),
            '⁸' => Some("8"), '⁹' => Some("9"), '⁺' => Some("+"), '⁻' => Some("-"),
            'ⁿ' => Some("n"), 'ᵢ' => Some("i"), 'ⱼ' => Some("j"), 'ₖ' => Some("k"),
            'ₙ' => Some("n"), 'ₓ' => Some("x"), '₀' => Some("0"), '₁' => Some("1"),
            '₂' => Some("2"), '₃' => Some("3"), '₄' => Some("4"), '₅' => Some("5"),
            '₆' => Some("6"), '₇' => Some("7"), '₈' => Some("8"), '₉' => Some("9"),
            '∑' => Some("sum"), '∏' => Some("prod"), '∫' => Some("int"),
            '√' => Some("sqrt"), '∞' => Some("inf"), '≈' => Some("~="),
            '≠' => Some("!="), '≤' => Some("<="), '≥' => Some(">="),
            '±' => Some("+/-"), '×' => Some("x"), '÷' => Some("/"),
            '→' => Some("->"), '←' => Some("<-"), '⇒' => Some("=>"),
            '∂' => Some("d"),
            _ => None,
        };

        if let Some(rep) = mapped {
            out.push_str(rep);
            chars_since_newline += rep.chars().count();
            prev = None;
            run_len = 0;
            continue;
        }

        // 2) 白名单之外直接丢
        if !keep_char(ch) {
            continue;
        }

        // 3) 同一字符连续超过 20 次只保留 20 个（折叠 abcabc/!!!! 这类超长同构串）
        if Some(ch) == prev {
            run_len += 1;
        } else {
            run_len = 1;
            prev = Some(ch);
        }
        if run_len > 20 {
            continue;
        }

        // 4) 软换行：每 400 字符插一个换行，打断超长单行
        if chars_since_newline >= 400 {
            out.push('\n');
            chars_since_newline = 0;
        }
        chars_since_newline += 1;
        if ch == '\n' || ch == '\t' {
            chars_since_newline = 0;
        }

        out.push(ch);
    }

    out

}

/// 取物理核心数（拿不到时退回逻辑核数，最少 1）
pub(crate) fn physical_cores() -> usize {
    let phys = num_cpus::get_physical();
    if phys > 0 {
        return phys;
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1)
}

/// 根据运行模式决定实际线程数
///
/// * 纯 CPU：按配置值，但不超过核心数（后台服务原则：不主动开超线程以外的额外并发）
/// * GPU：再叠加"最多占用 N% 核心"的硬性上限
pub(crate) fn resolve_threads(
    configured: usize,
    cores: usize,
    gpu_enabled: bool,
    percent: u32,
) -> usize {
    let cores = cores.max(1);
    if gpu_enabled {
        cap_threads_for_gpu(configured, cores, percent)
    } else {
        configured.max(1).min(cores)
    }
}

/// GPU 模式下按"最多占用 N% 核心"折算线程数
///
/// 取向下取整，保证**不会超过**给定比例；同时至少为 1，避免算出 0 线程。
pub(crate) fn cap_threads_for_gpu(configured: usize, cores: usize, percent: u32) -> usize {
    let pct = percent.clamp(1, 100) as usize;
    let cores = cores.max(1);
    let allowed = ((cores * pct) / 100).max(1);
    configured.max(1).min(allowed)
}

#[cfg(test)]
mod thread_cap_tests {
    use super::{cap_threads_for_gpu, resolve_threads};

    #[test]
    fn cpu_mode_uses_configured_threads_up_to_cores() {
        // 纯 CPU：配置 16、32 核 → 16；配置 64 → 夹到 32
        assert_eq!(resolve_threads(16, 32, false, 60), 16);
        assert_eq!(resolve_threads(64, 32, false, 60), 32);
        assert_eq!(resolve_threads(0, 32, false, 60), 1);
    }

    #[test]
    fn gpu_mode_is_capped_to_percent() {
        // 本机物理 16 核：16 × 60% = 9，配置 32 也要被压到 9
        assert_eq!(resolve_threads(32, 16, true, 60), 9);
        // 配置本来就低时不受影响
        assert_eq!(resolve_threads(4, 16, true, 60), 4);
        // 8 核 × 60% = 4
        assert_eq!(resolve_threads(8, 8, true, 60), 4);
    }

    #[test]
    fn caps_to_60_percent_of_cores() {
        // 16 核 × 60% = 9 核（向下取整）
        assert_eq!(cap_threads_for_gpu(16, 16, 60), 9);
        assert_eq!(cap_threads_for_gpu(32, 16, 60), 9);
        // 配置本来就低于上限时不受影响
        assert_eq!(cap_threads_for_gpu(4, 16, 60), 4);
    }

    #[test]
    fn never_below_one_thread() {
        assert_eq!(cap_threads_for_gpu(1, 1, 60), 1);
        assert_eq!(cap_threads_for_gpu(8, 1, 60), 1);
    }

    #[test]
    fn percent_is_clamped() {
        // 0% / 超过 100% 都会被夹到合法区间
        assert_eq!(cap_threads_for_gpu(16, 8, 0), 1);
        assert_eq!(cap_threads_for_gpu(16, 8, 200), 8);
    }
}

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_for_tokenizer;

    #[test]
    fn maps_math_symbols_to_ascii() {
        let s = sanitize_for_tokenizer("μ = (1/n)∑xᵢ，σ = √(∑(xᵢ-μ)²/n)");
        assert!(!s.contains('∑'));
        assert!(!s.contains('ᵢ'));
        assert!(!s.contains('²'));
        assert!(s.contains("sum"));
        assert!(s.contains("sqrt"));
        // 中文标点必须保留
        assert!(s.contains('，'));
    }

    #[test]
    fn strips_emoji_and_symbols() {
        let s = sanitize_for_tokenizer("今天和 😀 张伟 👨‍💻 一起 review，🚀 下周上线 👀");
        assert!(!s.contains('😀'));
        assert!(!s.contains('🚀'));
        assert!(s.contains("张伟"));
        assert!(s.contains("review"));
    }

    #[test]
    fn keeps_normal_chinese_and_english() {
        let raw = "张伟说明天上午十点开周会，deadline 是 11-04。";
        assert_eq!(sanitize_for_tokenizer(raw), raw);
    }
}
