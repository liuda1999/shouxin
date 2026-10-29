#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""联系人专项 API 验收：冷热表检索 + 虚拟角色分离 + 详情三库联查。

覆盖 10 项：
1. 写入一段对话并等抽取完成；
2. 普通联系人检索（热表）能命中；
3. 把某人归档（模拟热表裁剪）后，普通检索**仍能找到**（回退冷表）；
4. `include_archived=true` 时结果里能看到归档项；
5. 创建虚拟角色（历史/动漫人物名）；
6. 默认普通联系人列表**不含**虚拟角色；
7. `include_virtual=true` 时才能看到虚拟角色；
8. `/api/virtual_entities` 只返回虚拟角色；
9. 联系人详情返回 人设/关系/事件/记忆，且记忆原文含本人姓名；
10. 虚拟角色详情 `is_virtual=true`，真实联系人 `false`。

用法：
    python scripts/api_acceptance.py --base http://127.0.0.1:3200 --user acc1030
"""

import argparse
import datetime
import json
import sqlite3
import time
import urllib.error
import urllib.request

PASS = FAIL = 0


def req(method, url, payload=None, timeout=90):
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
            return (json.loads(body) if body.strip() else {}), e.code
        except Exception:
            return {"error": body}, e.code
    except Exception as e:
        return {"error": str(e)}, 0


def check(name, ok, detail=""):
    global PASS, FAIL
    if ok:
        PASS += 1
        print(f"  [PASS] {name}" + (f"  {detail}" if detail else ""))
    else:
        FAIL += 1
        print(f"  [FAIL] {name}" + (f"  {detail}" if detail else ""))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:3200")
    ap.add_argument("--user", default="acc1030")
    ap.add_argument("--db", default="./data_acc/shouxin.db")
    ap.add_argument("--timeout-min", type=int, default=6)
    args = ap.parse_args()
    B, U = args.base, args.user

    print("== 1. 写入与抽取 ==")
    convs = [
        f"我是{U}团队的负责人，后端张伟（外号大炮）负责排期，前端李娜（英文名 Nina）出交互稿。",
        "张伟的直属上级是技术总监罗军，李娜和张伟是同级。下周三之前要交付接口文档，张伟来写。",
    ]
    cids = []
    for t in convs:
        r, _ = req("POST", f"{B}/api/memory/add", {"conversation": t, "user_id": U})
        cid = (r.get("data") or {}).get("conversation_id")
        if cid:
            cids.append(cid)
    check("POST /api/memory/add", len(cids) == len(convs), f"{len(cids)}/{len(convs)}")

    deadline = time.time() + args.timeout_min * 60
    while time.time() < deadline:
        sts = []
        for c in cids:
            d, _ = req("GET", f"{B}/api/memory/extract_status/{c}", timeout=30)
            sts.append((d.get("data") or {}).get("status"))
        if all(s in ("completed", "failed") for s in sts):
            break
        time.sleep(10)
    check("抽取到达终态", all(s in ("completed", "failed") for s in sts), str(sts))

    print("== 2. 热表检索 ==")
    r, code = req("POST", f"{B}/api/memory/search/contacts", {"query": "张伟", "limit": 5, "user_id": U})
    rows = r.get("data") or []
    check("查询命中热表", code == 200 and any(c["name"] == "张伟" for c in rows), f"命中 {len(rows)}")

    print("== 3. 冷热回退：归档后仍可检索 ==")
    archived_ok = False
    try:
        con = sqlite3.connect(f"file:{args.db}?mode=rw", uri=True)
        cur = con.execute(
            "UPDATE contact_index SET is_archived=1 WHERE name='张伟' AND owner_id=?", (U,))
        con.commit()
        archived_ok = cur.rowcount > 0
        con.close()
    except Exception as e:
        print("   （直连 DB 失败，跳过归档模拟:", e, "）")

    if archived_ok:
        r2, _ = req("POST", f"{B}/api/memory/search/contacts", {"query": "张伟", "limit": 5, "user_id": U})
        hit2 = any(c["name"] == "张伟" for c in (r2.get("data") or []))
        check("归档后普通检索仍能找到（回退冷表）", hit2, f"命中 {len(r2.get('data') or [])}")
        r3, _ = req("POST", f"{B}/api/memory/search/contacts",
                    {"query": "张伟", "limit": 5, "user_id": U, "include_archived": True})
        check("include_archived 能看到归档项", any(c["name"] == "张伟" for c in (r3.get("data") or [])))
        # 复原，避免影响后续
        try:
            con = sqlite3.connect(f"file:{args.db}?mode=rw", uri=True)
            con.execute("UPDATE contact_index SET is_archived=0 WHERE name='张伟' AND owner_id=?", (U,))
            con.commit()
            con.close()
        except Exception:
            pass
    else:
        check("归档后普通检索仍能找到（回退冷表）", False, "归档模拟失败")

    print("== 4. 虚拟角色 ==")
    vs = [("孙悟空", "西游记"), ("哆啦A梦", "哆啦A梦"), ("ChatGPT", "OpenAI")]
    created = []
    for name, work in vs:
        r, code = req("POST", f"{B}/api/virtual_entities", {"name": name, "user_id": U})
        vid = (r.get("data") or {}).get("entity_id")
        created.append((name, vid))
        check(f"创建虚拟角色 {name}", code == 200 and bool(vid), str(code))
        _ = work

    r, _ = req("POST", f"{B}/api/memory/search/contacts", {"query": "", "limit": 100, "user_id": U})
    names = [c["name"] for c in (r.get("data") or [])]
    leaked = [n for n, _ in created if n in names]
    check("普通联系人列表不含虚拟角色（默认）", not leaked, f"泄漏 {leaked}")

    # 拆表后角色根本不在 contact_index 里，include_virtual 也不会把它们混进来
    r, _ = req("POST", f"{B}/api/memory/search/contacts",
               {"query": "", "limit": 100, "user_id": U, "include_virtual": True})
    names2 = [c["name"] for c in (r.get("data") or [])]
    still = [n for n, _ in created if n in names2]
    check("虚拟角色不在联系人表（角色有独立表）", not still, f"混入 {still}")

    # 同名不同作品必须能共存（这是拆表最直接的收益）
    r1, c1 = req("POST", f"{B}/api/virtual_entities",
                 {"name": "雅典娜", "source_work": "希腊神话", "category": "myth", "user_id": U})
    r2, c2 = req("POST", f"{B}/api/virtual_entities",
                 {"name": "雅典娜", "source_work": "圣斗士星矢", "category": "anime", "user_id": U})
    check("同名不同作品可共存（雅典娜 ×2）", c1 == 200 and c2 == 200,
          f"HTTP {c1}/{c2}")

    r, _ = req("GET", f"{B}/api/virtual_entities")
    vnames = [x.get("name") for x in (r.get("data") or [])]
    check("/api/virtual_entities 返回新建的角色", all(n in vnames for n, _ in created),
          f"共 {len(vnames)} 个")

    print("== 5. 联系人详情（人设/关系/事件/记忆） ==")
    r, _ = req("POST", f"{B}/api/memory/search/contacts", {"query": "张伟", "limit": 1, "user_id": U})
    rows = r.get("data") or []
    if rows:
        cid = rows[0]["contact_id"]
        d, code = req("GET", f"{B}/api/contacts/{cid}/detail?user_id={U}&event_limit=10&memory_limit=10")
        data = d.get("data") or {}
        check("详情接口 200", code == 200 and bool(data.get("contact")))
        check("详情含 related_contacts 字段", "related_contacts" in data)
        mems = data.get("memories") or []
        keys = [rows[0]["name"]] + (rows[0].get("aliases") or [])
        bad = [m for m in mems if not any(k and k in (m.get("snippet") or "") for k in keys)]
        check("记忆原文都包含本人姓名/别名", not bad, f"{len(mems)} 条，异常 {len(bad)}")
        check("真实联系人 is_virtual=false", (data.get("contact") or {}).get("is_virtual") is False)
    else:
        check("详情接口", False, "未找到张伟")

    # 角色的详情走独立接口（不再复用 /api/contacts/:id/detail）
    target = next(((n, i) for n, i in created if i), None)
    if target:
        n, vid = target
        d, code = req("GET", f"{B}/api/virtual_entities/{vid}")
        data = d.get("data") or {}
        check("虚拟角色详情（独立接口）", code == 200 and data.get("name") == n, f"{code}")
        check("角色详情带 source_work 字段", "source_work" in data)
        check("角色详情不含 contact 结构（与真人分离）", "background" not in data)
    else:
        check("虚拟角色详情（独立接口）", False, "创建失败")

    # ---- 同名不同人：编号唯一，姓名可重复 ----
    #
    # 这里直接写库而不走抽取，是因为"模型愿不愿意输出消歧标签"是模型能力问题，
    # 而这里要验收的是**系统设计**：同一个 owner 下允许两条同名记录、
    # 各有独立 UUID、检索能同时返回并靠 disambiguation 区分。
    con = sqlite3.connect(f"file:{args.db}?mode=rw", uri=True)
    now = datetime.datetime.now(datetime.timezone.utc).isoformat()
    pair = [(f"dup-bj-{U}", "北京"), (f"dup-sh-{U}", "上海")]
    try:
        for cid, tag in pair:
            con.execute(
                "INSERT OR REPLACE INTO contact_index "
                "(id,name,disambiguation,aliases,is_self,created_at,last_interaction_at,"
                "related_contact_ids,relationship_tags,relation_weight,background,notes,"
                "owner_type,owner_id,is_agent,is_virtual,is_archived,activity_score) "
                "VALUES (?,?,?,'[]',0,?,?,'[]','[]','','','',?,?,0,0,0,0.5)",
                (cid, "李雷", tag, now, now, "User", U),
            )
        con.commit()
    finally:
        con.close()

    r, _ = req("POST", f"{B}/api/memory/search/contacts",
               {"query": "李雷", "limit": 10, "user_id": U})
    rows = [x for x in (r.get("data") or []) if x.get("name") == "李雷"]
    check("同名两条都能查到（姓名不唯一）", len(rows) >= 2, f"{len(rows)} 条")
    ids = {x["contact_id"] for x in rows}
    check("同名者各有独立编号", len(ids) == len(rows), f"{len(ids)} 个编号")
    tags = {x.get("disambiguation") for x in rows}
    check("同名者带消歧标签", {"北京", "上海"} <= tags, f"{tags}")

    if rows:
        d, code = req("GET", f"{B}/api/contacts/{rows[0]['contact_id']}/detail?user_id={U}")
        got = ((d.get("data") or {}).get("contact") or {}).get("disambiguation")
        check("详情接口返回消歧标签", code == 200 and got == rows[0].get("disambiguation"), f"{got}")

    print(f"\n== 汇总：PASS={PASS}  FAIL={FAIL} ==")


if __name__ == "__main__":
    main()
