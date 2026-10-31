"""用全新语料评估"自然语言召回"的实用性。

原则：
- 语料与问法都不复用测试报告/ CHANGELOG 里的任何句子；
- 问法刻意避开原文关键词（"婚礼"→"办喜事"、"借钱"→"周转"），模拟真实回忆；
- 每个问题有唯一的 ground truth 关键词，命中即算召回。
"""
import json
import os
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

B = "http://127.0.0.1:3000"
DB = "data_eval2/shouxin.db"
USER = "eval2"
NOW = datetime.now(timezone.utc)

CORPUS = [
    ("舅妈打电话说表弟高宇高考完要来城里玩几天。", 5),
    ("楼下修车铺老陈说我的车刹车片该换了，报价四百。", 4),
    ("公司体检预约到12月3号上午，要求空腹。", 3),
    ("跟大学室友沈凯视频，他在成都做游戏，说年底可能回武汉。", 2),
    ("房东通知明年一月起房租涨三百，合同还在有效期。", 1),
    ("妈妈血压有点高，社区医生让少吃盐，两周后去复查。", 1),
    ("把年终总结初稿发给部门经理郑楠，她说下周二反馈。", 0),
    ("订了元旦去厦门的动车票，1月1号早上七点。", 0),
    ("邻居刘阿姨送了自家腌的咸菜，说要教我做法。", 2),
    ("手机屏幕摔裂了，问了维修店，换屏要六百。", 3),
]

# (问法, ground truth 关键词集合)
QUERIES = [
    ("谁要来城里玩？", {"高宇", "表弟"}),
    ("车子哪里要修？", {"刹车片"}),
    ("体检安排在什么时候？", {"12月3", "体检"}),
    ("我室友在哪个城市？", {"成都", "沈凯"}),
    ("房租有什么变动？", {"涨三百", "房租"}),
    ("妈妈身体有什么问题？", {"血压"}),
    ("总结交给谁了？", {"郑楠"}),
    ("元旦打算去哪？", {"厦门"}),
]


def post(p, pl=None, to=180, method="POST"):
    data = None if pl is None else json.dumps(pl).encode()
    r = urllib.request.Request(B + p, data=data,
                               headers={"Content-Type": "application/json"}, method=method)
    try:
        with urllib.request.urlopen(r, timeout=to) as resp:
            return resp.status, json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, {"_err": e.read().decode()[:200]}


def q(sql, args=()):
    c = sqlite3.connect(DB, timeout=20)
    try:
        return c.execute(sql, args).fetchall()
    finally:
        c.close()


def ingest():
    if os.path.exists(".eval2_ingested"):
        print("语料已写入过，跳过")
        return
    for text, days_ago in CORPUS:
        ts = (NOW - timedelta(days=days_ago)).isoformat()
        post("/api/memory/add", {"conversation": text, "user_id": USER, "timestamp": ts})
        time.sleep(0.3)
    open(".eval2_ingested", "w").close()
    print("已写入", len(CORPUS), "条")


def wait_ready(timeout=1200):
    """以字典库抽取状态为准：pending 与 processing 都为 0 才算完成。

    之前用"事件数稳定"判断，噪音句不产生事件时会提前放行，导致评估跑在抽取中途。
    """
    t0 = time.time()
    while time.time() - t0 < timeout:
        try:
            with urllib.request.urlopen(f"{B}/api/dictionary/stats?user_id={USER}", timeout=30) as r:
                counts = json.loads(r.read().decode())["data"]["status_counts"]
        except Exception:
            time.sleep(5)
            continue
        done = counts.get("completed", 0)
        pending = counts.get("pending", 0)
        processing = counts.get("processing", 0)
        if pending == 0 and processing == 0 and done >= len(CORPUS):
            return q("SELECT COUNT(*) FROM event_index WHERE owner_id=?", (USER,))[0][0]
        time.sleep(10)
    return q("SELECT COUNT(*) FROM event_index WHERE owner_id=?", (USER,))[0][0]


def collect_values(items):
    """把返回的 JSON 摊平成文本，便于匹配关键词"""
    out = []
    for it in items:
        if isinstance(it, dict):
            for k in ("name", "title", "summary", "content", "snippet", "description"):
                v = it.get(k)
                if isinstance(v, str):
                    out.append(v)
        elif isinstance(it, str):
            out.append(it)
    return " ".join(out)


def main(mode="smart"):
    ingest()
    n = wait_ready()
    print(f"抽取完成：{n} 条事件\n")

    rows = []
    for query, truth in QUERIES:
        status, body = post("/api/search/smart", {"query": query, "user_id": USER})
        d = (body.get("data") or {})
        text = collect_values(d.get("events", [])) + " " + collect_values(d.get("history", [])) \
               + " " + collect_values(d.get("contacts", []))
        hit = any(t in text for t in truth)
        where = []
        for k in ("contacts", "events", "history"):
            if any(t in collect_values(d.get(k, [])) for t in truth):
                where.append(k)
        rows.append((query, hit, where, d.get("matched_terms")))

    passed = sum(1 for r in rows if r[1])
    print(f"{'问题':<24} {'命中':<4} 来源")
    for query, hit, where, terms in rows:
        print(f"{query:<24} {'✓' if hit else '×':<4} {','.join(where) or '-':<18} terms={terms}")
    print(f"\n召回率: {passed}/{len(rows)} = {passed / len(rows):.0%}")
    return 0 if passed == len(rows) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "smart"))
