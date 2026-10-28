//! GGUF 推理基准工具
//!
//! 用于量化本机 CPU 推理的真实吞吐（tok/s），并支持线程数扫描。
//! 抽取链路每次调用都会打印 `[GGUF] 生成 N tokens / Xs = Y tok/s`，
//! 但那个数字混着 prompt 处理和各种业务逻辑；本工具只测"纯推理"，便于横向对比
//! （与 LM Studio / llama-bench 的口径接近）。
//!
//! 用法：
//! ```bash
//! cargo run --release --features gguf --bin bench_gguf -- \
//!     --model models/gemma-4-E4B-it-Q4_K_M.gguf --threads 16 --runs 2
//!
//! # 线程扫描
//! for t in 1 4 8 16 32; do cargo run --release --features gguf --bin bench_gguf -- --threads $t; done
//! ```

#[cfg(feature = "gguf")]
fn main() {
    use std::collections::HashMap;
    use std::time::Instant;

    use shouxin::llm::GgufClient;
    use shouxin::traits::{LlmClient, LlmConfig};

    let mut model = "models/gemma-4-E4B-it-Q4_K_M.gguf".to_string();
    let mut threads = 16usize;
    let mut ctx = 4096usize;
    let mut batch = 512usize;
    let mut max_tokens = 128u32;
    let mut runs = 1usize;

    let argv: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--model" => {
                model = argv[i + 1].clone();
                i += 2;
            }
            "--threads" => {
                threads = argv[i + 1].parse().unwrap_or(16);
                i += 2;
            }
            "--ctx" => {
                ctx = argv[i + 1].parse().unwrap_or(4096);
                i += 2;
            }
            "--batch" => {
                batch = argv[i + 1].parse().unwrap_or(512);
                i += 2;
            }
            "--max-tokens" => {
                max_tokens = argv[i + 1].parse().unwrap_or(128);
                i += 2;
            }
            "--runs" => {
                runs = argv[i + 1].parse().unwrap_or(1);
                i += 2;
            }
            other => {
                eprintln!("未知参数: {}", other);
                std::process::exit(2);
            }
        }
    }

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio runtime 失败");

    rt.block_on(async move {
        let mut extra = HashMap::new();
        extra.insert("model_path".to_string(), serde_json::json!(model));
        extra.insert("context_size".to_string(), serde_json::json!(ctx));
        extra.insert("threads".to_string(), serde_json::json!(threads));
        extra.insert("batch_size".to_string(), serde_json::json!(batch));
        extra.insert("n_gpu_layers".to_string(), serde_json::json!(0));
        extra.insert("use_gpu".to_string(), serde_json::json!(false));
        extra.insert("offload_kv_cache".to_string(), serde_json::json!(false));

        let config = LlmConfig {
            model: "bench".to_string(),
            endpoint: None,
            temperature: 0.0,
            max_tokens: Some(max_tokens),
            system_prompt: None,
            extra_params: extra,
        };

        let client = GgufClient::new(config);
        if let Err(e) = client.preload().await {
            eprintln!("模型加载失败: {}", e);
            std::process::exit(1);
        }

        // 固定的中文抽取式提示，尽量贴近真实业务负载
        let prompt = "从下面的对话中提取人物、事件、待办。用 JSON 输出，字段为 people/events/todos。\n\
                      对话：昨天下午我和老王在星巴克讨论了下季度的预算，他负责后端服务，\
                      我们需要在下周五之前交付接口文档，另外我答应李姐整理客户反馈。\n\
                      输出：";

        for run in 1..=runs {
            let t0 = Instant::now();
            match client.complete(prompt).await {
                Ok(text) => {
                    let elapsed = t0.elapsed().as_secs_f64();
                    let chars = text.chars().count();
                    println!(
                        "[run {}] threads={} ctx={} batch={} max_tokens={} | {:.2}s | {} 字符 | {:.1} 字符/s",
                        run,
                        threads,
                        ctx,
                        batch,
                        max_tokens,
                        elapsed,
                        chars,
                        chars as f64 / elapsed
                    );
                }
                Err(e) => {
                    eprintln!("[run {}] 推理失败: {}", run, e);
                    std::process::exit(1);
                }
            }
        }
    });
}

#[cfg(not(feature = "gguf"))]
fn main() {
    eprintln!("请加 --features gguf 编译本工具");
    std::process::exit(2);
}
