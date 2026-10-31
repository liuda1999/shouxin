# 守心 ShouXin 外部测试报告

| 项 | 内容 |
|----|------|
| 测试对象 | shouxin-server v0.6.7（commit `bb578d0`） |
| 测试日期 | 2026-11-01 |
| 测试方式 | 黑盒为主：以"接入守心的智能体"身份，走完整读写链路；关键结论均用独立数据目录实测复现，非静态推断 |
| 运行配置 | `config.toml` 原样（gguf / gemma-4-E4B-it-Q4_K_M / 纯 CPU / 127.0.0.1:3000） |
| 测试数据 | 全新 `--data-dir`，自造多日中文闲聊语料，未污染仓库既有 `data*/` |
| 结论 | 基础设施（写入、三库、抽取链路、画像）扎实；**接入层的语义理解是主要短板**，其中 0 个 P0（身份校验按本地单机威胁模型排除）、8 个 P1 |

> 本文档面向修复。每条问题都包含：现象 → 复现 → 根因（文件:行号）→ 影响 → 修复建议 → 验证方法。
> 标 `已确证` 的是实测复现或代码逐行核对过；标 `待确认` 的是静态分析推断，修复前需先复现。

---

## 一、测试方法

分两轮，第二轮刻意不参考第一轮结论、换角度重测，用于交叉验证：

1. **第一轮（缺陷导向）**：官方冒烟 + 单元测试建立基线，然后对边界、并发、权限、异常路径做定向攻击。
2. **第二轮（体验导向）**：把自己当成第一次接入的智能体，用 5 段"多日闲聊"喂进去，再像智能体那样去回忆——问"李明最近有什么变化"这类自然语言。这一轮暴露的问题最贴近真实可用性。

每轮都用 `rm -rf` 新建数据目录、独立启动进程，避免历史数据干扰结论。

---

## 二、结论摘要

### 问题统计

| 级别 | 数量 | 含义 |
|------|------|------|
| P0 | 0 | ——（原 X-01 身份校验按本地单机威胁模型排除，见 §4 X-01） |
| P1 | 8 | 核心功能失效 / 数据错误 / 接口契约不一致 |
| P2 | 13 | 健壮性、性能、体验、潜伏缺陷 |

合计 **21 条待修 + 1 条已排除**（X-01）。

### 一页速览

| ID | 级别 | 问题 | 一句话后果 |
|----|------|------|-----------|
| X-02 | P1 | `POST /api/todos` 的 `user_id` 只认 body | 按 REST 惯例传 query 的客户端，数据全落 `default_user` |
| X-03 | P1 | 融合检索只做整串子串匹配 | "李明"能查到，"李明最近怎么样"返回空 |
| X-04 | P1 | 历史检索不接受 query，limit 也无效 | 接口名是 search，实现是 list |
| X-05 | P1 | 截止时间跨句借用 + "X号"解析失败 | "12月20号前"被记成今天，6 小时后自动消失 |
| X-06 | P1 | 文件分块无单块体积上限 | 无空行的大文件 100% 入库失败 |
| X-07 | P1 | 文件处理失败不落状态、不删临时文件 | 状态永驻 processing、错误为 null、磁盘泄漏 |
| X-08 | P1 | 待办引用的 contact_id 悬空（实测 50%） | 联系人详情的关联待办永远缺一块 |
| X-09 | P2 | 限流信任 X-Forwarded-For | 一行 header 绕过限流（仅对外暴露时成立） |
| X-10 | P1 | 后台队列串行且无界 | 积压后新请求长时间 pending，内存无上界 |
| X-11 | P2 | 推理超时硬编码 180s | 配置 `timeout_secs=1800` 完全不生效 |
| X-12 | P2 | 向量库容量硬编码 10000 | 长期使用必撞天花板，无告警无扩容 |
| X-13 | P2 | 事件抽取漏关键事件，画像与事件不一致 | 画像提到的事，事件列表里找不到 |
| X-14 | P2 | 待办语义边界错误 | 已发生的事、他人的事都被当成本人待办 |
| X-15 | P2 | 融合检索先截断后过滤 | 匹配项排在第 N 名之后就被截掉 |
| X-16 | P2 | 融合检索的历史硬编码 30 天窗口 | 30 天前的原文在智能检索里不可见（直查能返回） |
| X-17 | P2 | 联系人检索不支持多词 | "李明 王芳" → 0 条 |
| X-18 | P2 | 同步抽取路径 dedup_key 用整段对话 | 一段对话只能落 1 个事件（当前主链路未触发） |
| X-19 | P2 | 启动日志被 llama.cpp 刷屏 | 关键就绪信息埋在 5 万字符里 |
| X-20 | P2 | 错误响应格式不统一 | 422 返回纯文本、400 返回 JSON |
| X-21 | P2 | 画像藏在 `background` 的 JSON 字符串里 | 调用方需二次解析 |
| X-22 | P2 | 推理信号量每实例独立、超时不中止 worker | 心跳与主链路可并发推理同一模型 |

> **X-01 已排除**：待办读写不做身份校验，经复核属于本地单机威胁模型之外，不计入待修项。评估过程与"若对外暴露时"的修复方案保留在 §4 X-01，避免后人误以为是漏测。

---

## 三、基线：已验证正常（修复时勿破坏）

这些都是实测通过的，作为回归基线：

- `scripts/api_smoke_test.sh`：**39/39 PASS**（含 1 SKIP：无待确认合并）。
- `cargo test --features gguf --lib`：**104 passed, 0 failed**。
- 写入延迟：**5–52 ms**，符合"毫秒级返回"承诺。
- 冷启动：页缓存命中时约 **4 秒**就绪。
- 三库落库正常：5 段对话 → 3 联系人 / 5 事件 / 6 待办 / 8 历史 / 2 份画像。
- 相对时间解析正确：`下周一` → 11-02、`三个月后` → 2027-01-29、`下周` → 11-08。
- 联系人画像生成成功，`objective_evaluation` 质量明显高于事件抽取。
- 无 SQL 注入（全部参数化绑定）、无路径穿越（file_id 为服务端 UUID）、`/admin` 与 `/internal` 鉴权 fail-closed。

---

## 四、问题详情

### X-01 【已排除｜本地单机威胁模型】待办详情/完成/删除不校验调用者身份

**复核结论（2026-11-01 补充）**

本项目定位为**本机单机使用**：服务默认绑定 `127.0.0.1:3000`，调用方只有本机上的智能体，不存在互不信任的主体。因此"不带 token、仅凭 `todo_id` 即可读写"**不构成安全缺陷**，本条从待修清单中排除。

保留此条目是为了留痕：说明它被评估过，而不是漏测。同时记录一个**前置条件**——一旦服务改为监听 `0.0.0.0`、经反向代理暴露，或同机出现多个互不信任的调用方，本条立即回到 P0，按本节末尾的方案修复。

**现象（已确证）**

用户 `alice` 创建待办 `012aeb13-...`，随后**不带任何身份信息**（无 token、无 user_id、无 scope）：

```
PUT /api/todos/012aeb13-.../complete   → 200 {"success":true}
GET /api/todos/012aeb13-...            → 200 返回 alice 待办的完整内容
```

状态被改写为 `completed`，`completed_at` 被写入。README 宣称"越权访问不泄露存在性"，实际返回 `success:true` + 完整数据。

**复现**

```bash
TID=$(curl -s -X POST "http://127.0.0.1:3000/api/todos" \
  -H "Content-Type: application/json" \
  -d '{"title":"alice私密待办","user_id":"alice"}' \
  | python3 -c "import sys,json;print(json.load(sys.stdin)['data']['todo_id'])")

curl -s "http://127.0.0.1:3000/api/todos/$TID"                     # 期望 401/403，实际 200
curl -s -X PUT "http://127.0.0.1:3000/api/todos/$TID/complete"     # 期望 401/403，实际 200
curl -s -X DELETE "http://127.0.0.1:3000/api/todos/$TID"           # 同上
```

**根因**

`src/api/routes.rs:2394-2411`：

```rust
async fn resolve_owner(
    async_memory: &AsyncMemory,
    scope: Option<Scope>,
    user_id: Option<&str>,
    todo_id: &str,
) -> (OwnerType, String) {
    if let Some(scope) = scope {
        return scope.to_owner_params(user_id);
    }
    // 未显式指定作用域时，以待办自身的归属为准。
    if let Ok(Some(owner)) = async_memory.get_todo_owner(todo_id).await {
        return owner;      // ← 从被访问对象自身取 owner，再拿它去查自己
    }
    Scope::Agent.to_owner_params(user_id)
}
```

调用方不提供身份时，函数**从待办记录里读出 owner，再原样回传用于鉴权**。这等价于没有鉴权——任何持有 id 的人都能通过。

注释里解释了动机（F-T2：不传 scope 时 Agent 作用域的待办查不到，于是回退到"以自身归属为准"），但修法选错了方向：用"降低校验"解决"查不到"。

**影响（仅在突破本机边界后成立）**

- **单机内部**：调用方本就是唯一的本机智能体，不存在"越权"语义，影响可忽略。
- **若对外暴露**：任一持有 id 的客户端都能读改删全部待办，并配合 X-09 批量遍历，届时为 P0。
- **文档层面**：README 把"多用户隔离 / 越权访问不泄露存在性"写成安全承诺，与"单机不鉴权"的实际定位不一致，容易让接入方误判（见附录 D）。

**若将来对外暴露，按以下方案修复（当前不执行）**

1. `get_todo` / `complete_todo` / `delete_todo` 三处 handler 必须要求调用者身份（`scope` 或 `user_id` 至少其一），缺失即 `401`。
2. `resolve_owner` 只负责**把调用者身份归一为 (owner_type, owner_id)**，不得从目标对象反查。
3. 保留 F-T2 的诉求，正确做法是：查不到时按两种 owner 类型各查一次（`User` + `Agent`），而不是从对象身上取。
4. 越权统一返回 `404`（不泄露存在性），不要返回 `403`。

**验证**

- 补集成测试：A 用户建待办 → B 用户凭 id 调用三个接口 → 全部 404/401。
- 现有冒烟里的"`GET /api/files/:id`(越权应拒绝) 返回 200"实际是被当作 PASS 的，说明断言口径也需一并修正。

---

### X-02 【P1】`POST /api/todos` 忽略 query 中的 user_id，数据全部落到 default_user

**现象（已确证）**

| 请求 | 实际 owner_id |
|------|--------------|
| `POST /api/todos?user_id=alice`（body 无 user_id） | **default_user** |
| `POST /api/todos`（body `user_id=bob`） | bob |
| `POST /api/todos`（都不传） | default_user |
| `GET /api/todos?user_id=alice` | 正常按 alice 过滤 |

同一个资源，`GET` 认 query、`POST` 只认 body。调用方按 REST 惯例把 user_id 放 query 时，数据**静默**写入 `default_user`，返回 `success:true`，无任何警告。

**根因**

`src/api/routes.rs:2163-2166`：

```rust
async fn add_todo(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(mut req): Json<AddTodoRequest>,   // ← 只有 Json 提取器，没有 Query
) -> ApiResponse<TodoItem> {
```

而 `list_todos`（`routes.rs:2297`）用的是 `Query<ListTodosRequest>`。两处不一致。

**影响**

- 待办被静默写进 `default_user` 命名空间：用户 `alice` 之后按 `user_id=alice` 查自己的待办会**查不到**（数据没丢，但归属错了）。
- 与 Agent scope 混用时症状更隐蔽：智能体以为写在自己空间，实际落在用户空间。
- 因为返回 `success:true`，接入方很难发现。

**修复建议**

1. 统一参数来源：要么两边都用 `Json`（推荐，body 里带 `user_id`），要么加 `Query<ScopeQueryParams>` 作为可选覆盖并与 body 合并（body 优先）。
2. 若最终 owner 落到默认值，且请求显式提供了别处的 user_id，应返回 `400` 或在响应里回显 `owner_id` 让调用方能自检。
3. 审计其它 POST 接口是否存在同类不一致（`add_todo` 之外还有多处 `Json` only）。

**验证**

- 三种传参组合各写一条，断言 owner_id 与传入值一致。

---

### X-03 【P1】融合检索只做整串子串匹配，自然语言提问全部落空

**现象（已确证）**

同一实体「李明」，仅改变问法，`POST /api/search/smart` 召回数：

| query | contacts | events | history | todos | 合计 |
|-------|----------|--------|---------|-------|------|
| `李明` | 1 | 0 | 5 | 1 | **7** |
| `李明跳槽` | 0 | 0 | 0 | 1 | 1 |
| `李明 变化` | 0 | 0 | 0 | 0 | **0** |
| `李明怎么样了` | 0 | 0 | 0 | 0 | **0** |
| `李明最近有什么变化` | 0 | 0 | 0 | 0 | **0** |

返回体始终是 `success: true` + 四个空数组，调用方无法区分"这个人没有记忆"与"问法系统不认识"。

**根因**

`src/api/routes.rs:2512-2586`，四路检索全部是整串匹配：

```rust
"contacts" => {
    // 整串直接传给 SQL：name LIKE '%李明最近有什么变化%'
    async_memory.search_contacts(&req.query, safe_limit, ...)
}
"events" => {
    // 整串作为 search_query
    EventFilter::default().with_search_query(req.query.clone())
}
"todos" => {
    t.title.to_lowercase().contains(&query_lower)   // 整串子串
}
"history" => {
    h.content.to_lowercase().contains(&query_lower) // 整串子串
}
```

`query_lower = req.query.to_lowercase()`（`routes.rs:2534`）——**没有分词、没有实体抽取、没有停用词处理**。

联系人的 SQL 侧同样是整串（`src/stores/relational/store.rs:1017`）：

```rust
let like_pattern = format!("%{}%", search_query);
// WHERE (c.name LIKE ? OR a.alias LIKE ?)
```

**影响**

项目的服务对象是智能体，智能体发出的检索请求天然是自然语言（"李明最近怎么样""谁要跳槽"）。当前实现在最主要的使用方式上直接失效，且静默失败。

**修复建议**

1. **最小改动**：检索前对 query 做中文分词（或至少按标点/空格切分 + 抽取人名候选），逐词 OR 匹配；联系人/事件侧按"任一词命中"召回，再按命中数排序。
2. **正解**：`smart_search` 已有向量库可用，history/events 应走语义检索（`semantic` 接口证明向量路是通的），关键词路作为补充做 RRF 或加权融合。
3. 空结果时返回可区分的字段（如 `suggestions` 或 `matched_terms: []`），不要静默返回四个空数组。
4. 顺带修 X-17（多词联系人检索返回 0）——同一个根因。

**验证**

- 上表 5 种问法作为回归用例，要求 `李明最近有什么变化` 至少召回事件 1 条 / 历史 1 条。

---

### X-04 【P1】历史检索不接受 query，limit 也不生效

**现象（已确证）**

```bash
curl -X POST /api/memory/search/history \
  -d '{"query":"王芳喜欢什么颜色","limit":3,"user_id":"me"}'
```

- 返回 **8 条**（全部），`limit=3` 被忽略；
- 返回顺序为**写入时间序**，不是相关度；
- 正确答案（"月底要给王芳过生日，她喜欢蓝色的东西"）在**最后一条**；
- 每条记录里没有 `score` 字段。

补充实测（2026-11-01，`user_id=ordertest`，两条记录分别写在 45 天前与今天）：

```
不带 start_time 查询 → 先返回 2026-09-16（45 天前），后返回 2026-10-31（今天）
```

即默认查询（起点 1970）返回的是**时间正序**的头 100 条——**最旧的 100 条**。对话一旦超过 100 条，最新写入的内容永远进不了结果集。这一点比"limit 无效"更危险：接口看起来一直在正常工作，只是永远停在过去。

**根因**

`src/api/routes.rs:2035-2077`：

```rust
async fn search_history(
    Extension(async_memory): Extension<Arc<AsyncMemory>>,
    Json(req): Json<SearchHistoryRequest>,
) -> ApiResponse<Vec<Conversation>> {
    ...
    match async_memory.search_history((start_time, end_time), req.scope, req.user_id.as_deref()).await {
```

`search_history` 的签名只有**时间区间 + scope + user_id**，**没有 query 参数**。`req.query` 被反序列化后从未使用，`req.limit` 同样未使用。

即：该接口的真实语义是"按时间范围列出对话"，而不是"检索"。

而"列出"的方向也错了，三处叠加：

| 位置 | 代码 | 后果 |
|---|---|---|
| `routes.rs:2052-2056` | 未传 `start_time` → `Utc.timestamp_opt(0,0)` | 范围从 1970 年开始，覆盖全库 |
| `three_library_manager.rs:1257-1261` | 固定 `100`，不接收调用方 limit | 上限写死，`limit` 参数彻底无效 |
| `stores/dictionary/store.rs:1677-1682` | 区间遍历到 100 条即 break，最后 `sort_by_key(created_at)` 升序 | 取的是**最旧**的 100 条，且在内存里再排一次正序 |

**影响**

- 历史检索这一核心能力形同虚设：数据多起来之后，用户只能拿到**最旧的 N 条**，完全不知道哪条相关。
- 对话总量超过 100 条后，**最新记忆永久不可见**——对记忆系统是最致命的一类"静默失效"。
- 与 X-16 叠加：融合检索调用它时又额外限了 30 天窗口，两头都在截断。

**修复建议**

1. 补齐实现：优先走向量检索（嵌入 + HNSW，已有基础设施），按 `score` 降序；关键词路作为召回补充。
2. 把排序方向改为**时间降序**，`limit` / `offset` 下推到检索层生效（现在是内存里 take + 升序）。
3. 默认时间范围改为"最近 N 天"或全量但降序，不要用"1970 起 + 正序"这种组合。
4. 若短期无法做语义检索，至少把接口改名或文档标注为"按时间列出"，避免误导调用方。

**验证**

- `query="王芳喜欢什么颜色"`，`limit=3`：第一条应为含"蓝色"的对话。

---

### X-05 【P1】截止时间：模型幻觉年份未拦截 + 解析器跨句借用无关时间

**现象（已确证，含对照实验）**

原文：`提醒我12月20号前把年度总结交上去。另外月底要给王芳过生日，她喜欢蓝色的东西。`
结果：todo `提交年度总结` 的 `deadline = 2026-10-31T23:59:59`（当天）。**6 小时后即过期**，随后被心跳标记 `auto_invalid` 并归档，用户从默认列表里彻底看不到——而这件事实际还有 50 天才到期。

对照实验（同一服务、同一模型、仅改措辞）：

| 用例 | 原文 | 模型输出 deadline | 最终 deadline | 结论 |
|------|------|------------------|--------------|------|
| t1 | `提醒我12月20号前把年度总结交上去` | `2026-12-20` | **2026-12-20** | 模型给对了，直接用 |
| t2 | `提醒我12月20日前把年度总结交上去` | `2026-12-20` | **2026-12-20** | 同上 |
| t3 | `提醒我12月20号交年度总结`（无"前"） | `2024-12-20`（年份幻觉） | **2026-11-08** | 过期被丢弃 → 本地解析失败 → +7 天 |
| t4 | t1 + `。另外月底要给王芳过生日…` | `2024-12-20`（年份幻觉） | **2026-10-31** | 过期被丢弃 → 本地解析**借用了下一句的"月底"** |

服务端日志实证：

```
[LLM Extract] Raw response: {"todos":[{"title":"提交年度总结","deadline":"2024-12-20T00:00:00Z"}]}
WARN [P1-4] 模型给出的 deadline 不可用，已改用本地时间解析：提交年度总结
```

**根因（两处叠加）**

**① 模型会把年份幻觉成 2024。** `background_queue.rs:946-953` 有防护（早于当前时间即丢弃），这一点是对的。

**② 丢弃后本地解析跨句借用，且"X号"根本解析不了。**

`src/utils/time_parser.rs:822-832` 的扫描正则能匹配 `\d{1,2}号`：

```rust
r"...|\d{1,2}月\d{1,2}日|\d{1,2}号"
```

注意 `\d{1,2}月\d{1,2}日` **只认"日"不认"号"**。所以 `12月20号` 被截成 **`20号`**（月份丢失）。

而 `parse_event_time`（time_parser.rs 内）**没有"X号"分支**——正则里支持、解析器里不支持，属于半支持。

于是 `resolve_deadline_in_text`（`time_parser.rs:788-818`）的流程变成：

```rust
// 句1: "提醒我12月20号前把年度总结交上去"
let exprs = find_time_exprs_with_pos(sentence);   // → [("20号", ..)]
for (expr, end) in &exprs {
    // tail = "前把年度总结交上去" → starts_with("前") ✓ 是截止标记
    if let Some(dt) = parse_event_time(expr, reference_time) {  // "20号" → None
        return Some(end_of_day(dt));
    }
}
// 兜底循环：同样 None
// ↓ 继续遍历 candidates 里的下一句
// 句2: "另外月底要给王芳过生日，她喜欢蓝色的东西"
// exprs → [("月底", ..)]
// parse_event_time("月底") → 2026-10-31  ← 借用了完全无关的时间
```

`time_parser.rs:777-786` 把"紧邻下一句"放进了 candidates：

```rust
Some((_, idx, sentence)) => {
    let mut v = vec![sentence];
    if let Some(next) = sentences.get(idx + 1) {
        v.push(next);        // ← 允许跨句，本意是覆盖被逗号拆开的情况
    }
    v
}
```

设计意图（`time_parser.rs:771` 注释）是"宁可返回 None，也不要给一个错误日期"，但 candidates 里带下一句，实际给了错误日期。

**影响**

这是本次测试危害最大的一条：用户明确说了截止日期，系统记成当天，随后静默归档消失。用户不会收到任何"我理解错了"的提示，只会发现待办不见了。

**修复建议（三处，缺一不可）**

1. **正则补"号"**：`\d{1,2}月\d{1,2}日` → `\d{1,2}月\d{1,2}[日号]`。
2. **解析器补分支**：`parse_event_time` 支持 `X号`（当月 X 号，已过则归下月）与 `X月X号`。当前扫描和解析能力不一致，属于明确缺陷。
3. **禁止跨句借用时间**：只有当"下一句本身也和标题高相关"时才允许借用；否则宁可返回 `None` 走 +7 天。同时把 fallback 从"当天"改为"Null + 提醒用户"，避免制造"当天过期"的假象。
4. 额外建议：模型给的 deadline 与原文时间表达做一致性校验（模型说 2024-12-20、原文写 12月20号 → 年份应纠正为最近未过期年份，而不是直接丢弃）。t3/t4 的 +7 天和当天，都是"丢弃后没兜住"造成的次生错误。

**验证**

- 把 t1–t4 四例作为回归用例，要求全部得到 `2026-12-20`。
- 补单测：`12月20号`、`12月20日`、`20号`、`月底` 四种表达的解析结果。

---

### X-06 【P1】文件分块没有单块体积上限，无空行的大文件必定失败

**现象（已确证）**

上传 4.95 MB 纯文本（内容为无空行的连续中文）：

```
[FileProcessor] Extracted 4950000 chars, chunking...
[FileProcessor] Created 1 chunks, embedding...          ← 495 万字符只切了 1 块
[FileProcessor] Embedding chunk 1/1 (4950000 chars)...
[FileAPI] File ... processing FAILED: Embedding错误: 输入文本过长: 4950000 字符，最大允许 100000 字符
```

上传接口返回 200 + `file_id`，看起来成功。

**根因**

`src/service/file_processor.rs:383-419`：

```rust
fn chunk_text(text: &str) -> Vec<FileChunk> {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();   // ← 只按空行分段
    ...
    for para in paragraphs {
        if current.len() + trimmed.len() > 2000 && !current.is_empty() {
            chunks.push(...);   // ← 只有"累积超过 2000"才切，单段本身不切
        }
        current.push_str(trimmed);
    }
```

切分条件是"累积长度 > 2000 且当前非空"，但**单个段落自身永远不会被切割**。一段 495 万字符、没有空行的文本 → 1 个 chunk。

嵌入器上限 100000 字符（`embed` 处校验），于是必然抛错。

**影响**

- 任何无空行的长文本（导出的聊天记录、日志、CSV、未分段的文章）100% 入库失败。
- 上传接口先返回 200，失败发生在后台，调用方毫不知情（叠加 X-07）。

**修复建议**

1. 在 `chunk_text` 末尾增加硬上限：单 chunk 超过 `MAX_CHUNK_CHARS`（建议 1200–1500，留足嵌入余量）时按字符/句子二次切分。
2. 处理单个超长段落：优先按 `。！？\n` 切句，仍超长再按字符硬切。
3. 在入库前校验 `chunk.content.chars().count() <= 嵌入器上限`，超限时给出明确错误而不是让嵌入器抛。
4. 文件类型与上限的对应关系写进文档（当前 `MAX_FILE_SIZE=20MB` 与嵌入上限 100000 字符之间没有换算关系）。

**验证**

- 上传 5 MB 无空行文本，期望 `status=completed` 且 chunks > 100。

---

### X-07 【P1】文件处理失败后不落库状态、不删临时文件

**现象（已确证）**

X-06 那次失败之后：

```
GET /api/files/24c3d0ba-...  →
  "status": "processing",
  "error_message": null,
  "extracted_text_summary": null

ls data_probe/temp_uploads/  →
  -rw-r--r-- 4950000  24c3d0ba-...        ← 临时文件一直在
```

后台日志明确写了 `processing FAILED`，但数据库里既没有失败状态也没有错误信息。

**根因**

`src/service/file_processor.rs`：

- `:107` `let embedding = self.embedder.embed(&chunk.content).await?;` —— `?` 直接向上传播；
- `:140-142` 状态更新为 `completed` 只在成功路径；
- `:144` `let _ = std::fs::remove_file(original_path);` —— 只在成功路径删除临时文件。

任何 `?` 提前返回都会跳过状态写入与清理，且调用方（`routes.rs:822-831` 的 `tokio::spawn`）没有兜底 handler。

**影响**

- 用户看到文件永远"处理中"，无法判断是慢还是失败，也无法重试（没有失败态可重放）。
- 每次失败泄漏一份原文件（本次 4.95 MB）。

**修复建议**

1. 用 `Result` 包装整段处理逻辑，在 `Err` 分支显式 `update_file_status(file_id, "failed", Some(&err), None)`。
2. 临时文件清理放进"无论如何都执行"的收尾（类似 `finally`），或改成启动时清理超过 N 小时的残留。
3. 文件表增加 `error_message` 写入与对外暴露（字段已存在，只是没写）。

**验证**

- 故意上传一个构造失败的文件，断言 `status=failed`、`error_message` 非空、`temp_uploads/` 为空。

---

### X-08 【P1】待办引用的 contact_id 悬空（实测 50%）

**现象（已确证）**

6 条待办中 4 条带 `contact_ids`，逐条用 `/api/contacts/:id/detail` 校验：

```
悬空  了解李明跳槽计划 -> e131abb9
OK    梳理项目排期     -> 2914f69a   (王芳真实 id)
OK    跳槽到字节       -> 6d74cd2d   (李明真实 id)
悬空  引荐王芳         -> 2769dc38
```

同一个人「李明」在两段对话里拿到了两个不同 id（`e131abb9` 与 `6d74cd2d`），只有后者与 `contact_index` 对得上。

**根因（待确认）**

待办写入时使用的 contact_id 来自抽取阶段的中间产物，与实际联系人落库时的 id 未对齐。可能位置：

- `background_queue.rs:979` 附近 `assigned_to` → contact_id 的匹配逻辑；
- 抽取结果里的 `participants` / `people` 与写入侧 `contact_id` 的映射。

需要在写入链路加日志（抽取侧 id vs 落库侧 id）才能定位到具体分支。

**影响**

- `/api/contacts/:id/detail` 的"关联待办"永远缺失；
- 按联系人过滤待办查不到；
- 联系人画像的 `major_events` 与待办无法互链。

**修复建议**

1. 待办落库前，用**姓名/别名**回查 `contact_index` 得到权威 id，不要直接沿用抽取阶段的 id。
2. 加外键约束或写入后校验：id 不存在则置空并 warn，不要留下悬空引用。
3. 提供一次性修复脚本，扫描 `todo_list.contact_ids` 并重建映射。

**验证**

- 写入 5 段含人物的对话后，遍历所有 `contact_ids`，断言 100% 可解析。

---

### X-09 【P2】限流可被 `X-Forwarded-For` 一行绕过

> 级别说明：默认绑定 `127.0.0.1`、单机单调用方时，不存在恶意伪造 XFF 的主体，本条不构成实际风险；仅在 `--host 0.0.0.0` 或经反向代理暴露时成立。保留 P2 是因为"限流表只增不减"的内存问题在长期运行时依然存在。

**现象（已确证）**

```
真实 IP 连续 70 次  → {200: 59, 429: 11}   限流生效
伪造 XFF 连续 70 次 → {200: 70}             完全绕过
```

**根因**

`src/shouxin-server.rs:148-151`：

```rust
// 从 X-Forwarded-For 头获取客户端 IP（取第一个），未设置时回退为 "unknown"
let client_ip = req.headers().get("X-Forwarded-For") ...
```

直接信任客户端可控的 header，无代理白名单校验。另外 `requests: HashMap<String, (Instant, u32)>`（`:118`、`:721`）只增不减，长期运行内存单调增长。

**影响**

- 单机默认配置下无实际影响；一旦 `--host 0.0.0.0` 或经反向代理暴露，限流形同虚设。
- 与 X-01 组合（对外暴露时）可放大为批量越权扫描。
- 限流表 `requests: HashMap<String, (Instant, u32)>` 只增不减，长期运行内存单调增长——这一条与部署形态无关，始终成立。

**修复建议**

1. 仅在配置了 `trusted_proxies` 时才采信 XFF，否则用 `ConnectInfo<SocketAddr>` 的真实 peer IP。
2. 限流表加 TTL 清理（或换成 `lru` / 令牌桶，项目已依赖 `lru`）。
3. 文档明确：`--host 0.0.0.0` 时必须设置 `SHOUXIN_API_TOKEN`。

**验证**

- 带随机 XFF 连发 100 次，期望仍触发 429。

---

### X-10 【P1】后台抽取队列串行且无界

**现象（已确证）**

连续写入 140 条后，新写入的对话 **10 分钟以上仍是 `pending`**；单条抽取 25–30 秒，队列严格串行。

**根因**

`src/service/background_queue.rs:149`（另有 `:115`、`:182`）：

```rust
let (tx, rx) = mpsc::unbounded_channel();
```

配合 `:297` 的单 worker 串行消费，且每条任务在 worker 内 `spawn_blocking(...).await` 后才取下一条。

**影响**

- 队列长度无上界，每条任务在内存中持有完整 `conversation_text`；
- 生产速度 > 消费速度时，延迟线性堆积，最终"新写的记忆几十分钟查不到"；
- 无背压、无优先级、无丢弃策略。

**修复建议**

1. 换成 `mpsc::channel(N)` 提供背压；满时对 `/api/memory/add` 返回 429 或降级为"只落盘不入队，稍后补偿"。
2. 至少支持并发度 > 1（注意 X-22：模型侧信号量是每实例独立的，需先统一）。
3. 增加队列深度指标到 `/health` 或 `/api/heartbeat/status`，让调用方能看到积压。

**验证**

- 灌入 200 条，观察 `/api/memory/extract_status` 的 pending 数回落曲线与内存曲线。

---

### X-11 【P2】推理超时硬编码 180s，配置项完全不生效

**根因（已确证）**

`src/llm/gguf.rs:624`：

```rust
let timeout_duration = std::time::Duration::from_secs(180);
```

`config.toml:19` 的 `timeout_secs = 1800` 在 `gguf.rs` 全文搜不到读取点。README 与配置注释都明确写了"纯 CPU 时别开太小"，用户改了却无效。

**影响**

长对话 / 大上下文单次推理超过 180s 会被中断并降级为正则抽取，抽取质量骤降。而配置暗示可以调到 1800。

**修复建议**

- 把 `config.llm.timeout_secs` 透传进 `GgufClient`，用配置值替换硬编码，并保留一个上限保护。
- 项目已有 `scripts/check_config.py` 用于发现死配置，把这个 case 补进检查规则。

---

### X-12 【P2】向量库容量硬编码 10000，无告警无扩容

**根因（已确证）**

`src/stores/vector/hnsw_store.rs` 三处硬编码：

```rust
:106   let max_elements = 10000;    // new()
:143   let max_elements = 10000;    // in_memory()
:491   let max_elements = 10000;    // clear_materials() 重建后仍写死
```

`VectorIndexConfig`（`types.rs`）没有该字段，无法配置；`load_existing`（`:166-206`）无上限地把磁盘记录 `insert_slice` 回填。

**影响**

记忆系统的数据是持续增长的。超过 1 万条后 `hnsw_rs` 的行为（panic 或静默丢点）无保护，且没有任何预警。

**修复建议**

1. 加 `max_elements` 配置项，按数据量估算（如 10 万）预分配。
2. 插入前检查 `count_loaded()`，接近上限时 warn，并在 `clear_materials` 之外的路径支持扩容重建。
3. `load_existing` 加载超过 N 条时打印进度，避免启动长时间无输出。

---

### X-13 【P2】事件抽取漏关键事件，且画像与事件列表自相矛盾

**现象（已确证）**

输入 5 段对话，事件库只得到 5 条：

```
事件: 公司数据团队了解 / 公司准备上市 / 王芳今天找我说项目延期了 / 二胎出生 / 喝咖啡
```

- 最有价值的"**李明准备明年三月跳槽去字节**"和"**李明跳槽定了，下周去上海入职**"**全部缺失**；
- 却抽出了"喝咖啡"这类无信息量事件；
- 而联系人的 `objective_evaluation` 却准确写了"有明确的职业规划（跳槽至上海/字节）"——**画像读到了原文，事件表没落下**。

**根因（待确认）**

抽取器提示词/解析对"事件"的定义偏向"有明确动作 + 时间"的表述，"准备跳槽"这类**计划类事件**可能被判为 `todo` 或直接丢弃。需对比 `memory_extractor/extractor.rs` 的提示词与解析分支。

**影响**

- 语义检索问"谁要跳槽去哪"召回不到（实测 top5 全是含"公司"二字的事件）；
- 画像与事件两套数据不一致，用户点开事件列表找不到画像里的依据。

**修复建议**

1. 提示词里明确区分 `event`（已发生/已确定）与 `plan`（计划中但已明确），两者都应入事件库。
2. 对"喝咖啡"这类低信息量事件做过滤（可复用现有 `importance` 评分）。
3. 画像生成后做一致性校验：`objective_evaluation` 里提到的事实，应能关联到至少一条 `major_events`。

---

### X-14 【P2】待办语义边界错误：已发生的事、他人的事都进了待办

**现象（已确证）**

5 段对话产出 6 条待办，其中只有 2 条是真正的"我承诺要做的事"：

| 待办 | 判定 | 说明 |
|------|------|------|
| 梳理项目排期 | ✅ 正确 | "我答应下周一帮她梳理排期" |
| 引荐王芳 | ✅ 正确 | "让我帮他引荐一下王芳" |
| 与张伟吃饭 | ❌ | 昨晚已经吃了，是已发生事件 |
| 跳槽到字节 | ❌ | 是李明的事，不是"我"的待办 |
| 了解李明跳槽计划 | ❌ | 原文没有这个承诺 |
| 提交年度总结 | ⚠️ | 是待办但日期错（见 X-05） |

**根因（待确认）**

抽取提示词对 `todos` 的判定标准过宽，未区分：主体（我 vs 他人）、时态（将做 vs 已做）、是否承诺。

**影响**

待办列表被污染。作为一个"主动提醒"系统，噪音待办的代价很高——用户会逐渐忽略提醒。

**修复建议**

1. 提示词加硬约束：`todos` 只收"主体是第一人称 / 时态是将来 / 有明确动作"的三元组，并要求模型对每条给出 `subject` 与 `modality`。
2. 落库前做规则校验：标题命中"已/了/昨天/刚才"等过去时标记则拒收。
3. 引入置信度，低置信待办进"待确认"而不是直接进列表。

---

### X-15 【P2】融合检索先截断后过滤，匹配项排后面就被丢掉

**根因（已确证）**

`src/api/routes.rs:2554-2563`：

```rust
let filter = crate::api::TodoFilter::new().with_limit(safe_limit as i64);  // ← 先取前 N 条
let todos = async_memory.list_todos(filter, ...).await {
    let filtered = todos.into_iter()
        .filter(|t| t.title.contains(&query_lower) ...)                     // ← 再在 N 条里过滤
```

若目标待办排在第 N+1 位，即使完全匹配也返回空。正确顺序应是"先全量过滤，再截断"。

**修复建议**

- 把关键词条件下推到 SQL（`title LIKE ?`），或先取较大候选集再过滤再 take。

---

### X-16 【P2】融合检索的历史硬编码 30 天窗口

**复核结论（2026-11-01 补充：先验证"冷表是否可用"）**

这条曾被质疑：只有"冷表查不到、只能查热表"时问题才成立。我按这个前提逐层验证，结论是——**前提不成立，但问题仍然成立**，根因要改写：

| 验证项 | 结论 | 证据 |
|---|---|---|
| 事件有没有冷热表设计 | 有，但**不是独立冷表**，是 `event_index.is_archived` 逻辑分区 | `store.rs:3152-3196 cleanup_expired_events`：长期事件 30 天、中期事件 7 天自动归档；`store.rs:3205 archive_oldest_hot_event`：热表超 200/100 条时归档最旧 |
| 冷事件能否被检索 | **能** | 事件接口默认 `include_archived=true`（`routes.rs:1952-1955`，注释写明"记忆系统的语义是全量可查"）；smart 的 events 分支显式 `.with_include_archived(true)`（`routes.rs:2544-2548`）；SQL 仅在 `!include_archived` 时才追加 `is_archived = 0`（`store.rs:1093`） |
| 冷事件实测 | **能召回** | 把一条事件置 `is_archived=1` 且回拨到 120 天前，`/api/search/smart` 仍返回 1 条 |
| 联系人有没有冷热 | 有，且**热表不足自动补冷表** | `store.rs:968-990 search_contacts_with_owner`：先查热表，不够再查冷表 |
| 对话原文有没有冷热 | **没有** | 字典库只有 `conversations` + `conversation_ts_index` 两棵树，无归档标记、无 retention/cleanup 函数；`store.rs:1639 get_conversation_by_time_range` 可从任意时间点区间扫描 |

所以"冷表不可用"被证伪——事件和联系人的冷数据都能查到。**但 history 这一路压根不在冷热体系里**：对话原文永久保留、任意时间范围可查，而 `smart_search` 却按"只看热窗口"的思路硬编码了 30 天。这不是冷热设计，是少了一个分支。

**现象（已确证）**

同一进程、同一份数据：

```
写入 timestamp = 90 天前的对话（user=histtest）
POST /api/search/smart         {"query":"青鸾计划","types":["history"]}          → 0 条
POST /api/memory/search/history {"query":"青鸾计划","start_time":"120 天前"}     → 1 条（created_at=2026-08-02）
```

数据在、能查，只是融合入口看不到。

再加一组对照，排除"history 分支整体坏掉"：

```
同分支查 30 天内的记录（query=ORDER-0） → 1 条
同分支查 90 天前的记录（query=青鸾计划）→ 0 条
```

分支本身可用，拦截点就是那个 30 天窗口。

**根因**

`src/api/routes.rs:2568-2571`：

```rust
let time_range = (
    Utc::now().checked_sub_signed(chrono::Duration::days(30)).unwrap(),
    Utc::now(),
);
```

**影响**

- 30 天前的对话原文在智能检索入口**永久不可见**，且返回 `success:true` + 空数组，调用方无法区分"没记过"和"被窗口截了"。
- 对长期记忆系统，这是与产品定位直接冲突的静默失效。
- 与 X-04 叠加会放大：即使放开窗口，`search_history` 仍只取**最旧 100 条**并升序，最新记忆还是可能进不来。

**修复建议**

不建议简单把 30 改成 3650，直接复用项目已有的"热→冷两段式"（`search_contacts_with_owner` 就是这么写的）：

1. 先在最近 30 天窗口内检索；命中数达到 `limit` 直接返回（保留热路径的速度优势）。
2. 不足时按时间窗向前回溯（30 天 → 1 年 → 全量），逐段补齐剩余名额。
3. 或在 `SmartSearchRequest` 增加可选 `start_time`，由调用方决定回溯深度，默认全量。
4. 响应加 `truncated` / `searched_range` 字段，让调用方知道"这次只搜了多远"。
5. 与 X-04 一起改：先按时间降序取数，再做相关性排序。

**验证**

- 90 天前的对话：smart 应返回 1 条；
- 若窗口配成 7 天：smart 返回 0 条且 `truncated=true`；
- 回归：冷事件仍能被 smart 召回（不能被这次改动破坏）。

---

### X-17 【P2】联系人检索不支持多词

**现象（已确证）**

```
query="李明"      → 1 条
query="王芳"      → 1 条
query="李明 王芳" → 0 条
```

**根因**

`src/stores/relational/store.rs:1017`：`format!("%{}%", search_query)` —— 整串 LIKE。

与 X-03 同源。**修复时与 X-03 一并处理**：query 分词后逐词匹配。

---

### X-18 【P2】同步抽取路径的 dedup_key 用整段对话（潜伏）

**根因（已确证，当前主链路未触发）**

`src/memory/three_library_manager.rs:611-618`：

```rust
// V1: 使用对话文本前200字符作为去重键
let dedup_key: String = conversation.chars().take(200).collect();
let existing = self.relational_store.search_events_by_summary(&dedup_key, ...).await?;
if !existing.is_empty() { continue; }   // ← 第 2 个事件起全部被跳过
```

写入时 `summary` 也存成 `dedup_key`（`:745` 附近），于是同一段对话的第 2..N 个事件全部命中"已存在"被丢弃。

对比正确的异步路径 `src/service/background_queue.rs:582`：

```rust
let dedup_key: String = event_desc.chars().take(200).collect();   // ← 用事件自身描述，正确
```

**影响**

当前 `/api/memory/add` 走异步队列，不触发。但同步路径仍被 `async_memory.rs:182` 的 `add()` 使用，一旦被内部 API 或未来重构调用，会静默丢失多事件对话的全部后续事件。

**修复建议**

- 同步路径改用 `event_desc` 作为 dedup_key，与异步路径对齐；抽公共函数避免再次分叉。

---

### X-19 【P2】启动日志被 llama.cpp 刷屏

**现象（已确证）**

单次启动输出约 **11 万字符**，其中 5 万+ 是 `repack tensor blk.N.xxx.weight` / `llama_kv_cache: layer N: filtered` 这类底层日志。README §5 说"启动成功的关键日志（三条都在才算真正就绪）"，实际要在几千行里找。

**影响**

运维体验差；日志文件迅速膨胀（`max_file_size_mb=10` 会被瞬间轮转）；排障时容易漏掉真正的 WARN/ERROR。

**修复建议**

- 构建/运行时通过 llama.cpp 的日志回调把 `GGML_LOG_LEVEL_INFO` 降到 WARN，或把 `llama_log_set` 重定向到独立文件。
- 关键就绪信息（模型加载完成 / 嵌入器就绪 / 监听地址）前置并加醒目分隔。

---

### X-20 【P2】错误响应格式不统一

**现象（已确证）**

| 场景 | 状态码 | 响应体 |
|------|--------|--------|
| JSON 类型错误（`conversation` 传数组） | 422 | **纯文本** `Failed to deserialize the JSON body...` |
| 业务校验失败（空 conversation） | 400 | JSON `{"success":false,"error":"对话内容为空"}` |
| 缺 user_id | 200 | 静默成功 |

**影响**

客户端若统一按 JSON 解析错误体，遇到 422 会二次抛错，拿不到真实原因。

**修复建议**

- 为 `JsonRejection` 加自定义 `FromRequest`，把各类 rejection 统一包装成 `ApiResponse` 格式。

---

### X-21 【P2】联系人画像藏在 `background` 的 JSON 字符串里

**现象（已确证）**

```json
"background": "{\"contact_id\":\"...\",\"personal_info\":{...},\"major_events\":[...]}"
```

是**字符串**不是对象，调用方需要二次 `json.loads`。文档与接口没有说明。

**修复建议**

- 对外输出时反序列化成对象（或提供 `?expand=profile`）。
- 顺带修正画像字段质量：实测 `known_occupation="跳槽/准备入职"`（把动作当职业）、`known_interests=["喝咖啡","了解数据团队"]`（把活动/事件当兴趣）。

---

### X-22 【P2】推理信号量每实例独立，超时后 worker 不会被中止

**根因（已确证）**

`src/llm/gguf.rs:213`：

```rust
inference_semaphore: Arc::new(Semaphore::new(1)),
```

semaphore 是**实例级**的。而主链路与心跳各自 `GgufClient::new`（`factory.rs` 两处），且 `config.toml:22` 与 `:53` 指向**同一个模型文件** —— 两份句柄、两套信号量，互不感知。

另外 `:625` 的 `tokio::time::timeout` 超时后，`spawn_blocking` 的 JoinHandle 被 drop **不会中止**已开始的阻塞推理；此时 permit 已随函数返回释放，下一次推理可能与旧任务并发跑同一个模型。

**影响**

- 内存翻倍（实测进程 RSS 7.5 GB）；
- CPU 争抢，抽取和心跳互相拖慢；
- 极端情况下同一模型被并发推理，KV cache 双份，可能 OOM 或触发 `unsafe impl Send/Sync` 的未定义行为。

**修复建议**

1. 同 (model_path, context_size) 的 client 复用同一 `Arc<GgufModelHandle>` 与同一 semaphore（用 `OnceCell` 做进程级单例）。
2. 超时路径要么真的 abort（改为可取消的推理循环，用 `cancel_flag` 检查点密度提高），要么在 permit 释放前等待 worker 真正结束。
3. 心跳与主链路共享同一个限流器，避免"后台任务把前台饿死"。

---

## 五、系统性根因（比单条缺陷更重要）

把上面 22 条按根因归类，能看出三个系统性短板：

**1. 检索层没有"语义"这一层**

X-03、X-04、X-15、X-17 四条共享同一个根因：**全链路整串子串匹配，没有分词/实体/向量融合**。

讽刺的是向量库基础设施是完备的（`/api/memory/search/semantic` 实测可用，能返回 0.858 相关度的事件）。但**主力入口 `/api/search/smart` 完全没用它**，`search_history` 干脆没有 query 参数。

X-16 属于另一类，别混在一起修：它不是"不会语义"，而是**先在时间维度上把候选砍掉了**。而且砍法与项目自己的冷热设计相矛盾——冷事件都默认回捞（`include_archived=true`），历史原文却硬编码 30 天。复核已确认冷表可用，所以这条不能拿"冷热分离"当解释。

对"服务对象是智能体"的系统，这是定位级偏差：智能体不会用关键词检索，它会问"李明最近怎么样"。

**建议**：把 `smart_search` 改成"向量召回 + 关键词召回 + 融合排序"，history/events 都走这条；联系人检索的 SQL 侧加分词。这是本次修复的最高性价比项。X-16 单独按"热→冷两段式"改，别等语义检索上线。

**2. 时间语义缺乏"不可确定就不猜"的纪律**

X-05 暴露的不是单点 bug，而是策略问题：解析器在句1失败后会去句2借时间，模型幻觉日期被丢弃后兜不住，最后落到"当天"——三个环节都在"猜"，而且猜错没有任何提示。

**建议**：确立一条规则——**任何无法高置信解析的时间，宁可留空 + 标记 `deadline_unresolved`，也不要填一个可能立即过期的值**。同时在响应里回显 `deadline_source`（model / local / default），让调用方能判断可信度。

**3. 写链路的"ID 与状态"缺少一致性收口**

X-07（文件状态不落库）、X-08（contact_id 悬空）、X-13（画像与事件不一致）本质相同：**跨子系统的写入没有事务边界或后置校验**。

**建议**：在写链路末尾加一个"一致性校验 + 修复"步骤：文件状态必须有终态、引用 id 必须可解析、画像提到的事实必须能关联到事件。校验失败记 warn 而不是静默。

---

## 六、建议修复顺序

按"危害 × 修复成本"排序：

| 顺序 | 问题 | 理由 |
|------|------|------|
| 1 | X-05 | 直接导致用户错过截止日期且数据静默消失，修复点明确（正则 + 禁止跨句 + fallback 策略） |
| 2 | X-04 | 对话超过 100 条后最新记忆永久不可见，是最隐蔽的"记忆失效"；改动小（排序方向 + limit 下推） |
| 3 | X-03 / X-17 | 决定"能不能用"的核心能力，且向量基础设施已具备 |
| 4 | X-16 | 复用联系人检索的"热→冷两段式"，与 X-04 一起改 |
| 5 | X-02 | 一个 Query 提取器的事，但不修则数据静默落错 owner |
| 6 | X-06 / X-07 | 文件入库整条链路目前不可用，两处一起改 |
| 7 | X-08 | 数据一致性，需先加日志定位 |
| 8 | X-10 / X-22 | 并发与资源，建议一起做（信号量统一后再提并发度） |
| 9 | X-09 | 仅在服务对外暴露时有意义，随部署形态决定 |
| 10 | 其余 P2 | 排期处理 |

> X-01（身份校验）**不在排期内**：本地单机威胁模型下不构成缺陷。若将来对外暴露，按 §4 X-01 的方案修，优先级直接提到最前。

---

## 七、附录

### A. 环境复现命令

```bash
cd /e/ShouXin
source scripts/dev_env.sh
./target/release/shouxin-server.exe --data-dir ./data_probe
```

### B. 第二轮体验测试的完整语料

```python
lines = [
 "今天下午和李明在公司楼下喝咖啡，他说他老婆刚生了二胎，是个女儿。顺便聊到他准备明年三月跳槽去字节。",
 "王芳今天找我说项目延期了，她压力很大。我答应下周一帮她梳理一下排期。",
 "晚上和张伟吃饭，他问我认不认识做数据合规的律师，他公司最近在准备上市。",
 "李明发微信说他跳槽的事定了，下周就去上海入职。让我帮他引荐一下王芳，说想了解她们公司的数据团队。",
 "提醒我12月20号前把年度总结交上去。另外月底要给王芳过生日，她喜欢蓝色的东西。",
]
```

### C. X-05 对照实验（最小复现）

```bash
# t1: 应得 2026-12-20
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我12月20号前把年度总结交上去","user_id":"t1"}'

# t4: 只多了一句"另外月底要给王芳过生日…"，实际得到 2026-10-31（今天）
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"提醒我12月20号前把年度总结交上去。另外月底要给王芳过生日，她喜欢蓝色的东西。","user_id":"t4"}'

# 等待约 30s 后对比
curl "localhost:3000/api/todos?user_id=t1"
curl "localhost:3000/api/todos?user_id=t4"
```

### D. 已发现的"文档与实现不符"清单

修复时建议一并订正文档或实现（二者取其一）：

| 位置 | 文档说法 | 实际 |
|------|---------|------|
| README「多用户隔离」 | 越权访问不泄露存在性 | 单机不做鉴权，返回 200 + 完整数据；措辞应改成"单机内不做鉴权"（X-01 已按威胁模型排除，此处仅订正文档） |
| README「多用户隔离」 | 每个 owner 独立命名空间 | query 传 user_id 静默落 default_user（X-02） |
| `config.toml:19` | `timeout_secs = 1800` 可调 | 硬编码 180s（X-11） |
| README API 表 | `POST /api/memory/search/history` 历史对话检索 | 无 query 参数，实为按时间列出（X-04） |
| README「文件入库」 | 上传 → 提取 → 切片 → 可检索 | 无空行大文件必失败（X-06） |
| `routes.rs:701` | `MAX_FILE_SIZE = 20MB` | 实际上限取决于嵌入器 100000 字符，二者无换算（X-06） |
| `time_parser.rs:844` 注释 | 支持"某号" | 扫描正则认"号"，解析器不认（X-05） |
| `time_parser.rs:771` 注释 | 宁可返回 None 也不给错误日期 | candidates 含下一句，实际给了错误日期（X-05） |

### E. 冷热表验证实验（回答"是不是只能查热表"）

目的：确认 `is_archived = 1` 的数据是否真的检索不到。**结论：事件与联系人的冷数据都能查到；history 根本没有冷热分区。**

代码路径：

| 环节 | 位置 | 行为 |
|---|---|---|
| 事件按时归档 | `store.rs:3152-3196` | 长期事件（>0.7）30 天、中期事件（0.4–0.7）7 天自动置 `is_archived=1` |
| 事件按量归档 | `store.rs:3205 archive_oldest_hot_event` | 热表超 200/100 条时归档最旧 |
| 事件检索 | `store.rs:1093` | 仅当 `include_archived=false` 才追加 `is_archived = 0` |
| 事件接口默认 | `routes.rs:1954` | `include_archived = req.include_archived.unwrap_or(!req.hot_only)` → 默认 true |
| 融合检索 | `routes.rs:2547` | 显式 `.with_include_archived(true)` |
| 联系人检索 | `store.rs:968-990` | 先热表，不足自动补冷表 |
| 历史原文 | `dictionary/store.rs` | 只有 `conversations` + `conversation_ts_index`，无归档标记、无清理函数 |

实测（`data_probe4`，2026-11-01）：

```bash
# 1) 写一条事件，等抽取完成（约 20s）
curl -X POST localhost:3000/api/memory/add -H 'Content-Type: application/json' \
  -d '{"conversation":"上周我和陈海涛签了航天项目的合同，合同编号是 QX-8899。","user_id":"coldtest"}'

# 2) 直接改库：标记归档 + 时间回拨到 120 天前
#    UPDATE event_index SET is_archived=1, archived_at=?, created_at=?, time_range_start=?, time_range_end=? WHERE event_id=?

# 3) 仍能被融合检索召回
curl -X POST localhost:3000/api/search/smart -H 'Content-Type: application/json' \
  -d '{"query":"航天项目合同","types":["events"],"user_id":"coldtest"}'
# 结果：归档前 1 条 → 归档并回拨 120 天后仍 1 条
```

同法验证 history：

```bash
# 写入时带 "timestamp": "90 天前"
# /api/search/smart          {"query":"青鸾计划","types":["history"]}        → 0 条
# /api/memory/search/history {"query":"青鸾计划","start_time":"120 天前"}    → 1 条
```

**结论**：冷表本身是可用的，X-16 不能用"冷热分离的预期行为"解释；它对应的是 history 缺少回溯分支，外加 X-04 的"最旧 100 条 + 升序"。

---

*报告结束。除已排除的 X-01 外，所有 P1 条目均可在 30 分钟内用附录命令复现。*
