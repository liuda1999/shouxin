# 排障手册（Troubleshooting）

按"观察到什么现象"组织，每条都给判定方法和处理方式。

---

## 1. 启动相关

### 1.1 `Embedder initialized` 后面跟着 `backend: hash`

不是错误，是**降级**。含义：FastEmbed 没能加载 ONNX 模型，改用离线哈希嵌入器。
检索还能用，但语义精度下降。

排查：

```bash
# 模型是否在该在的位置（注意是 {缓存根}/<org>/<model>/onnx/model.onnx）
ls -l .fastembed_cache/intfloat/multilingual-e5-base/onnx/model.onnx

# 目录布局对了但还是失败？看是否把「缓存目录」当成「模型名」了
grep -A 4 '\[embedder\]' config.toml
#   model_path = ".fastembed_cache"            ← 缓存根目录
#   model      = "intfloat/multilingual-e5-base"  ← 仓库 ID，不是路径
```

早期最常见的坑就是 `model` 里填了路径，FastEmbed 会去拼
`.fastembed_cache/.fastembed_cache/onnx/model.onnx`，必然找不到。

### 1.2 启动即退出，日志里没有 `Server listening`

按顺序看最后一条日志：

| 最后一条日志 | 处理 |
|--------------|------|
| `[GGUF] 初始化: ...` 之后无输出 | 模型文件不完整/损坏，见 2.1 |
| `备模型客户端创建失败: 未知的 LLM 来源: none` | 无害警告（`secondary = "none"` 时的正常提示） |
| `Address already in use` | 端口被占：`--host 127.0.0.1:3001` 或结束旧进程 |

---

## 2. 推理相关

### 2.1 `加载 GGUF 模型失败` / `unknown model architecture`

| 可能原因 | 判定 | 处理 |
|----------|------|------|
| 文件被截断 | `xxd -l 4 model.gguf` 不是 `GGUF` | 重新下载，用 `scripts/download_gguf.py`（结束会校验长度） |
| 大小不符 | `ls -l` 与上游 `Content-Length` 不一致 | 同上 |
| 架构不支持 | 日志含 `unknown model architecture` | 换模型或升级 `llama-cpp-2`；本项目自带版本支持 `GEMMA/GEMMA2/GEMMA3/GEMMA4` |

### 2.2 推理很慢（< 5 tok/s）

**这是本项目踩过的最深的坑**，完整分析见 [PERFORMANCE.md](PERFORMANCE.md)，这里只给最短判定：

```bash
B=$(ls -d target/debug/build/llama-cpp-sys-2-*/out/build | head -1)

# ① 必须为 0：非 0 表示 ggml 只编译了标量通用实现（会慢 20 倍）
grep -c "GGML_CPU_GENERIC" "$B/build.ninja"

# ② 必须有 /arch:AVX2
grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" | sort -u
```

若 ① 不为 0 → `scripts/llama_cpu_toolchain.cmake` 里的 `CMAKE_SYSTEM_PROCESSOR` 补丁没生效
（构建目录可能是旧的），执行：

```bash
cargo clean -p llama-cpp-sys-2 && bash scripts/build_gguf.sh
```

若两项都正常仍很慢，检查：

- `threads` 是否等于物理核心数（32 线程实测不如 16）；
- 是否有其它进程抢 CPU（后台抽取、心跳、其它模型服务）；
- 是否误开了 GPU 卸载但机器无 GPU：`use_gpu` 应为 `false`。

### 2.3 抽取状态 `completed`，但结果明显变差
**先查是不是降级成了正则提取**：

```bash
grep -E "falling back to regex|Insufficient Space|LLM call failed" .server.log | tail
```

出现 `Insufficient Space of N` → 提示词长度超过 `n_batch`，说明分块预填充没生效
（旧版本 bug，修复方式见 [PERFORMANCE.md 第 4 节](PERFORMANCE.md)）。
临时绕过：把 `[llm.gguf].batch_size` 调大（如 4096）。

### 2.4 抽取很慢（> 5 分钟）

小模型纯 CPU 的每一步都要十几秒，而抽取链路会做**多次** LLM 调用（主抽取 + 逐个人名校验）。
先确认单次推理速度正常（2.2），再考虑：

- 降低 `[llm].max_tokens`；
- 减少逐条人名校验（改配置或用更强的模型一次判完）；
- 提高 `[llm].timeout_secs`，避免整轮超时后降级。

### 2.5 换模型后输出在"复述原文"，拿不到 JSON

**症状**：抽取返回一大段和输入几乎一样的文字；日志里 `生成 1024 tokens`（正好撞 `max_tokens`）。

两个原因，都出在**对话模板**上：

1. **模板不匹配**。旧实现把 Llama-3 的 `<|begin_of_text|><|start_header_id|>…` 写死了，
   换成 Gemma 后模型把这个当"续写任务"，于是照抄输入。
   现在会优先读 GGUF 自带的 `tokenizer.chat_template`（`model.chat_template(None)`）；
   读不到时按 `general.architecture` 走内置模板（gemma / qwen / llama 三套）。
   验证方式：日志里 `Constructed prompt` 的开头应该是模型对应的标记（Gemma 为 `<start_of_turn>user`）。
2. **停止标记识别不全**。Gemma 的对话结束符是 `<end_of_turn>`，它不等于 `token_eos()`。
   只判 `token_eos()` 会一直生成到 `max_tokens`。
   现在用 `is_eog_token()`（覆盖所有结束 token）判定。

若你换的是本实现未内置模板的模型（如 Mistral 的 `[INST]`），需要：
- 确认该 GGUF 带 `tokenizer.chat_template` 元数据（`llama_model_loader` 启动日志里能看到这一项）；或
- 在 `src/llm/gguf.rs::build_chat_prompt` 里按 `arch` 增加一套模板。

---

## 3. 构建相关

| 报错 | 原因 | 处理 |
|------|------|------|
| `error C2001: newline in constant` | MSVC 按 GBK 解析 UTF-8 源码 | 用 `scripts/build_gguf.sh`（内部传 `-utf-8`），或跑 `scripts/fix_llama_cpp_utf8.sh` |
| 构建脚本**静默退出 1**（无任何输出） | `PATH` 是 POSIX 形态，Windows 版 cmake 解析不了；或命中了 python/Scripts 下的**失效 cmake shim** | 用脚本内的 Windows 绝对路径调用 cmake |
| `is cmake not installed?` | 同上 | 同上 |
| `Failed to run reg.exe` / cmake 卡在探测 VS | VS 生成器要读注册表 | 用 Ninja + 显式 `cl.exe`（脚本已做） |
| `schannel: ... CRYPT_E_NO_REVOCATION_CHECK` | TLS 吊销检查被环境阻断 | `git -c http.schannelCheckRevoke=false clone …`；curl 加 `--ssl-no-revoke` |
| `error LNK1104: 无法打开 cublas64_12.lib` | 用了 `gguf-cuda` 但没装 CUDA | 改用 `--features gguf`（纯 CPU） |
| 首次构建 10 分钟 | 要完整编译 llama.cpp | 正常；之后增量编译约 1 分钟 |

---

## 4. 运行与数据相关

### 4.1 文件上传后检索不到

1. 上传是异步解析，稍等再查；
2. `GET /api/files/:id/text` 确认文本是否提取成功（PDF 扫描件无文本层会为空）；
3. 语义检索需要 `scope` 与 `user_id` 与上传时一致。

### 4.2 跨用户访问返回"文件不存在"

这是**设计行为**：越权时不泄露资源是否存在。

### 4.3 事件检索为空，但抽取说提取到了事件

事件相似度落在 0.75–0.90 区间时不会自动合并，而是进入 `pending_merges` 等确认：

```bash
curl "http://127.0.0.1:3000/api/events/pending_merges?owner_type=User&owner_id=default_user&limit=50"
curl -X POST http://127.0.0.1:3000/api/events/pending_merges/<id>/resolve \
  -H "Content-Type: application/json" -d '{"action":"merge"}'
```

确认后事件才会进索引。

### 4.4 待办截止时间不对

已改为**优先用本地 `time_parser` 从原文解析**，模型给的日期只在本地解析失败时兜底。
若仍不对，检查原文是否包含可识别的时间表达（今天/明天/后天/下周X/本周X/X月X日/X号/月底/月初）。

已知限制：`下季度`、`下半年` 这类**时间范围**不会被当作截止日（这是刻意的）。

---

### 4.5 联系人的人设/别名/关系一直是空的

按顺序检查（这四项分别由不同环节产出）：

```bash
# ① 库里到底有没有（判断是"没生成"还是"没读出"）
python - <<'PY'
import sqlite3
c=sqlite3.connect('file:data/shouxin.db?mode=ro', uri=True)
for r in c.execute("select name,length(coalesce(background,'')),relationship_tags,related_contact_ids,aliases "
                   "from contact_index order by last_interaction_at desc limit 5"):
    print(r)
PY
```

- 库里有内容、接口是空 → 读取路径问题（历史 bug，见 CHANGELOG C1/C2）
- 库里也是空 → 继续：

```bash
# ② 画像生成有没有真的跑起来
grep -iE "Failed to parse profile|画像|contacts updated" .server.log | tail
# ③ 心跳是否被频率限制挡住（每天 2 次、间隔 8h）
grep -i "Profile update blocked" .server.log | tail
```

- 出现 `Failed to parse profile`：模型没按 JSON 输出，确认提示词里"key 保持英文"的约束是否生效
- 想立刻重跑：`POST /api/heartbeat/trigger {"maintenance_only": false}`
- 关系（`relationship_tags` / `related_contact_ids`）需要画像里的 `social_relations` 非空；
  若模型只回"未知"，说明**提示词里缺少关系证据** —— 检查该联系人的对话原文是否被召回
  （需要同 owner 下存在包含其姓名/别名的对话）

### 4.6 联系人列表里出现没说过的人名

小模型会照抄提示词 few-shot 示例里的人名（"李总的女儿""张总的秘书"）。
现在过滤规则会要求姓名逐字出现在对话原文中，若仍出现，检查 `.server.log` 里
`[Contact Filter] Removed ... 对话原文中未出现` 是否生效。

---

## 5. 收集诊断信息

```bash
# 版本与配置
./shouxin-server --version
grep -E "^\[|primary|model|threads|batch_size|backend" config.toml

# llama.cpp 真实编译参数
B=$(ls -d target/debug/build/llama-cpp-sys-2-*/out/build | head -1)
grep -c "GGML_CPU_GENERIC" "$B/build.ninja"
grep -oE "/arch:[A-Za-z0-9]+" "$B/build.ninja" | sort -u

# 推理吞吐
grep "GGUF] 生成" .server.log | tail -20

# 降级痕迹
grep -E "falling back|Insufficient Space|超时|timed out" .server.log | tail -20
```
