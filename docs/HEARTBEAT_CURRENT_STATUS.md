# ShouXin v0.5.1 心跳模块当前状态

## 版本: v0.5.1

## 模块状态: ✅ 正常运行

心跳模块在 v0.5.1 中已完成拆分架构升级，所有核心功能正常。

## 功能清单

| 功能 | 状态 | 说明 |
|------|------|------|
| 定时调度 | ✅ | 基于 SchedulerConfig 配置，带超时控制 |
| 拆分架构 | ✅ | Phase 1 维护阶段 + Phase 2 LLM 阶段解耦 |
| 维护阶段 | ✅ | 清理过期联系人、事件去重、联系人合并、歧义检测 |
| LLM 分析 | ✅ | 画像分析、问题生成（仅 Phase 2） |
| maintenance_only | ✅ | 支持仅执行维护的轻量心跳 |
| 问题生成 | ✅ | 检测信息缺口并生成结构化问题 |
| 问题持久化 | ✅ | SQLite + 文件双存储 |
| 手动触发 | ✅ | POST /api/heartbeat/trigger（支持 maintenance_only 参数） |
| 状态查询 | ✅ | GET /api/heartbeat/status（含新增统计字段） |
| 问题回答 | ✅ | POST /api/heartbeat/answer |
| 超时控制 | ✅ | 每次心跳设置执行超时 |

## HeartbeatReport 新增字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `maintenance_only` | `bool` | 是否仅执行维护阶段 |
| `events_merged` | `u32` | 累计合并事件数 |
| `contacts_merged` | `u32` | 累计合并联系人数 |
| `ambiguities_detected` | `u32` | 累计检测歧义数 |
| `profiles_updated` | `u32` | 累计更新画像数 |

## 配置参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `enabled` | `true` | 启用心跳 |
| `base_interval_secs` | `360` | 基础扫描间隔（秒） |
| `min_interval_secs` | `300` | 最小间隔 |
| `max_interval_secs` | `600` | 最大间隔 |
| `adaptive` | `false` | 自适应调整（v0.5.1 关闭用于测试） |
| `idle_threshold_secs` | `30` | 空闲阈值 |

## 存储

- 问题池数据: `.heartbeat_data/`
- 问题状态: SQLite `questions` 表
- 关联: 通过 `RelationalStore` 访问用户数据

## v0.5.1 变更

- **拆分架构**: HeartbeatExecutor 分为 Phase 1（维护）和 Phase 2（LLM），两阶段解耦
- **maintenance_only**: 支持仅执行维护阶段的轻量心跳，跳过 LLM 调用
- **HeartbeatReport 扩展**: 新增 `maintenance_only`、`events_merged`、`contacts_merged`、`ambiguities_detected`、`profiles_updated`
- **HeartbeatAction 枚举**: 新增 `AskUser`、`UpdateContactSummary`、`MarkEventCompleted`、`MergeEntities`、`CleanupPendingEvents`、`UpdateTodoStatus`、`ResolveAmbiguity`、`NoOp`
- **ProcessingState**: 新增 `heartbeat_count` 和 `consecutive_skips` 追踪
- **超时控制**: 每次心跳设置执行超时，防止 LLM 卡死
- **配置调整**: 测试环境使用较短间隔（60s），关闭自适应

## 验证方法

```bash
# 检查心跳状态（含新增统计字段）
curl http://127.0.0.1:3000/api/heartbeat/status

# 手动触发完整心跳
curl -X POST http://127.0.0.1:3000/api/heartbeat/trigger

# 手动触发仅维护阶段心跳
curl -X POST http://127.0.0.1:3000/api/heartbeat/trigger \
  -H "Content-Type: application/json" \
  -d '{"maintenance_only": true}'

# 查看待处理问题
curl http://127.0.0.1:3000/api/heartbeat/pending_questions

# 回答问题
curl -X POST http://127.0.0.1:3000/api/heartbeat/answer \
  -H "Content-Type: application/json" \
  -d '{"question_id": "uuid", "answer": "用户回答内容"}'
```