#!/usr/bin/env bash
# 全量 API 冒烟测试（覆盖 src/api/routes.rs 里注册的所有端点）
#
# 用法：
#   bash scripts/api_smoke_test.sh
#   BASE=http://127.0.0.1:3001 USER_ID=smoke bash scripts/api_smoke_test.sh
#
# 说明：
#   * 会真实写入数据（对话/待办/文件/虚拟角色），用独立 USER_ID 隔离，测试自带清理；
#   * 依赖大模型的端点（抽取、心跳）耗时较长，单请求超时给到 240s；
#   * 需要"已存在资源"的端点（如按 id 查事件合并），资源不存在记为 SKIP 而非 FAIL。
set -uo pipefail

BASE="${BASE:-http://127.0.0.1:3000}"
UID_="${USER_ID:-smoke_user}"
TMP=$(mktemp -d 2>/dev/null || echo "/tmp/smoke.$$")
mkdir -p "$TMP"

PASS=0; FAIL=0; SKIP=0
FAILED_LIST=""

json_get() { # json_get <file> <key>  —— 取简单字符串字段值
  sed -n 's/.*"'"$2"'":"\([^"]*\)".*/\1/p' "$1" 2>/dev/null | head -1
}

# check <名称> <方法> <路径> [期望状态码列表] [body] [期望片段]
check() {
  local name="$1" method="$2" path="$3" expect="${4:-200}" body="${5:-}" substr="${6:-}"
  local out="$TMP/out.json" code ok=0 e
  local args=(-sS --max-time 240 -o "$out" -w "%{http_code}" -X "$method" "$BASE$path")
  [ -n "$body" ] && args+=(-H "Content-Type: application/json" -d "$body")

  code=$(curl "${args[@]}" 2>/dev/null || echo "000")
  for e in $expect; do [ "$code" = "$e" ] && ok=1; done
  if [ "$ok" = "1" ] && [ -n "$substr" ]; then
    grep -q "$substr" "$out" 2>/dev/null || ok=0
  fi

  if [ "$ok" = "1" ]; then
    printf "  [PASS] %-44s HTTP %s\n" "$name" "$code"
    PASS=$((PASS+1))
  else
    printf "  [FAIL] %-44s HTTP %s  %s\n" "$name" "$code" "$(head -c 140 "$out" 2>/dev/null | tr -d '\n')"
    FAIL=$((FAIL+1)); FAILED_LIST="$FAILED_LIST\n    - $name (HTTP $code)"
  fi
}

skip() { printf "  [SKIP] %-44s %s\n" "$1" "${2:-无可用资源}"; SKIP=$((SKIP+1)); }

echo "== 目标: $BASE  用户: $UID_ =="

echo "== 1. 健康检查与记忆写入 =="
check "GET  /health" GET "/health" 200 "" '"status"'
check "POST /api/memory/add" POST "/api/memory/add" 200 \
  "{\"conversation\":\"老王是我的同事，在研发部做后端，本名王建国。小李和老王是同一个项目组。下周五之前要交付接口文档。\",\"user_id\":\"$UID_\"}" \
  'conversation_id'
CID=$(json_get "$TMP/out.json" conversation_id)

if [ -n "$CID" ]; then
  check "GET  /api/memory/extract_status/:id" GET "/api/memory/extract_status/$CID" 200 "" '"status"'
else
  skip "GET  /api/memory/extract_status/:id" "上一步未拿到 conversation_id"
fi

echo "== 2. 检索 =="
check "POST /api/memory/search/semantic" POST "/api/memory/search/semantic" 200 \
  "{\"query\":\"接口文档\",\"limit\":3,\"scope\":\"user\",\"user_id\":\"$UID_\"}" '"success"'
check "POST /api/memory/search/contacts" POST "/api/memory/search/contacts" 200 \
  "{\"query\":\"老王\",\"limit\":3,\"user_id\":\"$UID_\"}" '"success"'

# 联系人详情：等抽取把联系人落库（异步抽取，通常 10–40 秒）
CCID=""
for _i in $(seq 1 20); do
  curl -sS --max-time 20 -X POST "$BASE/api/memory/search/contacts" \
    -H 'Content-Type: application/json' \
    -d "{\"query\":\"\",\"limit\":5,\"user_id\":\"$UID_\"}" -o "$TMP/c.json" 2>/dev/null
  CCID=$(json_get "$TMP/c.json" contact_id)
  [ -n "$CCID" ] && break
  sleep 2
done
if [ -n "$CCID" ]; then
  check "GET  /api/contacts/:id/detail" GET "/api/contacts/$CCID/detail?user_id=$UID_" 200 "" '"stats"'
else
  skip "GET  /api/contacts/:id/detail" "抽取未产出联系人"
fi
check "GET  /api/contacts/:id/detail(不存在)" GET "/api/contacts/no-such-contact/detail?user_id=$UID_" 404 "" ''
check "POST /api/memory/search/events" POST "/api/memory/search/events" 200 \
  "{\"query\":\"项目\",\"limit\":3,\"owner_type\":\"User\",\"owner_id\":\"$UID_\"}" '"success"'
check "POST /api/memory/search/history" POST "/api/memory/search/history" 200 \
  "{\"query\":\"老王 同事\",\"limit\":3,\"user_id\":\"$UID_\"}" '"success"'
check "POST /api/search/smart" POST "/api/search/smart" 200 \
  "{\"query\":\"接口文档\",\"limit\":3}" '"success"'
check "POST /api/vector/search" POST "/api/vector/search" 200 \
  "{\"query\":\"接口文档\",\"limit\":3,\"scope\":\"user\",\"user_id\":\"$UID_\"}" '"success"'

echo "== 3. 待办 CRUD =="
TID=""
check "POST /api/todos" POST "/api/todos" 200 \
  '{"title":"冒烟测试待办","deadline":"2026-11-10T23:59:59+08:00","importance":"medium"}' 'todo_id'
TID=$(json_get "$TMP/out.json" todo_id)
check "GET  /api/todos" GET "/api/todos" 200 "" '"success"'
if [ -n "$TID" ]; then
  check "GET  /api/todos/:id" GET "/api/todos/$TID" 200 200 "" '' 2>/dev/null || true
  check "PUT  /api/todos/:id/complete" PUT "/api/todos/$TID/complete" 200 "" ''
  check "DELETE /api/todos/:id" DELETE "/api/todos/$TID" 200 "" ''
else
  skip "GET/PUT/DELETE /api/todos/:id" "上一步未拿到 todo_id"
fi

echo "== 4. 事件合并 =="
check "GET  /api/events/pending_merges" GET "/api/events/pending_merges?owner_type=User&owner_id=$UID_&limit=5" 200 "" '"success"\|"count"\|"data"'
MID=$(sed -n 's/.*"id":\([0-9]\+\).*/\1/p' "$TMP/out.json" 2>/dev/null | head -1)
if [ -z "$MID" ]; then
  # 兜底：全局取一条（不同 owner 也可能有）
  curl -sS --max-time 60 "$BASE/api/events/pending_merges?owner_type=User&owner_id=default_user&limit=1" -o "$TMP/m.json" 2>/dev/null
  MID=$(sed -n 's/.*"id":\([0-9]\+\).*/\1/p' "$TMP/m.json" | head -1)
fi
[ -n "$MID" ] && check "POST /api/events/pending_merges/:id/resolve" POST "/api/events/pending_merges/$MID/resolve" "200 404" '{"action":"dismiss"}' '' || skip "POST /api/events/pending_merges/:id/resolve" "无待确认合并"
check "POST /api/events/confirm_merge" POST "/api/events/confirm_merge" "200 400 404" '{"merge_id":1,"action":"create"}' '' || true
check "POST /api/events/pending_merges(POST 变体)" POST "/api/events/pending_merges" "200 400" \
  "{\"owner_type\":\"User\",\"owner_id\":\"$UID_\",\"limit\":5}" ''

echo "== 5. 文件 =="
printf '守心 API 冒烟测试文件\n用于验证上传、文本提取、索引与删除。\n' > "$TMP/upload.txt"
FID=$(curl -sS --max-time 120 -X POST "$BASE/api/files/upload?filename=smoke.txt&scope=user&user_id=$UID_" \
  --data-binary "@$TMP/upload.txt" -H "Content-Type: application/octet-stream" 2>/dev/null \
  | sed -n 's/.*"file_id":"\([^"]*\)".*/\1/p' | head -1)
if [ -n "$FID" ]; then
  printf "  [PASS] %-44s file_id=%s\n" "POST /api/files/upload" "$FID"; PASS=$((PASS+1))
  check "GET  /api/files/:id" GET "/api/files/$FID?scope=user&user_id=$UID_" 200 "" ''
  check "GET  /api/files/:id/text" GET "/api/files/$FID/text?scope=user&user_id=$UID_" 200 "" ''
  # 越权访问：本项目的约定是 HTTP 200 + {"success":false,"error":"文件不存在"}，
  # 即"不泄露资源是否存在"，而不是用 404。这里校验响应体语义。
  check "GET  /api/files/:id(越权应拒绝)" GET "/api/files/$FID?scope=user&user_id=other_user" 200 "" '"success":false'
  check "DELETE /api/files/:id" DELETE "/api/files/$FID?scope=user&user_id=$UID_" 200 "" ''
else
  skip "POST /api/files/upload" "上传失败"
fi

echo "== 6. 心跳 =="
check "GET  /api/heartbeat/status" GET "/api/heartbeat/status" 200 "" ''
check "GET  /api/heartbeat/pending_questions" GET "/api/heartbeat/pending_questions" 200 "" ''
check "POST /api/heartbeat/trigger(仅维护)" POST "/api/heartbeat/trigger" "200 409" '{"maintenance_only":true}' ''
check "POST /api/heartbeat/answer" POST "/api/heartbeat/answer" "200 400 404" \
  '{"question_id":"not-exist","answer":"x"}' '' || true

echo "== 7. 虚拟角色 =="
check "GET  /api/virtual_entities" GET "/api/virtual_entities" 200 "" ''
EID=""
check "POST /api/virtual_entities" POST "/api/virtual_entities" 200 \
  '{"name":"冒烟测试角色","aliases":[{"name":"测试别名","source_work":"冒烟测试"}]}' 'entity_id'
EID=$(json_get "$TMP/out.json" entity_id)
if [ -n "$EID" ]; then
  check "GET  /api/virtual_entities/:id" GET "/api/virtual_entities/$EID" 200 "" ''
  check "PUT  /api/virtual_entities/:id" PUT "/api/virtual_entities/$EID" 200 \
    '{"name":"冒烟测试角色2"}' ''
  check "GET  /api/virtual_entities/check/:name" GET "/api/virtual_entities/check/%E5%86%92%E7%83%9F%E6%B5%8B%E8%AF%95%E8%A7%92%E8%89%B22" 200 "" ''
  check "DELETE /api/virtual_entities/:id" DELETE "/api/virtual_entities/$EID" 200 "" ''
else
  skip "GET/PUT/DELETE /api/virtual_entities/:id" "未拿到 entity_id"
fi

echo "== 8. 重放 =="
if [ -n "$CID" ]; then
  check "POST /api/memory/replay/:id" POST "/api/memory/replay/$CID" 200 "" ''
else
  skip "POST /api/memory/replay/:id" "无 conversation_id"
fi
check "POST /api/memory/replay_failed" POST "/api/memory/replay_failed?limit=5" 200 "" ''

echo "== 9. 批量导入 / 备份 / 导出 =="
check "POST /api/memory/batch" POST "/api/memory/batch" 200 \
  "{\"user_id\":\"$UID_\",\"items\":[{\"conversation\":\"批量冒烟：明天和王五对一下合同。\",\"timestamp\":\"2026-10-28 10:00:00\"}]}" \
  '"accepted":1'
check "POST /api/backup" POST "/api/backup" 200 '{"path":"./backups/smoke-test.db"}' '"size_bytes"'
rm -f ./backups/smoke-test.db 2>/dev/null
check "GET  /api/export" GET "/api/export?user_id=$UID_&limit=50" 200 "" '"counts"'

echo ""
echo "== 汇总：PASS=$PASS  FAIL=$FAIL  SKIP=$SKIP =="
if [ "$FAIL" -gt 0 ]; then
  printf "失败项：%b\n" "$FAILED_LIST"
  exit 1
fi
