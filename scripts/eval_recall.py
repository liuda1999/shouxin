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
DB = "data_eval/shouxin.db"
USER = "eval"
NOW = datetime.now(timezone.utc)

CORPUS = [
    ("下午去看了套二手房，中介说业主急售，价格还能再谈。", 4),
    ("表姐陈露下个月8号办婚礼，让我提前一天过去帮忙。", 4),
    ("周明说他妈住院了，这两天要请假回老家。", 4),
    ("跟老徐他们打球，聊到他想自己开个工作室。", 3),
    ("公司通知下季度开始双周发一次工资，财务在群里发了说明。", 3),
    ("晚上整理书架，翻出大学时的日记，看了半本。", 3),
    ("徐建国（老徐）问我借两万周转，说月底前还。", 2),
    ("体检报告出来了，甲状腺有个小结节，医生让三个月后复查。", 2),
    ("项目上线延期到11月20号，客户那边我去解释。", 2),
    ("邻居家小孩考上大学了，晚上送了喜糖过来。", 1),
    ("把阳台的花搬进屋，天气预报说周末降温。", 1),
    ("答应陈露婚礼上帮她拍照，记得带相机和备用电池。", 1),
    ("老板说今年的年终奖要跟新项目挂钩，具体方案下周五前公布。", 0),
    ("老徐把上次借的钱还了一半，剩下一万说下个月给。", 0),
]

# (问法, ground truth 关键词集合, 期望来源)
QUERIES = [
    ("谁最近要办喜事？", {"陈露", "婚礼"}),
    ("有没有人找我周转？", {"借", "两万", "徐建国"}),
    ("体检有没有查出什么？", {"结节", "甲状腺"}),
    ("那个项目推迟到几号了？", {"11月20", "延期"}),
    ("年终奖的事定了吗？", {"年终奖"}),
    ("谁家里有人住院？", {"周明", "住院"}),
    ("老徐最近在忙什么？", {"老徐", "徐建国", "工作室"}),
    ("我答应帮谁拍照？", {"拍照", "陈露"}),
    ("房子的事有眉目吗？", {"二手房", "业主"}),
    ("最近身体上有什么要留意的？", {"结节", "复查"}),
    ("借出去的钱还清了吗？", {"还了一半", "一万", "借"}),
    ("工资发放有什么变化？", {"双周", "工资"}),
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
    if os.path.exists(".eval_ingested"):
        print("语料已写入过，跳过")
        return
    for text, days_ago in CORPUS:
        ts = (NOW - timedelta(days=days_ago)).isoformat()
        post("/api/memory/add", {"conversation": text, "user_id": USER, "timestamp": ts})
        time.sleep(0.3)
    open(".eval_ingested", "w").close()
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
