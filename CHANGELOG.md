# 变更记录

## [未发布] 2026-10-29 — 242 条验收压测：修 6 个缺陷 + 联系人全息详情 + 进程级加固

用 242 条自造语料（短内容 / 长内容 / 数字公式 / 复杂人物关系 / 多人对话 / **联系人专项** / 边界输入）
做验收前压测，全部走真实链路（Ministral-3B、纯 CPU、独立数据目录 `data_stress2`）。
完整报告见 **[docs/STRESS_TEST.md](docs/STRESS_TEST.md)**。

### 压测结果

- 抽取 **242/242 完成，0 失败、0 崩溃**（耗时约 2 小时 40 分，含一次设备断电重启）；
- 联系人 **111 条**，人名槽位召回 **386/393 = 98.2%**，**凭空捏造 0 条**；
- 事件 101、待办 30；
- 人设 23 条（结构化 JSON 100%），其中 17 条带关联联系人、20 条带关系标签；
- 虚拟人物区分、联系人详情三库联查全部通过。

### 压测暴露并修复的问题

| 级别 | 问题 | 真实表现 | 修复 |
|------|------|----------|------|
| **P0** | llama.cpp 分词正则崩服务 | 含超长数字串/无标点长句/重复字符/emoji 的输入触发 `regex_error(error_stack)`；C++ 异常跨 FFI 抛回 Rust 无法捕获 → `fatal runtime error` → **整个进程 abort**。第一轮 200 条语料崩 4 次，每次中断 50+ 条排队任务 | ① 病态输入识别（数字占比>50% / 同字符连续>100 / 无标点>400 字符）→ 跳过 LLM 走正则兜底；② 提示词字符**白名单**（只放行 CJK/ASCII/常用标点，其余丢弃）；③ 每 400 字符软换行；④ 512 字符分段分词；⑤ 线程栈 2 MB → 32 MB；⑥ `SHOUXIN_SKIP_CHAT_TEMPLATE=1` 规避模板内部整段分词 |
| **P1** | 崩溃导致任务无限重试 | 进程 abort 不走普通失败计数，重启后同一条"毒输入"再次崩溃，队列永远推不动 | 恢复任务时 `retry_count += 1` 并写回，超过 3 次直接放弃该条 |
| **P1** | 归档联系人导致整条对话抽取失败 | 活跃联系人超过 50 条被归档后，对话再提到他们时去重查询过滤了 `is_archived=0` → 查不到 → 新 UUID 插入 → 撞唯一索引 `(name,owner_id,owner_type)` → 整条对话的 contacts/events/todos **全部回滚** | 去重包含归档行；写入时自动复活；唯一冲突再按姓名归并重试一次 |
| **P1** | 心跳重复抽取全部历史对话 | `process_new_conversations` 从 1970 年拉全量对话重跑 LLM，与后台队列完全重复（实测一启动就重跑 144 条），还会造重复事件 | 后台队列存在时心跳不再做对话抽取 |
| **P1** | 歧义检测提示词 47k 字符 | 把 30 段对话全量拼进一个提示词，远超 4096 上下文，每轮白跑 30–90 秒 | 从最新往前取，累计 3000 字符为止 |
| **P2** | 关系短语/组织名被当人名 | "小明的姐姐""客户的工程师""外包团队"进联系人表（逐字校验拦不住） | `is_relational_phrase()`：含"的"、含"和/与"且≥4 字、>6 字、纯身份词一律丢弃；并在 MemoryWriter 写入层再加一道闸（事件里的 `related_person_ids` 会绕过抽取层过滤） |
| **P2** | 默认截止日按 UTC 算 | UTC+8 的 00:00–08:00 之间，"7 天后"算成 6 天后 | 新增 `default_deadline_in_days()`，按本地日期算 23:59:59 |

### 性能优化（不改语义）

- **人名校验缓存**：同一次运行内同名只问模型一次，省掉约 40% 的重复推理；
- **压测脚本状态文件按用户隔离**：此前复用同一个 state 文件，导致"断点续传"把上一轮
  `conversation_id` 当成本轮的，200 条被静默跳过、只提交了 42 条；
- **`scripts/run_supervised.sh`**：崩溃自动重启 + 连续秒退保护。

### 新增能力

- **`GET /api/contacts/:id/detail`**：一次返回人设 + 关联联系人（含标签）+ 关联事件 + 相关记忆原文，
  记忆按姓名/别名逐字命中（有界扫描 500 条），不是向量召回；
- **`is_virtual` 进入联系人检索与详情**，虚拟角色与真实联系人共用同一套接口；
- **虚拟角色支持 `user_id`**：此前 `owner_id` 写死 `default_user`，多用户场景下创建完就查不到。

### 新增工具

- `scripts/stress_200.py`：242 条语料生成 + 提交（限流退避）+ 轮询 + 四项校验；
- `scripts/gen_personas.py`：按轮驱动心跳批量生成画像；
- `scripts/run_supervised.sh`：进程守护；
- `tests/test_contact_detail.rs`：联系人详情三库取数的回归测试。

---

## [未发布] 2026-10-29 — 真实场景跑通测试：修 6 个问题 + 文档重写 + Logo

用"产品经理小林的一天"（4 段对话：晨会 / 客户沟通 / 个人事务 / 技术知识）做端到端实测，
逐项核对抽取、检索、待办、文件、心跳、重启持久化与异常输入。

### 实测发现并修复

| 编号 | 问题（真实表现） | 根因 | 修复 |
|------|------------------|------|------|
| E1 | "月底前出一版方案" 的截止时间变成**当天** | 截止时间取句用"单字命中数"打分，中文常用字几乎人人命中 → 挑错句子 | 改为**覆盖率**（标题字符被句子覆盖 ≥50%），且只在该句/紧邻下一句找时间 |
| E2 | "买维生素" 被安上上一句的"下周一" | 指定句无时间时会**跨句借用** | 匹配句存在时不再跨句回退（宁可回退默认 +7 天）；无匹配句才保留旧的全句扫描 |
| E3 | "月底 / 月初" 解析不出来 | 正则匹配了但解析器不支持 | 新增 `parse_month_edge`（含 12 月跨年） |
| E4 | 5 个互不相关的新事件全被判"相似 0.85"，**一条都没落库** | 合并阈值 0.75/0.90 是按别的嵌入模型定的；e5-base 对无关中文短句本来就有 0.84–0.89 | 阈值改为可配置并校准为 **0.96 / 0.90**（`[extraction]`），实测后事件正常落库 |
| E5 | 事件时间全是"提取当天"（下周三的评审记成今天） | `event_parsed_time` 算出来但**没被使用**（三处写入代码同病） | 统一改用 `event_parsed_time.unwrap_or(now)` |
| E6 | 3.5 万字对话被切成 **175 块**，每块一次 LLM 调用（CPU 上≈1 小时） | 分段合并条件是 `< 80 字符`，且块数无上限 | 改为目标 1200 字符/块、**最多 8 块**，超长部分告警（原文仍在字典库）；实测 142 秒完成 |
| E7 | 联系人校验把整段对话塞进 prompt，超上下文后必然失败并重试 | 未截断 | 校验只取对话前 1500 字符 |
| E8 | 空对话被接受，写入无意义任务 | 缺少非空校验 | 空/纯空白返回 400 |
| E9 | 数据目录硬编码（`./data`、`.sled_data`），配置里的 `data_dir` 被忽略 | 配置加载晚于存储初始化 | 重排初始化顺序，新增 `--data-dir`，支持多实例隔离与备份 |

### 文档

- **README 重写开头**：不再用开发机跑分当标准，改为「三种运行方式 + 三档模型的资源需求」；
  明确标注"想知道自己机器能跑多快，请自己 benchmark"；
- 新增 **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)**：配置覆盖顺序、
  **gguf / Ollama / 云端 API 三种后端切换**、**`context_size` 调节与内存换算**、
  事件阈值校准方法、心跳与待办语义、常用组合示例；
- `docs/PERFORMANCE.md` 的开发机数据明确标注为「仅用于回归对比，不作为选型标准」。

### 品牌

- 新增项目 Logo（`assets/logo.png` 512×512、`assets/logo-128.png`），置于 README 顶部。

### 验证（真实场景，Ministral-3B / 纯 CPU）

- 抽取：4 段对话 → 3 联系人 / 3 待办 / 4 事件，截止时间全部正确（周五=10-30、月底=10-31、默认=11-04）
- 事件时间：评审=11-04、体检=11-02（原文"下周三/下周一"正确解析）
- 语义检索："缓存 key 过期策略" → 命中 0.842；联系人检索、历史检索、智能检索正常
- 待办：完成 / 列表 / 删除正常；文件：上传 / 取文本 / 越权拒绝正常
- 重启持久化：联系人 3 / 待办 2 / 事件 4 全部保留
- 异常：空对话 400、缺字段/非法 JSON 有明确报错、3.5 万字输入 142 秒完成
- `cargo test`：74 单测 + 91 集成 全通过；API 冒烟 35/35

---

## [0.5.0] 2026-10-29 — 心跳审计、磁盘 I/O 优化、开源协议与仓库开放

### 心跳模块审计（12 项维护动作逐条验证）

| 能力 | 结论 | 验证方式 |
|------|------|----------|
| 过期待办失效 | ✅ | 建 `deadline=2026-10-01` 的待办 → 心跳后状态变 `auto_invalid`，日志 `Invalidated 1 expired todos` |
| 待办超量归档 | ✅（按设计只归档陈旧项） | 75 条 pending 时心跳归档 23 条，日志 `Archived 23 stale todos` |
| 待办提醒 | ✅ | pending 超阈值时生成 `⚠️ 你有 N 个待办未处理`，写入侧与心跳侧都有兜底 |
| 联系人画像/关系 | ✅ | 见上一版 C1–C7 修复，实测 tags=['同事'/'上级']、related 反解为姓名 |
| 事件合并待确认 | ✅ | `/api/events/pending_merges` 有数据，过期项由心跳清理 |
| 自适应频率 | ✅ | 日志 `Next heartbeat in 120s (adaptive=true)` |
| 内存保护 | ✅ | 日志 `available memory = 41824 MB (threshold: 4096 MB)` |
| 仅维护模式 | ✅ | `{"maintenance_only":true}` 跳过 LLM，耗时 2–4 ms |
| 并发保护 | ✅ | 重复触发返回 `Heartbeat is already executing` |

**审计中发现并修复**

- `POST /api/todos` 把 `deadline` 当必填字段，导致**无截止时间的待办根本建不出来**
  → 改为可选，缺省取"7 天后 23:59:59"（与抽取链路一致）。
- 曾尝试让"超过上限即归档最旧批次"，但 `tests/test_todo_hot_cold_archive.rs`
  明确约束"**新鲜待办不得被自动隐藏**"，已回退并保留年龄门槛；
  在代码注释与配置注释里写清了这条语义，避免后来者再改错。

### 磁盘 I/O（保护硬盘寿命）

审计到 5 处热点，全部修复：

| 位置 | 问题 | 处理 | 效果 |
|------|------|------|------|
| `heartbeat/executor.rs` 孤儿向量清理 | 每轮心跳（60–120s）**全量读取整个向量库** | 限流为每 6 小时一次；`Vec::contains` 改 `HashSet` | 3 次维护心跳合计读 112 KB |
| `stores/dictionary/store.rs` 时间索引 | `iter()` 从头全量扫描 | 改 `range(since..)` 直接定位 | 心跳不再扫全表 |
| 同上：画像取证最近对话 | 无上限倒扫，owner 对话少时退化为整库遍历 | 最多扫 500 条 | 有界 |
| `stores/vector/store.rs` | **每条记忆 insert 都 `flush()`（fsync）** | 去掉逐条 flush，交给 sled 周期落盘 + 关闭时 flush | 写放大从 N 次同步写降为周期批量 |
| `stores/vector/hnsw_store.rs` | `flush_every_ms(1000)` | 放宽到 5000 ms | 落盘频率降 80% |

**实测**（Ministral-3B 配置、空转 120 秒、无请求）：

```
读 244 KB（61 次） / 写 21.5 KB（22 次）  →  约 122 KB/min 读、10.7 KB/min 写
```

按此折算约 **0.5 GB/月写入**，对 SSD/HDD 寿命影响可忽略。

### 开源协议与仓库

- 新增 **PolyForm Noncommercial License 1.0.0**（禁止商业用途），附 `Required Notice`；
- `Cargo.toml` 补齐 `license` / `description` / `repository` / `readme`；
- 仓库由私有转为**公开**；
- README 开头明确三项预期管理：**面向深度本地智能体用户**、**重代码项目**、**先算物理资源账**。

### 验证

- `cargo test --features gguf --lib`：74 passed
- `cargo test --features gguf --tests`：91 passed（0 failed）
- `bash scripts/api_smoke_test.sh`：PASS=35 FAIL=0 SKIP=0

---

## [未发布] 2026-10-29 — 仓库审计：绝对路径清理、配置失效项修复、换行符规范化

对 GitHub 推送结果、绝对路径、配置文件做了全量审计，修掉 3 类实质问题。

### 1. 绝对路径（仓库可移植性）

| 文件 | 问题 | 处理 |
|------|------|------|
| `scripts/build_gguf.sh` | 写死 MSVC 版本号、`/d/2026-ComfyUI-.../cmake.exe`、甚至 **`E:/ShouXin/...` 项目绝对路径** | 重写为仅做编排，工具链全部交给 `dev_env.sh` 探测 |
| `scripts/dev_env.sh` | 同上 | 改为自动探测：`vswhere` / 扫 VS 安装目录 / SDK 取最新 / cmake 支持 PATH·标准目录·VS 自带·pip；支持 `SHOUXIN_MSVC_ROOT` 等环境变量覆盖 |
| `scripts/cmake_probe.sh` | 写死 crate 版本、构建目录、编译器路径 | 改为复用 `dev_env.sh`，版本从 `Cargo.lock` 读取，构建目录自建 |
| `examples/hnsw_test.rs` | 写死 `D://OpenClaw-Workspace//...//.fastembed_cache` | 改为读 `FASTEMBED_CACHE_DIR`，缺省用 `CARGO_MANIFEST_DIR/.fastembed_cache` |
| `.cargo/config.toml` | 写死本机 CUDA `v12.8` 的 `-L` 与 `CUDA_PATH` | 默认注释掉，改为环境变量方案（默认纯 CPU 构建本就不需要） |
| `src/llm/gguf.rs` | CUDA 探测写死 `C://Program Files\...` | 改用 `ProgramFiles` 环境变量拼接 |

新增 `scripts/toolchain.local.sh.example`：工具装在非标准位置时，复制为
`scripts/toolchain.local.sh`（已 gitignore）填写即可，**仓库内不出现机器路径**。

### 2. 配置失效项（写了不生效）

| 项 | 问题 | 处理 |
|----|------|------|
| `[server]` host/port | **整段未被代码读取**，实际只认命令行 `--host` | 新增 `ServerSettings` 并接入：`--host` 优先，其次配置 |
| `[server].internal_api_token` | 与 `[agent].internal_api_token` 重复，且代码只读后者 | 删除重复项并加注释说明 |
| `[llm.gguf].offload_kv_cache` | 结构体无此字段，`factory` 也未透传 → 恒为 false | 补字段并透传到客户端，配置真正生效 |
| `[embedder].backend` / `hash_dimension` | 文档里有、配置里没有，全靠默认值 | 显式写入并加注释 |
| `[llm.api].model = "gpt-3.5-turbo"` | 过时示例 | 改为 `gpt-4o-mini` 并注明是示例 |
| 重复/矛盾注释 | `[llm.gguf]` 有两条重复的 CPU 说明 | 清理 |

新增 `scripts/check_config.py`：校验 TOML 语法 + **死配置**（配置里有、结构体没有）
+ 漏配置提示。当前结果：`config.toml` / `config.3b.toml` 均 0 死配置。

### 3. 换行符（导致脚本完全无法执行）

Windows 下用文本模式批量改文件，把多个脚本写成了 **CRLF**，
表现为 `bash scripts/build_gguf.sh` **无任何输出、退出码 2**。

- 新增 `.gitattributes`：`*.sh / *.py / *.rs / *.toml / *.md` 强制 `eol=lf`，二进制按 binary 处理；
- 仓库 `core.autocrlf=false`、`core.eol=lf`；
- 全仓 43 个文本文件归一化为 LF；
- 排障文档新增「脚本没输出、退出码 2」一节。

### 验证

- `cargo test --features gguf --lib`：**74 passed**
- `cargo test --features gguf --tests`：**165 passed**（0 failed）
- `bash scripts/api_smoke_test.sh`：**PASS=35 FAIL=0 SKIP=0**
- `python scripts/check_config.py config.toml config.3b.toml`：**0 死配置**
- `[server]` 生效验证：不传 `--host` 时监听 `127.0.0.1:3000`；
  `SHOUXIN__SERVER__PORT=3123` 时监听 `:3123`
- `bash scripts/build_gguf.sh`：自动探测到 MSVC 14.44 + SDK 10.0.26100，构建成功

---

## [未发布] 2026-10-29 — 资源占用硬约束：默认不占显存 + GPU 模式 CPU ≤ 60%

背景：守心是**后台服务**，必须与其它进程共存。显存是稀缺资源，CPU 也不能被吃满。

### 变更

| 项 | 变化 |
|----|------|
| **默认纯 CPU** | `use_gpu` 的**代码缺省值从 `true` 改为 `false`**（配置缺省与 `GgufClient` 内部缺省一致），`n_gpu_layers` 恒为 0。不显式打开就绝不占显存 |
| **新增 `gpu_max_cpu_percent`** | `[llm.gguf]` 新配置项，默认 **60**。GPU 模式实际线程数 = `min(threads, **物理核数** × 百分比%)`，向下取整、最少 1 |
| 纯 CPU 模式 | 线程数沿用 `threads`，且不超过核心数 |
| 心跳客户端 | 固定纯 CPU（`use_gpu=false`），不占显存 |

### 实现

```rust
// src/llm/gguf.rs
pub(crate) fn resolve_threads(configured, cores, gpu_enabled, percent) -> usize {
    let cores = cores.max(1);
    if gpu_enabled { cap_threads_for_gpu(configured, cores, percent) }
    else { configured.max(1).min(cores) }
}
```

- 基准取**物理核心数**（`num_cpus::get_physical()`），不是逻辑核 ——
  否则 32 线程（16 物理核 + 超线程）× 60% = 19 会超过物理核数，反而造成超载；
- GPU 模式被压制时打日志：`[GGUF] GPU 模式限制 CPU 占用：threads 32 -> 19（核心 32，上限 60%）`；
- 启动日志新增核心数与上限信息，一眼可核对：
  `[GGUF] 初始化: GPU=false, n_gpu_layers=0, context=4096, threads=16（物理核 16，GPU 上限 60%）`；
- **未编译 CUDA 却写了 `use_gpu=true`** 时明确告警并回退纯 CPU（避免"以为用了 GPU、实际既没加速也没省事"）：
  `配置要求 GPU 推理（use_gpu=true），但当前二进制未编译 CUDA 后端…本次将回退纯 CPU 运行`。

### 测试

- 新增 5 个单元测试（`thread_cap_tests`）：60% 折算、配置低于上限时不干预、
  下限为 1、百分比越界夹取、CPU 分支不叠加限制；
- `cargo test --features gguf --lib`：**74 passed**（原 69 + 5）；
- `cargo test --features gguf --tests`：**91 passed**；
- `bash scripts/api_smoke_test.sh`：**PASS=35 FAIL=0 SKIP=0**（Ministral 3B 配置实跑）。

GPU 模式的线程压制已在运行时实测（用 `use_gpu=true` 的临时配置启动）：

```
[GGUF] GPU 模式限制 CPU 占用：threads 16 -> 9（核心 16，上限 60%）
[GGUF] 初始化: GPU=false, n_gpu_layers=0, threads=9（物理核 16，GPU 上限 60%）
[GGUF] 配置要求 GPU 推理（use_gpu=true），但当前二进制未编译 CUDA 后端…回退纯 CPU 运行
```

即：16 物理核 → 最多 9 线程（≤60%），且因为本机是纯 CPU 构建，显存占用为 0。

---

## [未发布] 2026-10-29 — 推理模型换挡：下线 Llama-3.2，新增 Ministral-3B 低配档

### 模型

| 项 | 变化 |
|----|------|
| 退役 | `Llama-3.2-3B-Instruct-Q5_K_M` **不再使用**，配置/脚本/文档中的引用全部移除 |
| 新增低配档 | `Ministral-3-3B-Reasoning-2512-Q8_0.gguf`（3.4 GB）+ `mmproj-BF16.gguf`（0.78 GB） |
| 标准档 | 不变，仍为 `gemma-4-E4B-it-Q4_K_M`（默认 `config.toml`） |
| 新增配置 | `config.3b.toml` —— 指向 Ministral，`SHOUXIN_CONFIG=./config.3b.toml` 即可切档 |
| 架构兼容 | Ministral 为 `mistral3`，`llama-cpp-sys-2 0.1.146` 已支持；对话模板从 GGUF 正常读取（`[INST]`） |

实测（16 线程 / ctx 4096 / 纯 CPU / 服务未运行）：

| 模型 | 生成速度 | 单次抽取端到端 |
|------|----------|----------------|
| Gemma-4-E4B Q4_K_M | ≈ 16 tok/s | ≈ 33 s |
| Ministral-3-3B Q8_0 | ≈ 15.4 tok/s | ≈ 41 s |

### 新增

- `scripts/api_smoke_test.sh`：**全量 API 冒烟测试**，覆盖 `routes.rs` 注册的全部 35 个端点
  （记忆写入/状态/重放、四类检索、待办 CRUD、事件合并、文件上传与越权、心跳、
  虚拟角色 CRUD、向量检索），自带数据隔离与结果汇总。
  当前基线：`PASS=35 FAIL=0 SKIP=0`，Gemma 与 Ministral 两套配置均通过。
- `config.3b.toml`：低配 3B 档配置模板。

### 修正

- `scripts/check_env.sh` 支持 `SHOUXIN_CONFIG`，可校验非默认配置（如 `config.3b.toml`）；
- 文档中"跨 owner 访问返回 404"更正为**实际行为**：HTTP 200 + `{"success":false,...}`
  （不泄露资源是否存在）；
- 明确记录一个测量陷阱：**服务运行中跑基准会因 CPU 争抢导致数字严重失真**
  （64 token 从 5 s 恶化到超时），已在 PERFORMANCE.md 标注。

---

## [未发布] 2026-10-29 — 联系人画像与关系自动生成（专项）

针对"联系人的人设/别名/社会关系/联系人之间关系能否自动生成"做了完整排查，
发现链路有 6 处断点，全部修复并端到端验证。

| 编号 | 问题 | 影响 |
|------|------|------|
| C1 | `search_contacts` 构造 `ContactDetail` 时把 `aliases/background/related_contacts/relationship_tags` **硬编码为空** | 即使库里有画像，接口也只返回空值 —— 看起来像"从没生成过" |
| C2 | 别名分两处存（`contact_index.aliases` 与 `contact_alias_index`），读取只看前者 | 抽取器写入的昵称（"王建国"）在接口里看不到 |
| C3 | `build_chat_prompt` 在模型句柄加载**之前**执行，首次调用拿不到 GGUF 模板 | 心跳画像用错模板 → 模型复述提示词 → 画像 JSON 永远解析失败 |
| C4 | 画像提示词要求"全部输出中文"，模型把 **JSON key 也翻成中文** | 解析器只认英文 key，字段全空 |
| C5 | 画像只喂"事件摘要"，不喂对话原文；且召回用的 `get_conversations_since_v2` **只返回 pending/failed** | 模型看不到"老王是我的同事"，只能输出"未知" |
| C6 | 画像里的 `social_relations` 只写进 `background` 的 JSON，`relationship_tags` / `related_contact_ids` 两列从未更新 | 联系人之间的关系永远是 `[]` |
| C7 | 抽取提示词的 few-shot 示例（李总/赵总/王总监…）被模型照抄成联系人 | 联系人列表被"李总的女儿""张总的秘书"等**虚构人名**污染 |

修复要点：

- 读取路径真正读库，并合并两处别名来源；关联联系人 ID 批量解析成姓名
- `build_chat_prompt` 前先 `ensure_loaded()`，杜绝"首次调用用错模板"
- 画像提示词：key 保持英文、value 用中文，并给出结构示例；解析器兼容数组/对象/中文 key 三种形态
- 画像证据补齐：新增 `get_recent_conversations_for_owner()`（不区分抽取状态），
  把"含该联系人或其别名"的最近 3 段原文喂进提示词
- 画像落库时同步写 `relationship_tags` 与 `related_contact_ids`，
  并把 `related_contact_name` 在同一 owner 内解析成 ID（`find_contact_id_by_name_same_owner`）
- 联系人过滤新增规则 8：**姓名或其别名必须逐字出现在对话原文中**，否则丢弃

端到端实测（用户 `rel4`，对话含"老王是我同事／小李与老王同组／张姐是上级"）：

```
张姐  aliases=[]        tags=['上级']  related=['老王','小李']  画像 505 字符
小李  aliases=[]        tags=['同事']  related=['老王','张姐']  画像 680 字符
老王  aliases=['王建国'] tags=['同事']  related=['小李']        画像 579 字符
```

> 生成机制与频率：由**心跳**驱动（`analyze_contact_relations`），每轮最多处理 3 个"画像为空"的联系人，
> 单个联系人每天最多更新 2 次、间隔 ≥ 8 小时（`MAX_PROFILE_UPDATES_PER_DAY` / `PROFILE_UPDATE_INTERVAL_HOURS`）。
> 想立即看到效果可 `POST /api/heartbeat/trigger {"maintenance_only": false}`。

---

## [未发布] 2026-10-28 — 启动阻塞、CPU 性能、新模型

本轮目标：让项目**能按文档从零部署并跑通**，并把 CPU 推理拉到可用水平。

### 修复（影响正确性）

| 编号 | 问题 | 影响 | 处理 |
|------|------|------|------|
| F1 | 配置把**缓存目录**当成**模型名**传给 FastEmbed | 嵌入模型永远加载失败，服务起不来 | 配置拆成 `model_path`（缓存根）与 `model`（HF 仓库 ID） |
| F2 | 嵌入模型是硬依赖 | 缺模型直接退出 | 新增 `[embedder].backend = fastembed\|hash` + `fallback_to_hash`，并实现离线哈希嵌入器 |
| F3 | 待办"模糊匹配"实为 `LIKE '%词%'` 连续子串 | "写报告"匹配不到"写项目报告"，按对话更新待办失效 | 改为子序列匹配（含单测） |
| F4 | `parse_event_time` 的"下周X"多算一周 | "下周一"算成 11-09（应为 11-02），所有事件时间受影响 | 公式修正为 `7 - since + offset` |
| F5 | 待办 deadline 直接采信模型给的日期 | 小模型常给出过期日期（"下周一"→ 07-11） | 改为优先用本地 `time_parser` 从原文解析，仅精确时间表达；模型日期退为兜底 |
| F6 | `pending_merges` 列表解析按 RFC3339 处理 SQLite 时间 | 接口报 `premature end of input`，确认入口不可用 | 新增 `parse_sqlite_datetime()` 兼容 `2026-10-28 13:38:37` 格式 |
| F7 | 提示词整体塞进一个 `n_batch` 大小的 batch | `prompt_tokens > n_batch` 时必然失败，LLM 抽取**静默降级为正则** | 改为分块预填充（chunked prefill），且只有最后一个 token 取 logits |
| F8 | 对话模板写死 Llama-3 格式 | 换 Gemma 后模型复述原文、拿不到 JSON | 优先用 GGUF 自带 `tokenizer.chat_template`，失败则按 `general.architecture` 内置 gemma / qwen / llama 模板 |
| F9 | 停止判定只比 `token_eos()` | Gemma 的 `<end_of_turn>` 不触发停止，白跑 1024 token | 改用 `is_eog_token()` |
| F10 | `n_gpu_layers = 0` 被当成"自动探测" | 宣称纯 CPU 却仍卸载 28 层到 GPU；心跳客户端还硬编码 99 层 | 新增 `[llm.gguf].use_gpu` 显式开关，心跳客户端同步 |

### 修复（影响性能）

| 编号 | 问题 | 效果 |
|------|------|------|
| P1 | 每 token 对全词表做全量构造 + 排序采样 | 改用 llama.cpp 原生采样链（top-k → top-p → temp → dist） |
| P2 | `GGML_AVX2=ON` 只写进缓存变量，没有变成 `/arch:AVX2` | 工具链文件里显式注入编译标志 |
| **P3** | **`CMAKE_SYSTEM_PROCESSOR` 为空 → ggml 走 `-DGGML_CPU_GENERIC` 标量兜底** | **生成速度 3.8 → 24 tok/s（3B）**，单次调用固定开销 8.9 s → 1.3 s |
| P4 | 构建脚本用 POSIX 路径调 Windows 版 cmake/ninja | 表现为"构建脚本静默退出 1"，改用 Windows 绝对路径 |

详见 [docs/PERFORMANCE.md](docs/PERFORMANCE.md)。

### 新增

- `src/bin/bench_gguf.rs`：纯推理基准工具（可扫线程数），配 `scripts/bench_cpu.sh`
- `scripts/download_gguf.py`：分段并行 + 断点续传下载，结束校验文件长度
- `scripts/check_env.sh`：部署前环境自检
- `scripts/dev_env.sh`：导出 MSVC/Ninja/AVX2 环境，使任意 `cargo` 命令（含 `cargo test --features gguf`）可用
- `scripts/llama_cpu_toolchain.cmake`：MSVC 下强制 AVX2 + 正确架构
- `docs/`：`MODEL_SETUP` / `BUILD` / `PERFORMANCE` / `TROUBLESHOOTING` / `API` / `ARCHITECTURE`
- `GET /api/events/pending_merges`、`POST /api/events/pending_merges/{id}/resolve`

### 变更

- 默认推理模型换为 `gemma-4-E4B-it-Q4_K_M`（4.6 GB），嵌入仍为 `multilingual-e5-base`
- `gguf` 特性改为**纯 CPU**（不再编译 CUDA 后端）；新增 `gguf-cuda` 供需要 GPU 时使用。
  副作用：二进制不再依赖 `cublas64_12.dll`，构建时间从 ~15 分钟降到 ~1 分钟
- 配置默认值：`threads=16`、`batch_size=2048`、`context_size=4096`、`timeout_secs=1800`、`use_gpu=false`

### 验证基线

- `cargo test --lib` 69 passed / 0 failed
- `cargo test --tests` 91 passed / 0 failed
- 真实语料端到端：Gemma-4-E4B @ 15.5–16 tok/s，33 s 完成 2 人 / 5 待办提取，
  deadline 全部正确（下周一=11-02、下周三=11-04、下周五=11-06）

### 已知限制

- 事件相似度 0.75–0.90 不自动合并，需确认后才进索引（可用接口确认）
- mmproj 视觉塔已下载但**不加载**（抽取链路为纯文本）
- `llama-cpp-2` 0.1.160 的 crate 包缺少 `llama.cpp/app` 目录无法编译，故仍用 0.1.146（已支持 GEMMA4）
