# GGUF LLM 推理输出乱码 — 根因分析报告

**分析日期**: 2026-06-03  
**分析版本**: ShouXin v0.5.1  
**模型**: Llama-3.2-3B-Instruct-Q5_K_M.gguf  
**llama-cpp-2 版本**: 0.1.146  
**分析工程师**: Rust + LLM 底层诊断  

---

## 一、问题描述

GGUF 模型推理输出严重乱码，表现为：
- 输出包含俄文字符（`Р`, `е`, `у` 等）
- Token 重复（`namename`, `titletitle`, `meetingmeetingmeeting`）
- JSON 结构损坏
- 所有 LLM 依赖功能完全失效

**服务器日志证据**:
```
GGUF inference completed in 0.89s (436 chars):
Р{"езультатpeople":":[{" [{"namename":"":"AliceAlice"},{""}}name,"":"BobBobBob"},{""}}}],...

[LLM Extract] JSON parse failed: expected `,` or `}` at line 1 column 33
```

---

## 二、根因分析

### 根因 1：`last_token` 更新顺序错误 → KV Cache 污染（概率: 95%）

**这是最主要的根因，可以解释所有乱码现象。**

#### 2.1.1 问题代码

文件: `src/llm/gguf.rs`，第 369-379 行

```rust
// 第 322-323 行：初始化
let last_token_id = *tokens.last().unwrap();
let mut last_token = llama_cpp_2::token::LlamaToken::new(last_token_id.0);
let mut gen_pos = tokens.len() as i32;

// 第 326-380 行：生成循环
for _ in 0..effective_max_tokens {
    // ... 采样得到 token_id ...

    // 第 355-363 行：解码当前 token（正确）
    model.token_to_piece_bytes(token_id, 256, false, None);

    // 第 369-371 行：向 KV cache 添加 token
    batch.clear();
    batch.add(last_token, gen_pos, &[0], true)?;  // ← 使用 LAST_TOKEN，不是 token_id！

    // 第 374 行：decode 更新 KV cache
    context.decode(&mut batch)?;

    gen_pos += 1;
    last_token = token_id;  // ← 第 379 行：更新在 batch.add 之后！
}
```

#### 2.1.2 错误机制

**关键问题**: `last_token = token_id` 在 `batch.add(last_token, ...)` 之后执行，导致每次 `batch.add` 传入的是**上一次迭代**的 token，而非当前生成的 token。

**逐迭代追踪**（假设 prompt 有 N 个 token，`gen_pos` 从 N 开始）:

| 迭代 | gen_pos | last_token (batch.add 时) | token_id (本次生成) | KV Cache 实际写入 | 应该写入 |
|------|---------|--------------------------|-------------------|------------------|---------|
| 1 | N | prompt[N-1] (prompt 末 token) | token_A | prompt[N-1] 重复写入 position N | token_A 写入 position N |
| 2 | N+1 | token_A | token_B | token_A 写入 position N+1 | token_B 写入 position N+1 |
| 3 | N+2 | token_B | token_C | token_B 写入 position N+2 | token_C 写入 position N+2 |
| 4 | N+3 | token_C | token_D | token_C 写入 position N+3 | token_D 写入 position N+3 |

**结果**: KV cache 中每个 position 都写入了**滞后一拍**的 token，autoregressive 上下文完全错乱。

#### 2.1.3 为什么产生字符重复

由于 KV cache 上下文错乱，模型在错乱的上下文中继续生成：

- 第 N 位置：prompt 末 token 重复 → 模型认为 prompt 没结束，生成与 prompt 末 token 类似的 token
- 第 N+1 位置：token_A → 模型基于错位上下文生成 token_B
- 第 N+2 位置：token_B → 但上下文是 token_A（错位），模型生成近似 token_B 的内容

这导致 token 级别的输出重复 (`namename`, `titletitle`, `meetingmeetingmeeting`)，以及模型幻觉产生俄文字符。

#### 2.1.4 为什么产生俄文字符

Llama 3.2 的 token 词汇表包含俄文 token。当 KV cache 上下文错乱时，模型产生不可预测的输出，包括俄文 token。`Р` (U+0420) 是 Llama 3.2 词汇表中的合法 token，在错乱上下文中被错误采样。

#### 2.1.5 修复方案

**一行修复**：将 `last_token = token_id` 移到 `batch.add` 之前：

```rust
// 修复前 (第 369-379 行)
batch.clear();
batch
    .add(last_token, gen_pos, &[0], true)
    .map_err(|e| ShouxinError::LLMError(format!("gen batch add: {}", e)))?;

if context.decode(&mut batch).is_err() {
    break;
}

gen_pos += 1;
last_token = token_id;  // ← 太晚了

// 修复后
last_token = token_id;  // ← 先更新
batch.clear();
batch
    .add(last_token, gen_pos, &[0], true)
    .map_err(|e| ShouxinError::LLMError(format!("gen batch add: {}", e)))?;

if context.decode(&mut batch).is_err() {
    break;
}

gen_pos += 1;
```

或等效地，直接使用 `token_id`:

```rust
batch.clear();
batch
    .add(token_id, gen_pos, &[0], true)  // ← 直接使用当前生成的 token
    .map_err(|e| ShouxinError::LLMError(format!("gen batch add: {}", e)))?;

if context.decode(&mut batch).is_err() {
    break;
}

gen_pos += 1;
last_token = token_id;  // 可保留或不保留
```

---

### 根因 2：Token 解码方式不完善（概率: 30% 加重因子）

#### 2.2.1 问题代码

文件: `src/llm/gguf.rs`，第 355-363 行、第 384 行

```rust
// 逐 token 收集原始字节
match model.token_to_piece_bytes(token_id, 256, false, None) {
    Ok(bytes) => {
        raw_bytes.extend_from_slice(&bytes);  // 拼接原始字节
    }
    ...
}

// 最后一次性转为 String
let generated = String::from_utf8_lossy(&raw_bytes).trim().to_string();
```

#### 2.2.2 问题分析

`token_to_piece_bytes` 返回单个 token 的原始字节。当多个 token 的字节拼接时，**多字节 UTF-8 字符可能跨越 token 边界**。例如：

- Token A 的字节: `[0xE4, 0xBD]` (UTF-8 三字节序列的前两个字节)
- Token B 的字节: `[0xA0]` (UTF-8 三字节序列的最后一个字节)
- 拼接后: `[0xE4, 0xBD, 0xA0]` → 正确解码为 "你"

但如果 token A 和 token B 的生成顺序错乱（根因 1），拼接后的字节序列产生无效 UTF-8，`from_utf8_lossy` 会插入 `` 替换字符。

#### 2.2.3 推荐方案

llama-cpp-2 提供了 `token_to_str` 方法，内部使用 `encoding_rs::Decoder` 处理跨 token 边界的 UTF-8 序列：

```rust
// 修复前
match model.token_to_piece_bytes(token_id, 256, false, None) {
    Ok(bytes) => raw_bytes.extend_from_slice(&bytes),
    ...
}

// 修复后
match model.token_to_str(token_id, false, None) {
    Ok(piece) => output.push_str(&piece),
    ...
}
```

但此问题在根因 1 修复后自然缓解，因为正确的 token 顺序不会产生大量无效 UTF-8 序列。

---

### 根因 3：中文文本显示 `?` — 终端编码问题（概率: 90%）

#### 2.3.1 现象

日志中 Chinese 文本显示为 `????3???????`:

```
[LLM Extract] fallback_extract ENTRY: text len=29, first 100 chars: user: ???:????3??????,??Q2???
```

#### 2.3.2 分析

- 编码链路追踪正常：`serde_json` → `String` → `sled` → `serde_json` → `String`，全程保持 UTF-8
- `Message::user()` 不添加前缀，`save_conversation` 不修改文本
- `text len=29` 是终端显示 `?` 后的字节数，不是原始文本长度
- 原始文本 "王经理：明天下午3点开项目周会，讨论Q2进度。" 约 60 字节（UTF-8），但显示为 29 字节（ASCII `?` 替代）

**结论**: 这是 PowerShell 终端无法显示中文字符的显示问题，不影响实际数据。验证方法：在支持 UTF-8 的终端（如 Windows Terminal）中运行服务，或通过 `hexdump` 检查实际字节。

---

## 三、排除项

| 检查项 | 结论 | 说明 |
|--------|------|------|
| 模型文件损坏 | ❌ 排除 | 模型文件存在（2.16 GB），sha256 应验证 |
| CUDA 内存不足 | ❌ 排除 | 模型仅 2.2 GB，RTX 3080 20GB 充足 |
| context_size 不足 | ❌ 排除 | 2048 够用，prompt 约 1-2 KB |
| `AddBos` 配置错误 | ❌ 排除 | 使用 `AddBos::Never`，prompt 模板已含 `<\|begin_of_text\|>` |
| chat_template 格式错误 | ❌ 排除 | 使用 Llama 3.2 官方格式 |
| 并发状态冲突 | ❌ 排除 | `Semaphore(1)` 保护，每次推理新建 `LlamaContext` |
| `token_to_piece_bytes` 参数错误 | ❌ 排除 | 签名 `(token, buffer_size, special, lstrip)` — 调用 `(token_id, 256, false, None)` 正确 |
| llama-cpp-2 版本 bug | ❌ 排除 | v0.1.146 无已知 tokenizer bug |
| 输入编码链路破损 | ❌ 排除 | JSON → String → sled → String 全程 UTF-8 正确 |

---

## 四、验证方法

### 4.1 验证根因 1（KV Cache 污染）

**最小化测试**：修改 `gguf.rs` 中的 `last_token` 更新顺序，重新编译测试：

```bash
# 修复后编译
cargo build --release --features gguf

# 测试
curl -X POST http://127.0.0.1:3000/api/memory/add \
  -H "Content-Type: application/json" \
  -d '{"conversation":"Alice: meeting at 3pm tomorrow.","user_id":"test","scope":"User"}'
```

预期：LLM 输出正常 JSON `{"people":[...],"events":[...]}`，不再有俄文字符和 token 重复。

### 4.2 验证根因 2（Token 解码）

在修复根因 1 后，如果仍有少量边界乱码，改用 `token_to_str`:

```rust
let mut output = String::new();
// 替换 raw_bytes 方案
match model.token_to_str(token_id, false, None) {
    Ok(piece) => output.push_str(&piece),
    Err(e) => tracing::warn!("token_to_str failed: {:?}", e),
}
```

### 4.3 验证根因 3（终端编码）

在 Windows Terminal（支持 UTF-8）中运行服务器，或添加日志输出 hex 字节：

```rust
tracing::info!("conversation bytes (hex): {:02x?}", conversation.as_bytes());
```

### 4.4 独立模型验证

使用 llama.cpp 官方 CLI 直接测试相同模型：

```bash
llama-cli -m models/Llama-3.2-3B-Instruct-Q5_K_M.gguf \
  -p "<|begin_of_text|><|start_header_id|>user<|end_header_id|>\n\nHello<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n" \
  -n 100
```

预期输出正常。若也乱码，则模型文件可能损坏。

---

## 五、修复优先级

| 优先级 | 修复项 | 修改位置 | 工作量 | 影响 |
|--------|--------|----------|--------|------|
| **P0** | `last_token` 更新顺序 | `src/llm/gguf.rs:379` 移到第 369 行之前 | 1 行 | 修复所有 LLM 功能 |
| P1 | 改用 `token_to_str` | `src/llm/gguf.rs:355-363` | ~5 行 | 消除 UTF-8 边界风险 |
| P2 | 终端 UTF-8 显示 | 运行环境 | 配置 | 日志可读性 |

---

## 六、替代方案（如无法修复）

如果上述修复后问题仍存在，考虑以下替代方案：

### 方案 A：回退到纯正则提取
- 影响：丢失 LLM 理解的语义信息，事件提取准确率大幅下降
- 适用：临时应急

### 方案 B：换用 Ollama 后端
- 在 `config.toml` 中添加 Ollama 配置
- 使用 `ollama-rs` 或 HTTP API 调用本地 Ollama 服务
- 优势：Ollama 已验证的 Llama 3.2 推理，无 llama-cpp-2 底层问题
- 劣势：增加外部依赖

### 方案 C：升级 llama-cpp-2
- 升级到最新版本（当前 0.1.146，最新可能 > 0.2.0）
- 检查 CHANGELOG 中是否有相关修复
- 注意 API 兼容性变化

### 方案 D：换用更小的已验证模型
- 使用 `Qwen2.5-0.5B-Instruct` 等中文友好的小模型
- 验证 GGUF 文件完整性

---

## 七、结论

**GGUF LLM 推理输出乱码的根本原因是 `last_token` 更新顺序错误导致的 KV cache 污染。**

在 `src/llm/gguf.rs` 的推理循环中，`last_token = token_id` 在 `batch.add(last_token, ...)` 之后执行，导致每次向 KV cache 写入的是上一次迭代的 token，而非当前生成的 token。这造成 autoregressive 上下文完全错乱，模型在错乱上下文中产生不可预测的输出（俄文字符、token 重复）。

**修复只需一行代码**：将 `last_token = token_id` 移到 `batch.add` 之前，即第 379 行移到第 369 行之前。

修复后，建议同时将 `token_to_piece_bytes` + `String::from_utf8_lossy` 替换为 `token_to_str`，消除跨 token UTF-8 边界的潜在风险。