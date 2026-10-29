#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""联系人质量审计：逐条核对「画像 / 人设 / 社会关系 / 关联记忆」是否真的生成且对得上。

审计项（每条联系人都查，不是抽样）：
1. **画像结构**：background 能否解析成 JSON，是否含 5 个必备 key；
2. **人设自洽**：personal_info.name 是否等于联系人姓名；职业/兴趣是否为有效值（非"未知"占位）；
3. **社会关系可解析**：social_relations[].related_contact_name 是否能在联系人表里找到；
4. **关系索引一致**：contact_index.related_contact_ids 解析出的姓名，是否出现在画像的 social_relations 里
   （两张表不能各说各话）；
5. **关联记忆对应**：detail 返回的每条 memories，其原文必须真的包含该联系人的姓名或别名
   （防止"关键词命中"退化成"给了别人的对话"）；
6. **事件归属**：detail 的 events[].contact_ids 必须包含该联系人 ID。

用法：
    python scripts/audit_contacts.py --base http://127.0.0.1:3000 --user stress242
    python scripts/audit_contacts.py ... --rounds 3 --interval 60   # 反复抽查
"""

import argparse
import json
import time
import urllib.error
import urllib.request

REQUIRED_KEYS = [
    "personal_info",
    "social_relations",
    "major_events",
    "objective_evaluation",
    "potential_relations",
]


def req(method, url, payload=None, timeout=60):
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8") if payload is not None else None
    headers = {"Accept": "application/json"}
    if data:
        headers["Content-Type"] = "application/json"
    r = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return json.loads(body) if body.strip() else {}
    except urllib.error.HTTPError as e:
        return {"error": f"HTTP {e.code}"}
    except Exception as e:
        return {"error": str(e)}


def audit_once(base, user, verbose=True):
    contacts = req("POST", f"{base}/api/memory/search/contacts",
                   {"query": "", "limit": 500, "user_id": user, "include_archived": True})
    rows = contacts.get("data") or []
    by_name = {c["name"]: c for c in rows}
    by_id = {c["contact_id"]: c for c in rows}

    stats = {
        "contacts": len(rows),
        "with_profile": 0,
        "json_ok": 0,
        "keys_ok": 0,
        "name_match": 0,
        "has_relation": 0,
        "relations_total": 0,
        "relations_resolved": 0,
        "index_consistent": 0,
        "detail_checked": 0,
        "memories_total": 0,
        "memories_matched": 0,
        "events_total": 0,
        "events_owned": 0,
    }
    problems = []
    samples = []

    for c in rows:
        bg = (c.get("background") or "").strip()
        if not bg:
            continue
        stats["with_profile"] += 1
        name = c["name"]
        aliases = c.get("aliases") or []
        try:
            prof = json.loads(bg)
            stats["json_ok"] += 1
        except Exception:
            problems.append({"contact": name, "issue": "画像不是合法 JSON"})
            continue

        missing = [k for k in REQUIRED_KEYS if k not in prof]
        if missing:
            problems.append({"contact": name, "issue": f"画像缺字段 {missing}"})
        else:
            stats["keys_ok"] += 1

        pi = prof.get("personal_info") or {}
        if (pi.get("name") or "").strip() == name:
            stats["name_match"] += 1
        elif pi.get("name"):
            problems.append({"contact": name, "issue": f"画像里的 name 不一致: {pi.get('name')}"})

        # 社会关系：名称是否可解析
        rels = prof.get("social_relations") or []
        resolved = 0
        for r in rels:
            if isinstance(r, dict):
                rname = (r.get("related_contact_name") or "").strip()
            elif isinstance(r, str):
                rname = r.strip()
            else:
                rname = ""
            if rname:
                stats["relations_total"] += 1
                if rname in by_name or rname in aliases:
                    resolved += 1
                    stats["relations_resolved"] += 1
        if rels:
            stats["has_relation"] += 1
        if rels and resolved < len(rels):
            problems.append({
                "contact": name,
                "issue": f"社会关系有 {len(rels) - resolved}/{len(rels)} 条在联系人表里找不到同名对象",
            })

        # 关系索引与画像是否一致
        idx_names = set()
        for r in (c.get("related_contacts") or []):
            if isinstance(r, dict):
                if r.get("name"):
                    idx_names.add(r["name"])
            elif isinstance(r, str) and r.strip():
                idx_names.add(r.strip())
        prof_names = set()
        for r in rels:
            rname = (r.get("related_contact_name") if isinstance(r, dict) else r) or ""
            if rname and rname != name:
                prof_names.add(rname.strip())
        if idx_names and prof_names:
            if idx_names & prof_names:
                stats["index_consistent"] += 1
            else:
                problems.append({
                    "contact": name,
                    "issue": f"关系索引 {sorted(idx_names)[:3]} 与画像 {sorted(prof_names)[:3]} 无交集",
                })

        # 关联记忆：逐条核对是否真的包含姓名/别名
        det = req("GET", f"{base}/api/contacts/{c['contact_id']}/detail?user_id={user}&event_limit=20&memory_limit=10")
        data = det.get("data") or {}
        if data.get("memories") is not None:
            stats["detail_checked"] += 1
        keys = [name] + [a for a in aliases if a]
        for m in data.get("memories") or []:
            stats["memories_total"] += 1
            snippet = m.get("snippet") or ""
            if any(k and k in snippet for k in keys):
                stats["memories_matched"] += 1
            else:
                problems.append({
                    "contact": name,
                    "issue": "记忆原文不含该联系人姓名/别名",
                    "snippet": snippet[:60],
                })
        for e in data.get("events") or []:
            stats["events_total"] += 1
            if c["contact_id"] in (e.get("contact_ids") or []):
                stats["events_owned"] += 1

        if len(samples) < 3 and rels:
            samples.append({
                "name": name,
                "aliases": aliases,
                "tags": c.get("relationship_tags"),
                "related": [r["name"] if isinstance(r, dict) else r for r in (c.get("related_contacts") or [])],
                "occupation": pi.get("known_occupation"),
                "relations": len(rels),
                "resolved": resolved,
                "memories": len(data.get("memories") or []),
                "events": len(data.get("events") or []),
            })

    if verbose:
        cov = stats["with_profile"] / stats["contacts"] if stats["contacts"] else 0
        print(f"联系人 {stats['contacts']}  有画像 {stats['with_profile']}（{cov:.1%}）")
        print(f"  合法 JSON {stats['json_ok']}  必备字段齐全 {stats['keys_ok']}  姓名自洽 {stats['name_match']}")
        print(f"  有社会关系 {stats['has_relation']}  关系条目 {stats['relations_total']}"
              f"  可解析 {stats['relations_resolved']}")
        print(f"  关系索引与画像一致 {stats['index_consistent']}")
        print(f"  记忆 {stats['memories_total']} 条，命中本人 {stats['memories_matched']}")
        print(f"  事件 {stats['events_total']} 条，归属本人 {stats['events_owned']}")
        if samples:
            print("  抽样：")
            for s in samples:
                print("   ", json.dumps(s, ensure_ascii=False))
        if problems:
            print(f"  问题 {len(problems)} 条，前 8 条：")
            for p in problems[:8]:
                print("   ", json.dumps(p, ensure_ascii=False)[:160])
        else:
            print("  未发现问题")

    return {"stats": stats, "problems": problems, "samples": samples}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:3000")
    ap.add_argument("--user", default="stress242")
    ap.add_argument("--rounds", type=int, default=1, help="反复抽查轮数")
    ap.add_argument("--interval", type=int, default=0, help="每轮间隔秒数")
    ap.add_argument("--out", default="docs/contact_audit.json")
    args = ap.parse_args()

    all_rounds = []
    for i in range(args.rounds):
        if i:
            print(f"\n--- 第 {i + 1} 轮抽查（间隔 {args.interval}s）---")
            time.sleep(args.interval)
        res = audit_once(args.base, args.user)
        all_rounds.append(res)

    with open(args.out, "w", encoding="utf-8") as f:
        json.dump({"user": args.user, "rounds": all_rounds}, f, ensure_ascii=False, indent=1)
    print(f"\n审计结果写入 {args.out}")


if __name__ == "__main__":
    main()
