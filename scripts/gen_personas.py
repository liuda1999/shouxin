#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""批量驱动心跳生成联系人画像/关系。

心跳每轮最多给 3 个"画像为空"的联系人生成画像（`LIMIT 3`），
单机上给几十个联系人生成画像需要多轮触发。本脚本负责按轮驱动并报告覆盖率。

用法：
    python scripts/gen_personas.py --base http://127.0.0.1:3000 --user stress200 --max-rounds 30
"""

import argparse
import json
import time
import urllib.error
import urllib.request


def req(method, url, payload=None, timeout=300):
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    r = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return json.loads(body) if body.strip() else {}
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")
        try:
            return json.loads(body)
        except Exception:
            return {"error": body}
    except Exception as e:
        return {"error": str(e)}


def contacts(base, user):
    r = req("POST", f"{base}/api/memory/search/contacts",
            {"query": "", "limit": 200, "user_id": user, "include_archived": True})
    return (r.get("data") or []) if isinstance(r, dict) else []


def stat(rows):
    total = len(rows)
    empty = [c for c in rows if not (c.get("background") or "").strip()]
    with_rel = [c for c in rows if c.get("related_contacts")]
    with_tag = [c for c in rows if c.get("relationship_tags")]
    return total, len(empty), len(with_rel), len(with_tag)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:3000")
    ap.add_argument("--user", default="stress200")
    ap.add_argument("--max-rounds", type=int, default=30)
    ap.add_argument("--wait", type=int, default=5, help="每轮后额外等待秒数")
    ap.add_argument("--round-timeout", type=int, default=180, help="单轮等待画像产出的上限秒数")
    args = ap.parse_args()

    rows = contacts(args.base, args.user)
    total, empty, rel, tag = stat(rows)
    print(f"起始：联系人 {total}，无画像 {empty}，有关系 {rel}，有标签 {tag}", flush=True)

    for i in range(1, args.max_rounds + 1):
        if empty == 0:
            print("所有联系人都已有人设，结束")
            break
        t0 = time.time()
        r = req("POST", f"{args.base}/api/heartbeat/trigger",
                {"maintenance_only": False})
        dur = time.time() - t0
        data = r.get("data") or {}
        print(f"第 {i} 轮：触发耗时 {dur:.1f}s  "
              f"actions={data.get('actions_generated')} "
              f"profiles_updated={data.get('profiles_updated')}", flush=True)

        # /api/heartbeat/trigger 是**异步**的：立即返回，实际 LLM 工作在后台跑。
        # 所以这里要等"无画像数量下降"再进入下一轮，而不是立刻再触发
        # （立刻触发只会拿到 already executing，看起来像"每轮 0.0 秒"）。
        deadline = time.time() + args.round_timeout
        while time.time() < deadline:
            time.sleep(12)
            rows = contacts(args.base, args.user)
            total, new_empty, rel, tag = stat(rows)
            if new_empty < empty:
                break
        gained = empty - new_empty
        empty = new_empty
        print(f"        联系人 {total}，无画像 {empty}，有关系 {rel}，有标签 {tag}"
              f"（本轮 +{gained}）", flush=True)
        if gained <= 0 and i >= 4:
            print("连续无进展，可能触发了每日上限或模型失败，提前结束")
            break


if __name__ == "__main__":
    main()
