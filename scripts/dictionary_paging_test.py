#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""字典库按天分页 + 凌晨兜底 专项验收。

验证的设计点：
1. 每条写入的原文都带时间戳（可由客户端指定，用于导入历史）；
2. 字典库按"会话日"分页：本地时间 04:00 前算前一天（跨零点不切页）；
3. 日索引（days）计数与页内分页（offset/limit）不重不漏；
4. 单条原文可按 ID 回溯；
5. owner 隔离：别人的对话查不到；
6. 非法/未来时间戳被拒绝；
7. 联系人事件档案（字典库永久副本）可读。

用法：
    python scripts/dictionary_paging_test.py --base http://127.0.0.1:3000
"""

import argparse
import json
import time
import urllib.error
import urllib.request

DAY1 = "2026-10-29"
DAY2 = "2026-10-30"

# (内容, 时间戳, 期望会话日)
CASES = [
    ("十月二十九日晚上十点四十，讨论发布计划。", "2026-10-29 23:40:00", DAY1),
    ("聊到零点之后还在继续，这一段属于同一次会话。", "2026-10-30 00:30:00", DAY1),
    ("凌晨三点五十九，仍在同一个会话日。", "2026-10-30 03:59:00", DAY1),
    ("早上四点整，换页了。", "2026-10-30 04:00:00", DAY2),
    ("中午十二点的对话。", "2026-10-30T12:00:00+08:00", DAY2),
    ("只给日期时按当天中午归档。", "2026-10-30", DAY2),
]


def req(method, url, payload=None, timeout=60):
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8") if payload is not None else None
    headers = {"Content-Type": "application/json"} if data else {}
    r = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return json.loads(body) if body.strip() else {}, resp.status
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")
        try:
            return json.loads(body), e.code
        except Exception:
            return {"raw": body}, e.code
    except Exception as e:
        return {"error": str(e)}, 0


class Report:
    def __init__(self):
        self.rows = []

    def check(self, name, ok, detail=""):
        self.rows.append((name, bool(ok), detail))
        print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))

    def summary(self):
        total = len(self.rows)
        passed = sum(1 for _, ok, _ in self.rows if ok)
        print(f"\n=== 字典库分页验收：{passed}/{total} 通过 ===")
        for name, ok, detail in self.rows:
            if not ok:
                print(f"  FAIL: {name} — {detail}")
        return passed, total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:3000")
    ap.add_argument("--user", default=f"dictpage_{int(time.time())}")
    args = ap.parse_args()
    base = args.base.rstrip("/")
    user = args.user
    rep = Report()

    # ---------- 1. 写入（带时间戳） ----------
    ids = []
    for text, ts, _ in CASES:
        body, code = req("POST", f"{base}/api/memory/add", {
            "conversation": text,
            "user_id": user,
            "timestamp": ts,
        })
        cid = (body.get("data") or {}).get("conversation_id")
        ids.append(cid)
        rep.check(f"写入 {ts}", code == 200 and bool(cid), f"http={code} id={cid}")

    # ---------- 2. 非法时间戳 ----------
    _, code = req("POST", f"{base}/api/memory/add", {
        "conversation": "时间格式错误", "user_id": user, "timestamp": "不是时间",
    })
    rep.check("非法时间戳被拒绝(400)", code == 400, f"http={code}")

    _, code = req("POST", f"{base}/api/memory/add", {
        "conversation": "未来时间", "user_id": user, "timestamp": "2099-01-01 10:00:00",
    })
    rep.check("未来时间戳被拒绝(400)", code == 400, f"http={code}")

    # ---------- 3. 日索引 ----------
    body, code = req("GET", f"{base}/api/dictionary/days?user_id={user}&limit=30")
    days = {d["day"]: d for d in (body.get("data") or {}).get("days", [])}
    rep.check("days 接口可用", code == 200 and len(days) >= 2, f"http={code} days={list(days)}")
    rep.check(f"{DAY1} 计数=3（含凌晨）", days.get(DAY1, {}).get("count") == 3,
              f"实际={days.get(DAY1, {}).get('count')}")
    rep.check(f"{DAY2} 计数=3", days.get(DAY2, {}).get("count") == 3,
              f"实际={days.get(DAY2, {}).get('count')}")
    rep.check("days 倒序（最新在前）",
              bool(days) and list(days)[0] == DAY2, f"顺序={list(days)}")

    # ---------- 4. 按天取页 ----------
    body, code = req("GET", f"{base}/api/dictionary/day/{DAY1}?user_id={user}&limit=50")
    page1 = (body.get("data") or {}).get("conversations", [])
    rep.check(f"{DAY1} 页有 3 条", code == 200 and len(page1) == 3, f"http={code} n={len(page1)}")
    rep.check("凌晨 00:30/03:59 归入前一天",
              all(c.get("session_day") == DAY1 for c in page1),
              f"session_day={[c.get('session_day') for c in page1]}")
    times = [c["timestamp"] for c in page1]
    rep.check("页内按时间升序", times == sorted(times), f"{times}")

    body, _ = req("GET", f"{base}/api/dictionary/day/{DAY2}?user_id={user}&limit=50")
    page2 = (body.get("data") or {}).get("conversations", [])
    rep.check(f"{DAY2} 页有 3 条", len(page2) == 3, f"n={len(page2)}")
    rep.check("04:00 整点换页", any(c["timestamp"].endswith("T20:00:00+00:00") for c in page2),
              f"times={[c['timestamp'] for c in page2]}")

    # ---------- 5. 页内分页不重不漏 ----------
    a, _ = req("GET", f"{base}/api/dictionary/day/{DAY2}?user_id={user}&limit=2&offset=0")
    b, _ = req("GET", f"{base}/api/dictionary/day/{DAY2}?user_id={user}&limit=2&offset=2")
    ids_a = [c["conversation_id"] for c in (a.get("data") or {}).get("conversations", [])]
    ids_b = [c["conversation_id"] for c in (b.get("data") or {}).get("conversations", [])]
    rep.check("分页不重叠", not (set(ids_a) & set(ids_b)), f"{ids_a} vs {ids_b}")
    rep.check("分页合计=当天条数", len(ids_a) + len(ids_b) == 3, f"{len(ids_a)}+{len(ids_b)}")
    rep.check("第一页满 2 条时 has_more=true",
              bool((a.get("data") or {}).get("has_more")) is True,
              f"has_more={(a.get('data') or {}).get('has_more')}")

    # ---------- 6. 单条回溯 ----------
    target = ids[1]
    body, code = req("GET", f"{base}/api/dictionary/conversation/{target}?user_id={user}")
    data = body.get("data") or {}
    rep.check("按 ID 取回原文", code == 200 and data.get("conversation_id") == target, f"http={code}")
    rep.check("原文完整（不是预览）",
              data.get("content") == CASES[1][0], f"content={data.get('content')!r}")

    # ---------- 7. owner 隔离 ----------
    _, code = req("GET", f"{base}/api/dictionary/conversation/{target}?user_id=someone_else")
    rep.check("其他 owner 查不到(404)", code == 404, f"http={code}")

    # ---------- 8. 统计 ----------
    body, code = req("GET", f"{base}/api/dictionary/stats")
    stats = body.get("data") or {}
    rep.check("stats 可用", code == 200 and stats.get("conversations", 0) >= len(CASES),
              f"http={code} conversations={stats.get('conversations')}")
    rep.check("stats 带分页口径", stats.get("cutoff_hour") == 4,
              f"cutoff={stats.get('cutoff_hour')} offset={stats.get('utc_offset_minutes')}")
    rep.check("stats 带状态分布", isinstance(stats.get("status_counts"), dict) and
              len(stats.get("status_counts") or {}) > 0, f"{stats.get('status_counts')}")

    # ---------- 9. 分页口径接口 ----------
    body, code = req("GET", f"{base}/api/dictionary/paging")
    rep.check("paging 口径可读", code == 200 and (body.get("data") or {}).get("cutoff_hour") == 4,
              f"http={code}")

    # ---------- 10. 联系人事件档案 ----------
    body, code = req("GET", f"{base}/api/contacts/__none__/event_archive")
    rep.check("事件档案接口可用（空联系人返回空列表）",
              code == 200 and (body.get("data") or {}).get("count") == 0, f"http={code}")

    passed, total = rep.summary()
    raise SystemExit(0 if passed == total else 1)


if __name__ == "__main__":
    main()
