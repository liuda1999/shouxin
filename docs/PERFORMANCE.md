# CPU 推理性能：基准、根因与调优

本文记录守心在纯 CPU 环境下的推理性能实测数据、**两次真实的性能事故的根因**，以及可复现的验证方法。
如果你遇到"本地 llama.cpp 比 LM Studio 慢十几倍"，大概率命中本文的坑之一。

---

## 1. 一句话结论

| 阶段 | 生成速度 | 单次抽取端到端 | 说明 |
|------|---------|---------------|------|
| 初始状态 | ~1 tok/s | 超时（600 s 触发降级） | ggml 走标量通用实现 |
| 修 AVX2 编译标志 | ~3.8 tok/s | 100–600 s | 有效但远远不够 |
| **修 `GGML_CPU_GENERIC`（根因）** | **~24 tok/s** | **≈70 s** | 达到预期 |

硬件：AMD Ryzen 9 9950X（16 核 / 32 线程）、61.6 GB DDR5、模型 `Llama-3.2-3B-Instruct-Q5_K_M`（2.3 GB）、
`llama.cpp` 由 `llama-cpp-sys-2 0.1.146` 静态编译、纯 CPU、无 GPU 参与。

---

## 2. 基准方法

不要用"整条抽取链路耗时"来评判推理性能——里面混着 prompt 构造、JSON 解析、多次人名校验、队列调度。
仓库内置了一个只测推理的基准工具：

```bash
# 编译
SHOUXIN_BIN=bench_gguf bash scripts/build_gguf.sh

# 跑（线程数可任意指定）
./target/debug/bench_gguf.exe \
  --model models/Llama-3.2-3B-Instruct-Q5_K_M.gguf \
  --threads 16 --ctx 4096 --max-tokens 128 --runs 1
```

输出示例：

```
[run 1] threads=16 ctx=4096 batch=512 max_tokens=128 | 6.73s | 238 字符 | 35.4 字符/s
```

**怎么从这行算 tok/s**：跑两次不同 `max_tokens`（如 16 和 128），用差值除掉 token 差：

```
per_token = (T128 - T16) / (128 - 16)
tok_per_s = 1 / per_token
```

这样能自动剔除"模型加载 + 上下文创建 + prompt 处理"的固定开销，得到纯生成速度。
服务端日志里也直接打了每轮的数字：

```
INFO [GGUF] 生成 166 tokens / 7.33s = 22.6 tok/s (context=4096)
```

---

## 3. 事故一：ggml 退化为标量实现（慢 20 倍）

### 3.1 症状

- 16 核跑满，但 3B Q5_K_M 只有 **3.8 tok/s**（同机 LM Studio 有 20 tok/s）；
- 生成 139 token 用了 36 s，prompt 处理又吃掉 65 s；
- 线程数从 4 加到 32，速度几乎不变。

### 3.2 排查路径

1. 确认 `CMakeCache.txt`：`CMAKE_BUILD_TYPE=Release`、`GGML_AVX2=ON`、`GGML_NATIVE=ON` —— 看着都正常；
2. 但 `build.ninja` 里 408 个编译单元的 `FLAGS` **一个 `/arch:` 都没有** → 说明 `GGML_AVX2` 变量并没有变成编译选项；
3. 加 `/arch:AVX2` 进 `CMAKE_C_FLAGS/CXX_FLAGS`，`build.ninja` 里出现了 `/arch:AVX2`，**但速度只从 1 涨到 3.8 tok/s**；
4. 继续看编译命令，发现决定性线索：

```
DEFINES = -DGGML_SCHED_MAX_COPIES=4 -DGGML_USE_CPU_REPACK -DGGML_USE_LLAMAFILE ... -DGGML_CPU_GENERIC
```

`-DGGML_CPU_GENERIC` 会让 ggml 只编译**通用标量实现**，此时 `/arch:AVX2` 只是让编译器用 AVX2 指令去跑标量代码，等于白给。

### 3.3 根因

`ggml/src/ggml-cpu/CMakeLists.txt` 的架构分支按 `CMAKE_SYSTEM_PROCESSOR` 判断。
本机构建环境（Ninja + 显式 `cl.exe` + 无 `vcvars`）下，这个变量是**空值**，于是走到最后的兜底分支：

```cmake
else()
    message(WARNING "Unknown CPU architecture. Falling back to generic implementations.")
    list(APPEND ARCH_FLAGS -DGGML_CPU_GENERIC)
endif()
```

configure 日志里的证据：

```
-- CMAKE_SYSTEM_PROCESSOR:
-- GGML_SYSTEM_ARCH: UNKNOWN
```

### 3.4 修复

在 `project()` 之前（即工具链文件里）显式指定架构 —— 见 `scripts/llama_cpu_toolchain.cmake`：

```cmake
if(NOT CMAKE_SYSTEM_PROCESSOR)
    set(CMAKE_SYSTEM_PROCESSOR "AMD64")
endif()
```

验证（必须为空）：

```bash
B=$(ls -d target/debug/build/llama-cpp-sys-2-*/out/build | head -1)
grep -c "GGML_CPU_GENERIC" "$B/build.ninja"    # 期望 0
grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" | sort -u   # 期望 /arch:AVX2
```

修复后：**3.8 → 24 tok/s**（约 6.3 倍），单次调用固定开销同时从 8.9 s 降到 1.3 s。

### 3.5 为什么"换最新 llama.cpp"不是答案

`llama-cpp-2` 最新版本是 0.1.160（比项目用的 0.1.146 新），但它的 crate 包**缺少 `llama.cpp/app` 子目录**，
CMake 直接报 `add_subdirectory given source "app" which is not an existing directory`，无法编译。
而 0.1.146 已包含 `LLM_ARCH_GEMMA4`，足以加载 Gemma 4 系列模型。

> 结论：性能瓶颈在**构建参数**，不在引擎版本。升级到 0.1.160 既编不过，也解决不了这个问题。
> 若将来新版 crate 修好了打包问题，升级只需改 `Cargo.toml` 一行并把 `Cargo.lock` 里的
> `llama-cpp-sys-2` 一并钉到同版本。

---

## 4. 事故二：提示词塞不进 batch（LLM 抽取静默降级）

### 4.1 症状

- 日志里反复出现 `LLM call failed, will retry: LLM错误: batch add 失败: Insufficient Space of 512`；
- 随后 `[LLM Extract] LLM extraction failed ... falling back to regex extraction`；
- 抽取仍然"成功"（`status: completed`），但结果是**正则提取**的，质量明显差。

### 4.2 根因

旧实现把全部 prompt token 一次性塞进一个 `n_batch` 大小的 batch：

```rust
let batch_sz = context.n_batch() as usize;   // 512
let mut batch = LlamaBatch::new(batch_sz, 1);
for (i, &token) in tokens.iter().enumerate() { batch.add(token, i as i32, &[0], true)?; }
```

而抽取用的提示词有 8000+ 字符（约 2500+ token）。只要 `prompt_tokens > n_batch` 就必然失败。
把 `batch_size` 从 4096 调到 512（为了省内存）后，这个问题立刻暴露出来。

### 4.3 修复

改成**分块预填充（chunked prefill）**，按 `n_batch` 切块 decode，且只有最后一个 token 需要输出 logits：

```rust
let batch_sz = context.n_batch() as usize;
let mut batch = LlamaBatch::new(batch_sz, 1);
let last_index = tokens.len().saturating_sub(1);
let mut pos = 0usize;
while pos < tokens.len() {
    let chunk_end = (pos + batch_sz).min(tokens.len());
    batch.clear();
    for (offset, &token) in tokens[pos..chunk_end].iter().enumerate() {
        let abs = pos + offset;
        batch.add(token, abs as i32, &[0], abs == last_index)?;
    }
    context.decode(&mut batch)?;
    pos = chunk_end;
}
```

副作用是正面的：不再对所有 token 计算 logits，prompt 处理更快、显存/内存占用更低。

### 4.4 教训

**"降级成功"比"报错"更危险**。抽取链路有正则兜底，所以 LLM 失效时接口依然返回 `completed`，
只有翻日志才能发现质量退化。因此：

- 关键路径的降级必须打 `WARN` 并体现在状态里（当前实现会在日志中明确标注 `falling back to regex extraction`）；
- 部署后应抽样核对"抽取结果是否真的来自 LLM"。

---

## 5. 参数调优对照表

| 参数 | 经验值 | 说明 |
|------|--------|------|
| `threads` | = 物理核心数（本机 16） | 超线程通常无收益；32 线程实测与 16 持平 |
| `batch_size` | 2048 | 分块预填充的块大小；过大浪费内存，过小拖慢 prompt 处理 |
| `context_size` | 4096 | 抽取提示 + 对话足够；8K 只在长文档场景需要 |
| `use_gpu` / `n_gpu_layers` | `false` / `0` | 纯 CPU 时显式关闭，避免误卸载到 GPU |
| `max_tokens` | 1024 | 抽取输出通常 < 600 token |
| `[llm].timeout_secs` | 1800 | 纯 CPU 下整轮超时要留足余量 |

> **注意**：`n_gpu_layers = 0` 在旧实现里会被当作"自动探测"，导致仍然把 28 层卸载到 GPU。
> 现已用独立的 `use_gpu` 开关显式控制（见 `[llm.gguf].use_gpu`）。

---

## 6. 复现实验清单

```bash
# 1) 确认指令集真的进了编译命令
B=$(ls -d target/debug/build/llama-cpp-sys-2-*/out/build | head -1)
grep -c "GGML_CPU_GENERIC" "$B/build.ninja"          # 0
grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" | sort -u

# 2) 纯推理吞吐
SHOUXIN_BIN=bench_gguf bash scripts/build_gguf.sh
./target/debug/bench_gguf.exe --model models/<model>.gguf --threads 16 --max-tokens 16
./target/debug/bench_gguf.exe --model models/<model>.gguf --threads 16 --max-tokens 128

# 3) 端到端（服务日志会打 tok/s）
grep "GGUF] 生成" .server.log | tail
```
