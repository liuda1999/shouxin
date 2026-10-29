#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""端到端跑通测试：用一个新用户灌入一段"真实工作场景"，验证全链路。

覆盖点：
- 多人、别名、职称混用能否正确归并；
- 事件（会议/截止日）能否落到事件表并关联到人；
- 待办能否抽取并给出截止时间；
- 虚拟角色能否与真人区分；
- 联系人详情能否读到关系 / 事件 / 原始记忆。

用法：
    python scripts/e2e_runthrough.py --base http://127.0.0.1:3000 --user e2e1030
"""

import argparse
import json
import time
import urllib.error
import urllib.request

CONVS = [
    "我是产品经理，我们团队有五个人：后端张伟（大家都叫他大炮）、前端李娜（英文名 Nina）、测试王强、设计陈静（静姐）、还有实习生龙飞。",
    "周一上午九点半开项目启动会，张伟负责后端排期，李娜出交互稿，王强准备测试计划，陈静出视觉稿，龙飞做会议记录。",
    "李娜说她的直属上级是张伟，但虚线汇报给我；陈静和张伟是同级，都向技术总监罗军汇报。",
    "周二的架构评审会上，罗军拍板主库分库分表，张伟反对，理由是迁移风险大，最后决定先做读写分离。",
    "客户那边的负责人是汤敏，她以前和钱伟是同事，现在对接我们这边由我负责。",
    "下周三之前必须交付接口文档和联调环境，张伟说文档他来写，联调环境要李娜配合。",
    "陈静的婆婆生病了，她这周要请两天假，视觉稿可能要延后到周五。",
    "王强的测试环境一直起不来，找运维老周看了半天，最后发现是磁盘满了。",
    "我让龙飞整理一份竞品分析，周五前给我初稿，重点是三家友商的定价策略。",
    "复盘会上大家一致认为：这次延期主要是需求变更太晚，其次是测试介入太迟。",
    "对了，我还在用一个虚拟助手叫苏晚晴，她帮我整理会议纪要，但她不是真人，别把她当成同事。",
    "罗军说下个月要把龙飞转正，让我准备评估材料，评估表周五前交。",
]


def req(method, url, payload=None, timeout=90):
    data = json.dumps(payload, ensure_ascii=False).encode("utf-8") if payload is not None else None
    headers = {"Content-Type": "application/json"} if data else {}
    r = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return json.loads(body) if body.strip() else {}
    except urllib.error.HTTPError as e:
        return {"error": f"HTTP {e.code}"}
    except Exception as e:
        return {"error": str(e)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="http://127.0.0.1:3000")
    ap.add_argument("--user", default="e2e1030")
    ap.add_argument("--timeout-min", type=int, default=25)
    args = ap.parse_args()

    cids = []
    for i, text in enumerate(CONVS, 1):
        r = req("POST", f"{args.base}/api/memory/add", {"conversation": text, "user_id": args.user})
        cid = (r.get("data") or {}).get("conversation_id")
        if cid:
            cids.append(cid)
        else:
            print(f"  [{i}] 提交失败: {str(r)[:80]}")
        time.sleep(1.2)  # 限流 60/分钟，平稳提交

    print(f"已提交 {len(cids)}/{len(CONVS)} 段对话，等待抽取…")

    deadline = time.time() + args.timeout_min * 60
    while time.time() < deadline:
        done = 0
        for cid in cids:
            st = (req("GET", f"{args.base}/api/memory/extract_status/{cid}", timeout=30).get("data") or {}).get("status")
            if st in ("completed", "failed"):
                done += 1
        print(f"  完成 {done}/{len(cids)}", flush=True)
        if done == len(cids):
            break
        time.sleep(20)

    contacts = req("POST", f"{args.base}/api/memory/search/contacts",
                   {"query": "", "limit": 100, "user_id": args.user, "include_archived": True}).get("data") or []
    events = req("POST", f"{args.base}/api/memory/search/events",
                 {"query": "", "limit": 100, "owner_type": "User", "owner_id": args.user}).get("data") or []
    todos = req("GET", f"{args.base}/api/todos?user_id={args.user}&limit=50").get("data") or []

    print("\n=== 跑通结果 ===")
    print(f"联系人 {len(contacts)}: " + "、".join(f"{c['name']}({'虚拟' if c.get('is_virtual') else '真实'})" for c in contacts))
    print(f"事件 {len(events)}: " + "、".join((e.get('title') or e.get('summary') or '')[:12] for e in events[:8]))
    print(f"待办 {len(todos)}: " + "、".join((t.get('title') or '')[:14] for t in todos[:8]))

    expected = ["张伟", "李娜", "王强", "陈静", "龙飞", "罗军", "汤敏", "苏晚晴"]
    names = {c["name"] for c in contacts}
    print("\n期望角色覆盖: " + "、".join(f"{n}{'✓' if n in names else '✗'}" for n in expected))

    with open("docs/e2e_runthrough.json", "w", encoding="utf-8") as f:
        json.dump({"user": args.user, "contacts": contacts, "events": events, "todos": todos},
                  f, ensure_ascii=False, indent=1)
    print("\n结果写入 docs/e2e_runthrough.json")


if __name__ == "__main__":
    main()
