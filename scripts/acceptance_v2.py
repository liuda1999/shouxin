"""守心验收审计 v2 —— 全新语料（225 条）+ 六项审计。

用法：
    python scripts/acceptance_v2.py corpus
    python scripts/acceptance_v2.py submit --base http://127.0.0.1:3000 --user acc2
    python scripts/acceptance_v2.py poll   --base http://127.0.0.1:3000 --user acc2 --timeout-min 200
    python scripts/acceptance_v2.py audit  --base http://127.0.0.1:3000 --user acc2 --db ./data/shouxin.db --phase before
    python scripts/acceptance_v2.py audit  ... --phase after     # 心跳跑过之后

审计六项：
    1. API 全功能覆盖（29 个端点逐个调用）
    2. 联系人创建（召回 / 捏造 / 归档）
    3. 别名识别（抽取进 aliases + 用别名能否搜到本人）
    4. 画像与人设（生成率 / JSON 合法性 / 更新检测）
    5. 多联系人关系（related_contacts 是否包含期望对象）
    6. 时间线索引（事件是否都带时间；since=30d 过滤是否正确）
"""

import argparse
import json
import os
import sqlite3
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from corpus_v2 import build_cases  # noqa: E402


def state_path(user):
    return f".acc2_state_{user}.json"


def profile_snapshot_path(user):
    return f".acc2_profiles_{user}.json"


def _req(method, url, payload=None, timeout=90, raw=None, headers=None):
    data = None
    hdrs = dict(headers or {})
    if raw is not None:
        data = raw
    elif payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        hdrs.setdefault("Content-Type", "application/json")
    r = urllib.request.Request(url, data=data, headers=hdrs, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return (json.loads(body) if body.strip() else {}), resp.status
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")
        try:
            return (json.loads(body) if body.strip() else {}), e.code
        except Exception:
            return {"error": body}, e.code
    except Exception as e:
        return {"error": str(e)}, 0


def post(base, path, payload=None, timeout=90):
    return _req("POST", base + path, payload, timeout)


def get(base, path, timeout=60):
    return _req("GET", base + path, None, timeout)


# ---------------------------------------------------------------- corpus

def cmd_corpus(_args):
    cases = build_cases()
    import collections
    print(f"语料总数: {len(cases)}")
    for k, v in collections.Counter(c["cat"] for c in cases).most_common():
        print(f"  {k:10s} {v}")
    print(f"人名槽位 {sum(len(c['people']) for c in cases)} | "
          f"别名对 {sum(len(c['aliases']) for c in cases)} | "
          f"关系对 {sum(len(c['relations']) for c in cases)} | "
          f"事件槽位 {sum(len(c['events']) for c in cases)} | "
          f"待办槽位 {sum(len(c['todos']) for c in cases)}")


# ---------------------------------------------------------------- submit

def post_add(base, text, user, max_retries=15):
    last = None
    for _ in range(max_retries):
        resp, code = _req("POST", base + "/api/memory/add",
                          {"conversation": text, "user_id": user}, timeout=60)
        if isinstance(resp, dict) and resp.get("success"):
            cid = (resp.get("data") or {}).get("conversation_id")
            if cid:
                return resp, code
        last = resp
        msg = str(resp)
        if "Rate limit" in msg or "限流" in msg:
            time.sleep(2.5)
            continue
        return resp, code
    return last, 0


def cmd_submit(args):
    cases = build_cases()
    sf = state_path(args.user)
    state = {"user": args.user, "base": args.base, "items": {}}
    if os.path.exists(sf):
        with open(sf, encoding="utf-8") as f:
            state = json.load(f)
        state["user"] = args.user
        state["base"] = args.base

    ok = skip = fail = 0
    for i, c in enumerate(cases, 1):
        prev = state["items"].get(c["id"])
        if prev and prev.get("cid"):
            skip += 1
            continue
        if not c["text"].strip():
            # 空文本是负向用例：服务端应拒绝，记录期望
            resp, code = _req("POST", args.base + "/api/memory/add",
                              {"conversation": c["text"], "user_id": args.user}, timeout=30)
            state["items"][c["id"]] = {"cat": c["cat"], "expect_reject": True,
                                       "http": code, "resp": str(resp)[:120]}
            if code >= 400:
                ok += 1
            else:
                fail += 1
            continue

        resp, code = post_add(args.base, c["text"], args.user)
        cid = (resp.get("data") or {}).get("conversation_id") if isinstance(resp, dict) else None
        if cid:
            state["items"][c["id"]] = {"cid": cid, "cat": c["cat"], "chars": len(c["text"])}
            ok += 1
        else:
            state["items"][c["id"]] = {"cat": c["cat"], "error": str(resp)[:160]}
            fail += 1
        if i % 25 == 0:
            with open(sf, "w", encoding="utf-8") as f:
                json.dump(state, f, ensure_ascii=False, indent=1)
            print(f"  进度 {i}/{len(cases)}（成功 {ok} 跳过 {skip} 失败 {fail}）", flush=True)

    with open(sf, "w", encoding="utf-8") as f:
        json.dump(state, f, ensure_ascii=False, indent=1)
    total = len(state["items"])
    print(f"提交完成：成功 {ok} / 跳过 {skip} / 失败 {fail} / 记录 {total} → {sf}")


# ---------------------------------------------------------------- poll

def cmd_poll(args):
    sf = state_path(args.user)
    if not os.path.exists(sf):
        print(f"缺少 {sf}")
        sys.exit(1)
    with open(sf, encoding="utf-8") as f:
        state = json.load(f)

    deadline = time.time() + args.timeout_min * 60
    while time.time() < deadline:
        pending = done = failed = rejected = 0
        for item in state["items"].values():
            if item.get("expect_reject"):
                rejected += 1
                continue
            cid = item.get("cid")
            if not cid:
                failed += 1
                continue
            resp, code = get(args.base, f"/api/memory/extract_status/{cid}", timeout=30)
            st = ((resp.get("data") or {}).get("status") or "").lower()
            if st in ("completed", "failed"):
                item["status"] = st
                if st == "failed":
                    item["error"] = (resp.get("data") or {}).get("extraction_error")
                    failed += 1
                else:
                    done += 1
            else:
                pending += 1
        with open(sf, "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=1)
        print(f"[{datetime.now():%H:%M:%S}] 完成 {done} | 处理中 {pending} | 失败 {failed} | 拒绝 {rejected}", flush=True)
        if pending == 0:
            print("全部到达终态")
            return
        time.sleep(args.interval)


# ---------------------------------------------------------------- audit helpers

def fetch_contacts(base, user, limit=500, include_archived=True, include_virtual=False):
    _r, _c = post(base, "/api/memory/search/contacts",
                  {"query": "", "limit": limit, "user_id": user,
                   "include_archived": include_archived,
                   "include_virtual": include_virtual})
    return (_r.get("data") or [])


def find_contact(base, user, query):
    r, _ = post(base, "/api/memory/search/contacts",
                {"query": query, "limit": 20, "user_id": user, "include_archived": True})
    return (r.get("data") or [])


def detail(base, user, cid, since=None, event_limit=100, memory_limit=10):
    q = f"?user_id={user}&event_limit={event_limit}&memory_limit={memory_limit}"
    if since:
        q += f"&since={since}"
    r, code = get(base, f"/api/contacts/{cid}/detail{q}")
    return (r.get("data") or {}), code


def profile_fields_ok(bg):
    """画像 JSON 是否合法且含五个必备字段"""
    if not bg:
        return False, "empty"
    try:
        d = json.loads(bg)
    except Exception:
        return False, "invalid_json"
    need = ["personal_info", "social_relations", "major_events",
            "objective_evaluation", "potential_relations"]
    missing = [k for k in need if k not in d]
    if missing:
        return False, f"missing:{','.join(missing)}"
    return True, "ok"


def cmd_audit(args):
    cases = build_cases()
    sf = state_path(args.user)
    with open(sf, encoding="utf-8") as f:
        state = json.load(f)

    report = {"user": args.user, "phase": args.phase, "generated_at": datetime.now(timezone.utc).isoformat()}

    # ---------- 1) 抽取 ----------
    items = state["items"]
    total = len(items)
    completed = sum(1 for v in items.values() if v.get("status") == "completed")
    failed = sum(1 for v in items.values() if v.get("status") == "failed")
    pending = total - completed - failed - sum(1 for v in items.values() if v.get("expect_reject"))
    rejected = sum(1 for v in items.values() if v.get("expect_reject") and v.get("http", 0) >= 400)
    report["extraction"] = {
        "total": total, "completed": completed, "failed": failed,
        "pending": pending, "rejected_as_expected": rejected,
        "by_cat": {},
    }
    for c in cases:
        it = items.get(c["id"], {})
        st = it.get("status", "rejected" if it.get("expect_reject") else "pending")
        d = report["extraction"]["by_cat"].setdefault(c["cat"], {"n": 0, "completed": 0})
        d["n"] += 1
        if st == "completed":
            d["completed"] += 1

    # 统计口径：只算"已抽取完成"的语料对应的期望，
    # 否则中途审计会把还没处理的人名算成漏检。
    done_ids = {cid for cid, v in items.items() if v.get("status") == "completed"}
    cases_done = [c for c in cases if c["id"] in done_ids]
    report["extraction"]["cases_completed"] = len(cases_done)

    # ---------- 2) 联系人 ----------
    contacts = fetch_contacts(args.base, args.user)
    names = {c["name"] for c in contacts}
    alias_set = set()
    for c in contacts:
        for a in (c.get("aliases") or []):
            if a:
                alias_set.add(a)

    exp_people = [p for c in cases_done for p in c["people"]]
    exp_unique = list(dict.fromkeys(exp_people))
    hit = [p for p in exp_unique if p in names or p in alias_set]
    missing = [p for p in exp_unique if p not in names and p not in alias_set]

    all_text = "\n".join(c["text"] for c in cases_done)
    junk = [c["name"] for c in contacts
            if c["name"] not in all_text and not any((a or "") in all_text for a in (c.get("aliases") or []))]

    report["contacts"] = {
        "count": len(contacts),
        "archived": sum(1 for c in contacts if c.get("is_archived")),
        "expected_unique_names": len(exp_unique),
        "recall_hit": len(hit),
        "recall_rate": round(len(hit) / max(1, len(exp_unique)), 4),
        "missing": missing[:30],
        "fabricated": junk[:20],
        "fabricated_count": len(junk),
    }

    # ---------- 3) 别名识别 ----------
    alias_pairs = [(n, a) for c in cases_done for (n, a) in c["aliases"]]
    by_name = {}
    for c in contacts:
        by_name.setdefault(c["name"], []).append(c)

    extracted = resolved = 0
    alias_miss = []
    for name, alias in alias_pairs:
        rows = by_name.get(name, [])
        if any(alias in (r.get("aliases") or []) for r in rows):
            extracted += 1
        got = find_contact(args.base, args.user, alias)
        target_ids = {r["contact_id"] for r in rows}
        if any(g["contact_id"] in target_ids for g in got):
            resolved += 1
        else:
            alias_miss.append(f"{name}/{alias}")

    report["alias"] = {
        "pairs": len(alias_pairs),
        "extracted_into_aliases": extracted,
        "extracted_rate": round(extracted / max(1, len(alias_pairs)), 4),
        "resolvable_by_alias": resolved,
        "resolve_rate": round(resolved / max(1, len(alias_pairs)), 4),
        "miss": alias_miss[:20],
    }

    # ---------- 4) 画像 ----------
    snapshot_file = profile_snapshot_path(args.user)
    prev = {}
    if args.phase == "after" and os.path.exists(snapshot_file):
        with open(snapshot_file, encoding="utf-8") as f:
            prev = json.load(f)

    with_bg, valid = 0, 0
    new_profiles = updated = 0
    bad_samples = []
    for c in contacts:
        bg = (c.get("background") or "").strip()
        if bg:
            with_bg += 1
            ok, why = profile_fields_ok(bg)
            if ok:
                valid += 1
            elif len(bad_samples) < 5:
                bad_samples.append(f"{c['name']}:{why}")

        # 与上次快照比较：区分"新增画像"和"更新画像"
        if prev:
            old = (prev.get(c["contact_id"]) or "").strip()
            if bg and not old:
                new_profiles += 1
            elif bg and old and bg != old:
                updated += 1

    snap = {c["contact_id"]: (c.get("background") or "") for c in contacts}
    with open(snapshot_file, "w", encoding="utf-8") as f:
        json.dump(snap, f, ensure_ascii=False)

    report["profiles"] = {
        "contacts": len(contacts),
        "with_profile": with_bg,
        "coverage": round(with_bg / max(1, len(contacts)), 4),
        "valid_json_with_5_fields": valid,
        "valid_rate": round(valid / max(1, with_bg), 4),
        "bad_samples": bad_samples,
        "new_since_last_audit": new_profiles if args.phase == "after" else None,
        "updated_since_last_audit": updated if args.phase == "after" else None,
    }

    # ---------- 5) 关系 ----------
    rel_pairs = [(a, b) for c in cases_done for (a, b) in c["relations"]]
    detail_cache = {}

    def related_names(cid):
        if cid in detail_cache:
            return detail_cache[cid]
        d, code = detail(args.base, args.user, cid, event_limit=5, memory_limit=1)
        names_ = []
        if code == 200:
            for r in (d.get("related_contacts") or []):
                names_.append(r.get("name") if isinstance(r, dict) else r)
        detail_cache[cid] = names_
        return names_

    # 关系只在画像阶段建立，所以分母要区分：
    # 全部关系对 vs "至少一方已生成画像"的可评估关系对。
    profiled_names = {c["name"] for c in contacts if (c.get("background") or "").strip()}
    rel_evaluable = [(a, b) for a, b in rel_pairs if a in profiled_names or b in profiled_names]

    rel_hit = 0
    rel_miss = []
    for a, b in rel_pairs:
        ra, rb = by_name.get(a, []), by_name.get(b, [])
        ok = False
        for r in ra:
            if b in related_names(r["contact_id"]):
                ok = True
                break
        if not ok:
            for r in rb:
                if a in related_names(r["contact_id"]):
                    ok = True
                    break
        if ok:
            rel_hit += 1
        else:
            rel_miss.append(f"{a}-{b}")

    rel_hit_eval = sum(1 for a, b in rel_evaluable if f"{a}-{b}" not in rel_miss)
    report["relations"] = {
        "pairs": len(rel_pairs),
        "recorded": rel_hit,
        "rate": round(rel_hit / max(1, len(rel_pairs)), 4),
        "evaluable_pairs": len(rel_evaluable),
        "evaluable_recorded": rel_hit_eval,
        "evaluable_rate": round(rel_hit_eval / max(1, len(rel_evaluable)), 4),
        "miss": rel_miss[:20],
    }

    # ---------- 6) 时间线 ----------
    tl_cases = [c for c in cases_done if c["cat"] == "timeline"]
    tl_total = tl_all_dated = 0
    filtered_ok = filtered_bad = 0
    tl_samples = []
    for c in tl_cases:
        for name in c["people"]:
            rows = by_name.get(name, [])
            if not rows:
                continue
            d_all, code_all = detail(args.base, args.user, rows[0]["contact_id"],
                                     event_limit=100, memory_limit=2)
            if code_all != 200:
                continue
            evs = d_all.get("events") or []
            tl_total += len(evs)
            tl_all_dated += sum(1 for e in evs if e.get("event_time") or e.get("time_range_start"))

            d_30, code_30 = detail(args.base, args.user, rows[0]["contact_id"],
                                   since="30d", event_limit=100, memory_limit=2)
            if code_30 != 200:
                continue
            ev30 = d_30.get("events") or []
            now = datetime.now(timezone.utc)
            ok = len(ev30) <= len(evs)
            for e in ev30:
                t = e.get("event_time") or e.get("time_range_start")
                if not t:
                    continue
                try:
                    dt = datetime.fromisoformat(t.replace("Z", "+00:00"))
                except Exception:
                    continue
                if (now - dt).days > 31:
                    ok = False
            if ok:
                filtered_ok += 1
            else:
                filtered_bad += 1
            if len(tl_samples) < 4:
                tl_samples.append({
                    "name": name, "all": len(evs), "last_30d": len(ev30),
                    "since_echo": (d_30.get("stats") or {}).get("since"),
                })

    report["timeline"] = {
        "contacts_checked": len(tl_cases),
        "events_total": tl_total,
        "events_with_time": tl_all_dated,
        "time_field_rate": round(tl_all_dated / max(1, tl_total), 4),
        "since_30d_ok": filtered_ok,
        "since_30d_bad": filtered_bad,
        "samples": tl_samples,
    }

    # ---------- 7) API 全功能覆盖 ----------
    # API 覆盖用独立 owner 跑，避免往被测用户里塞测试角色 / 临时待办 / 文件
    report["api_coverage"] = api_coverage(args.base, f"{args.user}_api")

    out = args.out or f"docs/acceptance_v2_{args.phase}.json"
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=1)

    print_summary(report)
    print(f"\n明细写入 {out}")


def api_coverage(base, user):
    """逐个端点调用，返回通过/失败清单。"""
    results = []

    def chk(name, ok, detail=""):
        results.append({"api": name, "ok": bool(ok), "detail": str(detail)[:120]})

    # health
    r, c = get(base, "/health")
    chk("GET /health", c == 200 and r.get("status") == "ok")

    # add + status
    r, c = post(base, "/api/memory/add",
                {"conversation": f"审计用例：{uuid.uuid4().hex[:6]} 何嘉树说下周要交付接口文档。",
                 "user_id": user})
    cid = (r.get("data") or {}).get("conversation_id")
    chk("POST /api/memory/add", c == 200 and bool(cid), c)
    if cid:
        r, c = get(base, f"/api/memory/extract_status/{cid}")
        chk("GET /api/memory/extract_status/:id", c == 200)

    # 检索类
    for path, body, name in [
        ("/api/memory/search/semantic", {"query": "接口文档", "limit": 3, "user_id": user},
         "POST /api/memory/search/semantic"),
        ("/api/memory/search/events", {"query": "", "limit": 3, "user_id": user},
         "POST /api/memory/search/events"),
        ("/api/memory/search/contacts", {"query": "", "limit": 3, "user_id": user},
         "POST /api/memory/search/contacts"),
        ("/api/memory/search/history", {"query": "接口", "limit": 3, "user_id": user},
         "POST /api/memory/search/history"),
        ("/api/search/smart", {"query": "接口文档", "limit": 3, "user_id": user},
         "POST /api/search/smart"),
        # /api/vector/search 与 /api/memory/search/semantic 是同一个 handler，请求体一致
        ("/api/vector/search", {"query": "接口文档", "limit": 2, "user_id": user},
         "POST /api/vector/search"),
    ]:
        r, c = post(base, path, body)
        chk(name, c in (200, 400), c)  # 400 可能是参数校验，不算端点缺失

    # 联系人详情
    rows = fetch_contacts(base, user, limit=5)
    if rows:
        cid0 = rows[0]["contact_id"]
        r, c = get(base, f"/api/contacts/{cid0}/detail?user_id={user}")
        chk("GET /api/contacts/:id/detail", c == 200)
        r, c = get(base, f"/api/contacts/{cid0}/detail?user_id={user}&since=30d")
        chk("GET /api/contacts/:id/detail?since=30d", c == 200)
        chk("detail 返回 stats.since", bool(((r.get("data") or {}).get("stats") or {}).get("since")))

    # 待办 CRUD
    r, c = post(base, "/api/todos", {"title": "审计临时待办", "user_id": user,
                                     "deadline": "2026-11-15T00:00:00Z"})
    todo_id = ((r.get("data") or {}).get("todo_id") or (r.get("data") or {}).get("id"))
    chk("POST /api/todos", c == 200 and bool(todo_id), c)
    r, c = get(base, f"/api/todos?user_id={user}")
    chk("GET /api/todos", c == 200)
    if todo_id:
        r, c = _req("PUT", f"{base}/api/todos/{todo_id}/complete?user_id={user}", {})
        chk("PUT /api/todos/:id/complete", c == 200, c)
        r, c = _req("DELETE", f"{base}/api/todos/{todo_id}?user_id={user}", None)
        chk("DELETE /api/todos/:id", c == 200, c)

    # 事件合并
    r, c = get(base, f"/api/events/pending_merges?user_id={user}")
    chk("GET /api/events/pending_merges", c == 200)

    # 心跳
    r, c = get(base, f"/api/heartbeat/status?user_id={user}")
    chk("GET /api/heartbeat/status", c == 200)
    r, c = get(base, f"/api/heartbeat/pending_questions?user_id={user}")
    chk("GET /api/heartbeat/pending_questions", c == 200)

    # 虚拟角色 CRUD
    vname = f"测试角色{uuid.uuid4().hex[:4]}"
    r, c = post(base, "/api/virtual_entities",
                {"name": vname, "source_work": "审计脚本", "user_id": user})
    vid = (r.get("data") or {}).get("entity_id")
    chk("POST /api/virtual_entities", c == 200 and bool(vid), c)
    r, c = get(base, "/api/virtual_entities")
    chk("GET /api/virtual_entities", c == 200)
    if vid:
        r, c = get(base, f"/api/virtual_entities/{vid}")
        chk("GET /api/virtual_entities/:id", c == 200, c)
        from urllib.parse import quote
        r, c = get(base, f"/api/virtual_entities/check/{quote(vname)}")
        chk("GET /api/virtual_entities/check/:name", c == 200, c)
        r, c = _req("PUT", f"{base}/api/virtual_entities/{vid}",
                    {"name": vname + "改", "user_id": user})
        chk("PUT /api/virtual_entities/:id", c == 200, c)
        r, c = _req("DELETE", f"{base}/api/virtual_entities/{vid}?user_id={user}", None)
        chk("DELETE /api/virtual_entities/:id", c == 200, c)

    # 文件上传：接口只接受原始字节流（application/octet-stream），
    # 文件名通过 query 传递；multipart 会被 415 拒绝（防止 boundary 污染 MD5）
    from urllib.parse import quote as _q
    content = "审计文件：这份文档记录了供应商比价的结论，需要归档。".encode("utf-8")
    r, c = _req("POST",
                f"{base}/api/files/upload?filename=audit_note.txt&user_id={_q(user)}",
                raw=content,
                headers={"Content-Type": "application/octet-stream"},
                timeout=120)
    fid = (r.get("data") or {}).get("file_id")
    chk("POST /api/files/upload", c == 200 and bool(fid), f"{c} {str(r)[:80]}")
    if fid:
        r, c = get(base, f"/api/files/{fid}")
        chk("GET /api/files/:id", c == 200)
        r, c = get(base, f"/api/files/{fid}/text")
        chk("GET /api/files/:id/text", c == 200)

    passed = sum(1 for x in results if x["ok"])
    return {"passed": passed, "total": len(results),
            "rate": round(passed / max(1, len(results)), 4),
            "items": results}


def print_summary(r):
    print("\n===== 验收审计 =====")
    e = r["extraction"]
    print(f"抽取：{e['completed']}/{e['total']} 完成，失败 {e['failed']}，待处理 {e['pending']}，"
          f"空输入被拒 {e['rejected_as_expected']}")
    c = r["contacts"]
    print(f"联系人：{c['count']} 条（归档 {c['archived']}）｜人名召回 {c['recall_hit']}/{c['expected_unique_names']} "
          f"= {c['recall_rate']:.1%}｜凭空捏造 {c['fabricated_count']}")
    a = r["alias"]
    print(f"别名：{a['pairs']} 对｜抽进 aliases {a['extracted_into_aliases']} = {a['extracted_rate']:.1%}｜"
          f"用别名能搜到本人 {a['resolvable_by_alias']} = {a['resolve_rate']:.1%}")
    p = r["profiles"]
    print(f"画像：{p['with_profile']}/{p['contacts']} = {p['coverage']:.1%}｜"
          f"合法 JSON+5 字段 {p['valid_json_with_5_fields']} = {p['valid_rate']:.1%}"
          + (f"｜新增 {p['new_since_last_audit']} 条｜更新 {p['updated_since_last_audit']} 条"
             if p["updated_since_last_audit"] is not None else ""))
    rl = r["relations"]
    print(f"关系：{rl['recorded']}/{rl['pairs']} = {rl['rate']:.1%} 被记录"
          f"（可评估 {rl['evaluable_recorded']}/{rl['evaluable_pairs']} = {rl['evaluable_rate']:.1%}）")
    t = r["timeline"]
    print(f"时间线：事件带时间 {t['events_with_time']}/{t['events_total']} = {t['time_field_rate']:.1%}｜"
          f"since=30d 正确 {t['since_30d_ok']}，异常 {t['since_30d_bad']}")
    api = r["api_coverage"]
    print(f"API 覆盖：{api['passed']}/{api['total']} = {api['rate']:.1%}")
    if api["passed"] < api["total"]:
        for it in api["items"]:
            if not it["ok"]:
                print(f"    [FAIL] {it['api']} {it['detail']}")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)

    p1 = sub.add_parser("corpus")
    p1.set_defaults(func=cmd_corpus)

    p2 = sub.add_parser("submit")
    p2.add_argument("--base", default="http://127.0.0.1:3000")
    p2.add_argument("--user", default="acc2")
    p2.set_defaults(func=cmd_submit)

    p3 = sub.add_parser("poll")
    p3.add_argument("--base", default="http://127.0.0.1:3000")
    p3.add_argument("--user", default="acc2")
    p3.add_argument("--timeout-min", type=int, default=200)
    p3.add_argument("--interval", type=int, default=30)
    p3.set_defaults(func=cmd_poll)

    p4 = sub.add_parser("audit")
    p4.add_argument("--base", default="http://127.0.0.1:3000")
    p4.add_argument("--user", default="acc2")
    p4.add_argument("--db", default="./data/shouxin.db")
    p4.add_argument("--phase", choices=["before", "after"], default="before")
    p4.add_argument("--out", default=None)
    p4.set_defaults(func=cmd_audit)

    args = ap.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
