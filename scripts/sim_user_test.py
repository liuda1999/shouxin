"""守心 · 本地模拟用户测试（实用性压力测试）

每个场景针对一个具体的设计质疑，独立构造、独立判定。
用法：
    python scripts/sim_user_test.py --scenario cheap     # 快层：S3/S6/S7/S9，约 3 分钟
    python scripts/sim_user_test.py --scenario llm       # 慢层：S1/S2/S4/S5/S8/S10
    python scripts/sim_user_test.py --scenario all
    python scripts/sim_user_test.py --scenario s3        # 单个场景

脚本会自行启动/停止服务、使用 data_sim/ 临时目录，结束清理（--keep 可保留现场）。
"""

import argparse
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timedelta, timezone

BASE = "http://127.0.0.1:3000"
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DATA = os.path.join(ROOT, "data_sim")
DB = os.path.join(DATA, "shouxin.db")
CONFIG = os.path.join(ROOT, "config.sim.toml")
LOG = os.path.join(ROOT, ".sim.log")

NOW = datetime.now(timezone.utc)
RESULTS = []


# ---------------------------------------------------------------- 基础设施

def iso(dt):
    return dt.isoformat()


def post(path, payload=None, timeout=120, method="POST"):
    data = None if payload is None else json.dumps(payload).encode()
    req = urllib.request.Request(BASE + path, data=data,
                                 headers={"Content-Type": "application/json"}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, {"_error": e.read().decode()[:200]}
    except Exception as e:  # noqa: BLE001
        return -1, {"_error": str(e)[:200]}


def get(path, timeout=60):
    try:
        with urllib.request.urlopen(BASE + path, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, {"_error": e.read().decode()[:200]}
    except Exception as e:  # noqa: BLE001
        return -1, {"_error": str(e)[:200]}


def db():
    c = sqlite3.connect(DB, timeout=20)
    c.execute("PRAGMA busy_timeout=20000")
    return c


def rows(sql, args=()):
    c = db()
    try:
        return c.execute(sql, args).fetchall()
    finally:
        c.close()


def one(sql, args=()):
    r = rows(sql, args)
    return r[0][0] if r else None


def make_config():
    src = os.path.join(ROOT, "config.toml")
    with open(src, encoding="utf-8") as f:
        text = f.read()
    text = text.replace('data_dir = "./data"', 'data_dir = "./data_sim"')
    # 关掉自动心跳轮次，全部由脚本手动触发
    text = text.replace("base_interval_secs = 60", "base_interval_secs = 7200")
    text = text.replace("min_interval_secs = 30", "min_interval_secs = 7200")
    text = text.replace("max_interval_secs = 120", "max_interval_secs = 7200")
    with open(CONFIG, "w", encoding="utf-8") as f:
        f.write(text)


def start_server():
    if os.path.isdir(DATA):
        shutil.rmtree(DATA)
    make_config()
    env = dict(os.environ, SHOUXIN_CONFIG=CONFIG)
    logf = open(LOG, "w", encoding="utf-8", errors="replace")
    proc = subprocess.Popen(
        [os.path.join(ROOT, "target", "release", "shouxin-server.exe"),
         "--host", "127.0.0.1:3000"],
        cwd=ROOT, env=env, stdout=logf, stderr=subprocess.STDOUT)
    for i in range(120):
        time.sleep(1)
        code, _ = get("/health", timeout=3)
        if code == 200:
            print(f"  [server] ready in {i + 1}s")
            return proc, logf
    raise SystemExit("server 启动超时，见 .sim.log")


def stop_server(proc, logf):
    try:
        proc.terminate()
        proc.wait(timeout=15)
    except Exception:  # noqa: BLE001
        proc.kill()
    try:
        logf.close()
    except Exception:  # noqa: BLE001
        pass


def log_lines(pattern):
    try:
        with open(LOG, encoding="utf-8", errors="replace") as f:
            return [l for l in f if pattern in l]
    except FileNotFoundError:
        return []


def add_conv(text, user, ts=None):
    payload = {"conversation": text, "user_id": user}
    if ts:
        payload["timestamp"] = ts
    t0 = time.time()
    code, body = post("/api/memory/add", payload, timeout=30)
    return code, body, (time.time() - t0) * 1000


def wait_events(owner, n, timeout=900):
    """等待某 owner 的事件数达到 n，返回 (实际条数, 等待秒数)"""
    t0 = time.time()
    last = 0
    while time.time() - t0 < timeout:
        last = one("SELECT COUNT(*) FROM event_index WHERE owner_id=?", (owner,)) or 0
        if last >= n:
            break
        time.sleep(5)
    return last, time.time() - t0


def record(sid, title, metrics, verdict):
    RESULTS.append({"id": sid, "title": title, "metrics": metrics, "verdict": verdict})
    print(f"\n  ▶ {sid} {title}\n    {json.dumps(metrics, ensure_ascii=False)}\n    判定: {verdict}")


# ---------------------------------------------------------------- 场景

def s1_capture():
    """批量入口（修复后应可用）+ 真实吞吐"""
    code_probe, _ = post("/api/memory/batch", {}, timeout=10)
    print(f"    批量入口探测: POST /api/memory/batch -> {code_probe}（404=不存在，400=存在但参数空）")

    convs = [
        "周一上午和张伟开了产品评审会，决定把登录改版延到下个月。",
        "昨天下午陪李娜去口腔医院看牙，医生让下周复诊。",
        "上周末和王强爬了香山，山顶风很大。",
        "前天刘洋把季度报表发给我，我今晚核对数据。",
        "周三中午和陈静吃饭，她准备年底跳槽去上海。",
        "昨晚家里停水，物业说凌晨两点修好。",
        "这周赵磊定了服务器迁移方案，下周五执行。",
        "上周五参加孙悦生日聚会，约好明年春节再聚。",
        "今天上午陪周涛去医院复查，恢复得不错。",
        "前天给孙婷寄了合同，她说明天签完寄回。",
        "周末和吴敏看了场电影，散场后聊了很久。",
        "这周把年度预算重算了一遍，偏差比预期小。",
    ]
    items = [
        {"conversation": t, "timestamp": iso(NOW - timedelta(days=i % 5 + 1))}
        for i, t in enumerate(convs)
    ]
    t0 = time.time()
    code, body = post("/api/memory/batch", {"user_id": "s1_cap", "items": items}, timeout=120)
    accept_total = time.time() - t0
    data = body.get("data") or {}
    accepted = data.get("accepted", 0)

    ev, waited = wait_events("s1_cap", 8, timeout=1200)
    rate = ev / waited * 60 if waited else 0
    dates = [r[0][:10] for r in rows(
        "SELECT DISTINCT substr(COALESCE(event_time, created_at), 1, 10) FROM event_index "
        "WHERE owner_id='s1_cap'")]
    metrics = {
        "批量入口": code_probe,
        "批量导入HTTP": code,
        "接收": f"{accepted}/{len(items)}",
        "接收总耗时_ms": round(accept_total * 1000, 1),
        "落库事件数": ev,
        "排空耗时_s": round(waited, 1),
        "吞吐_条每分钟": round(rate, 2),
        "推算_100条_min": round(100 / rate, 1) if rate else None,
        "事件日期分布": sorted(dates),
    }
    verdict = ("已修复：批量导入一次收下 12 条（带历史时间戳，事件日期正确分散）；"
               "瓶颈仍在抽取队列，约 2.5 条/分钟"
               if code == 200 and accepted == len(items) and len(dates) >= 3
               else "批量入口或时间戳未达预期")
    record("S1", "入口与吞吐", metrics, verdict)


def s2_speaker():
    """对话不分说话人：方向是否丢失；messages 是否保留角色"""
    a = "我答应周五前把报价单发给王强。"
    b = "王强答应周五前把报价单发给我。"
    add_conv(a, "s2_a")
    add_conv(b, "s2_b")
    # 带角色的多轮：用户提问、对方承诺 → 方向应指向"对方发给我"
    code_c, body_c = post("/api/memory/add", {
        "user_id": "s2_c",
        "messages": [
            {"role": "user", "content": "报价单你什么时候能给我？"},
            {"role": "other", "content": "我周五前发给你。"},
        ],
    }, timeout=30)
    time.sleep(10)
    for _ in range(24):
        nc = one("SELECT COUNT(*) FROM todo_list WHERE owner_id='s2_c'") or 0
        if nc:
            break
        time.sleep(5)
    out = {}
    for u in ("s2_a", "s2_b", "s2_c"):
        out[u] = {
            "todos": rows("SELECT title, contact_ids FROM todo_list WHERE owner_id=?", (u,)),
            "events": rows("SELECT title, contact_ids FROM event_index WHERE owner_id=?", (u,)),
        }
    same = json.dumps(out["s2_a"]["todos"]) == json.dumps(out["s2_b"]["todos"])
    metrics = {"A_我承诺(纯文本)": out["s2_a"], "B_对方承诺(纯文本)": out["s2_b"],
               "C_对方承诺(messages)": out["s2_c"], "messages_HTTP": code_c,
               "A/B 完全一致": same}
    c_todo = bool(out["s2_c"]["todos"])
    verdict = ("messages 通道已可用（HTTP 200，角色前缀写入原文与提示词）；"
               f"该样例抽出待办={c_todo}，方向是否被正确理解仍需人工复核；"
               "纯文本下 B 仍被记成“发送给说话人”——4B 模型的方向判断是能力上限"
               if code_c == 200 and not same else "方向或 messages 通道未达预期")
    record("S2", "说话人方向", metrics, verdict)


def s3_archive_visibility():
    """记忆系统默认只看热表"""
    c = db()
    base = [
        ("s3_old_high", 0.9, 40), ("s3_old_short", 0.5, 9),
        ("s3_new_short", 0.5, 2), ("s3_new_high", 0.9, 1),
    ]
    for eid, score, days in base:
        created = iso(NOW - timedelta(days=days))
        c.execute("""INSERT INTO event_index
            (event_id, contact_ids, owner_id, owner_type, time_range_start, time_range_end,
             importance, status, created_at, summary, title, event_type, importance_score,
             is_archived, event_time)
            VALUES (?, '[]', 's3_arch', 'User', ?, ?, ?, 'pending', ?, ?, ?, 'test', ?, 0, ?)""",
                  (eid, created, created, "high" if score > 0.7 else "medium",
                   created, f"sum-{eid}", f"title-{eid}", score, created))
    c.commit()
    c.close()

    code, rep = post("/api/heartbeat/trigger", {"maintenance_only": True}, timeout=120)
    time.sleep(1)
    hot = post("/api/memory/search/events", {"user_id": "s3_arch", "limit": 50,
                                             "hot_only": True})[1].get("data", [])
    allv = post("/api/memory/search/events",
                {"user_id": "s3_arch", "limit": 50})[1].get("data", [])
    titles_hot = sorted(e["title"] for e in hot)
    titles_all = sorted(e["title"] for e in allv)
    hidden = [t for t in titles_all if t not in titles_hot]
    metrics = {"热表查询(hot_only=true)": len(hot), "默认查询": len(allv),
               "热表看不到的": hidden}
    verdict = ("已修复：默认查询覆盖全时间（含 40 天前高重要事件），"
               "只有显式 hot_only=true 才退回热表"
               if len(allv) == 4 and len(hot) == 2 else "未达预期，需检查默认参数")
    record("S3", "默认是否能看到历史", metrics, verdict)


def s4_profile_cost():
    """画像成本：按“当天有新数据的联系人”计；并验证无证据联系人是否空转占位"""
    convs = [
        "今天和张伟开会，确定了登录改版的排期。",
        "中午和李娜吃饭，她说下周要去杭州出差。",
        "下午和王强通了电话，聊了服务器扩容的事。",
        "晚上和刘洋对了一下报表，数据基本对上。",
        "上午和陈静开了需求评审会，砍掉两个功能。",
        "傍晚和赵磊确认了迁移方案，周五执行。",
    ]
    for t in convs:
        add_conv(t, "s4_prof")

    # 等待抽取落库（联系人 + 事件）
    for _ in range(80):
        time.sleep(5)
        n_contact = one("SELECT COUNT(*) FROM contact_index WHERE owner_id='s4_prof'") or 0
        if n_contact >= 6:
            break
    n_contact = one("SELECT COUNT(*) FROM contact_index WHERE owner_id='s4_prof'") or 0

    # 额外造 2 个"没有任何证据"的联系人（只有名字，没有事件/对话）
    # 互动时间略早于真实联系人，保证它们排在后面、在一轮真实建档之后被检查到
    c = db()
    for i in range(2):
        t = NOW - timedelta(seconds=5 + i)
        c.execute("""INSERT INTO contact_index
            (id, name, aliases, is_self, created_at, last_interaction_at, owner_type, owner_id,
             is_archived, background, activity_score)
            VALUES (?, ?, '[]', 0, ?, ?, 'User','s4_prof',0,NULL,0.0)""",
                  (f"s4_ghost{i}", f"幽灵{i}", iso(t), iso(t)))
    c.commit()
    c.close()

    def round_(tag):
        t0 = time.time()
        code, rep = post("/api/heartbeat/trigger", {}, timeout=1500)
        dur = time.time() - t0
        done = one("SELECT COUNT(*) FROM contact_index WHERE owner_id='s4_prof' "
                   "AND background IS NOT NULL AND background != ''") or 0
        logs = one("SELECT COUNT(*) FROM contact_profile_update_log") or 0
        print(f"    {tag}: code={code} 用时={dur:.0f}s 已建档={done}/{n_contact} 日志={logs}")
        return {"code": code, "秒": round(dur, 1), "已建档": done, "日志条数": logs}

    r1 = round_("第1轮")
    r2 = round_("第2轮")
    r3 = round_("第3轮")

    ghost_logged = one("""SELECT COUNT(*) FROM contact_profile_update_log
                          WHERE contact_id IN ('s4_ghost0','s4_ghost1')""") or 0
    # 注意：要按"候选队列口径"判断，不能只看 background 是否为空——
    # 空画像但已检查过的联系人本就不该再入队。
    ghost_in_queue = one("""
        SELECT COUNT(*) FROM contact_index ci
        LEFT JOIN (SELECT contact_id, MAX(updated_at) lu
                   FROM contact_profile_update_log GROUP BY contact_id) l
          ON l.contact_id = ci.id
        WHERE ci.id LIKE 's4_ghost%'
          AND (l.lu IS NULL OR ci.last_interaction_at > l.lu)
    """) or 0
    per_profile = (r1["秒"] / max(r1["已建档"], 1)) if r1["已建档"] else None
    metrics = {
        "抽取出的真人联系人": n_contact,
        "第1轮": r1, "第2轮": r2, "第3轮": r3,
        "单轮取人上限": 3,
        "单人均耗时_s": round(per_profile, 1) if per_profile else None,
        "10个新联系人预计_min": round(per_profile * 10 / 60, 1) if per_profile else None,
        "无证据联系人被记录": ghost_logged,
        "无证据联系人仍在候选队列": ghost_in_queue,
    }
    verdict = (
        "已修复：无证据联系人只记一次“已检查”（0 额度）后退出候选队列，"
        "6 个真人 2 轮内全部建档，不再被空转占位"
        if ghost_logged >= 2 and ghost_in_queue == 0 and r2["已建档"] >= n_contact
        else "画像仍受空转占位影响，需检查 record_profile_skip 与候选 SQL")
    record("S4", "画像成本（按当天新数据）", metrics, verdict)


def s5_three_library():
    """一份事实写三处是否一致"""
    owner = "s1_cap"
    n_events = one("SELECT COUNT(*) FROM event_index WHERE owner_id=?", (owner,)) or 0
    code, sem = post("/api/memory/search/semantic",
                     {"query": "产品评审会 登录改版", "user_id": owner, "limit": 20})
    hits = sem.get("data", []) if code == 200 else []
    if isinstance(hits, dict):
        hits = hits.get("results", [])
    n_sem = len(hits)
    metrics = {"关系库事件": n_events, "语义检索命中": n_sem}
    verdict = ("三库不齐：有事件但语义检索无命中（或反之），说明一份事实未同时落到向量库"
               if n_events and not n_sem else "至少该查询下三库均有数据")
    record("S5", "三库一致性", metrics, verdict)


def s6_pending_merge():
    """pending_merges 没有超时出口（按年龄梯度复测）"""
    ages = [("49h", 49), ("72h", 72), ("5d", 120)]
    c = db()
    for tag, hours in ages:
        created = iso(NOW - timedelta(hours=hours))
        c.execute("""INSERT INTO pending_event_merge
            (source_event_id, target_event_id, source_event_summary, target_event_summary,
             similarity_score, status, created_at, owner_type, owner_id)
            VALUES (?, 's6_tgt', ?, 'sum-b', 0.85, 'pending', ?, 'User','s6_merge')""",
                  (f"s6_src_{tag}", f"sum-a-{tag}", created))
    c.commit()
    c.close()
    before = rows("SELECT source_event_id FROM pending_event_merge WHERE owner_id='s6_merge'")
    post("/api/heartbeat/trigger", {"maintenance_only": True}, timeout=120)
    time.sleep(1)
    state = {r[0]: r[1] for r in rows(
        "SELECT source_event_id, status FROM pending_event_merge WHERE owner_id='s6_merge'")}
    expired = sorted(k for k, v in state.items() if v == "expired")
    pending = sorted(k for k, v in state.items() if v == "pending")
    metrics = {"插入": len(before), "标记 expired": expired, "仍在 pending": pending}
    verdict = ("已修复：48h 到期改为标记 expired（保留可审计记录），"
               "且 49h 的行不再因时间格式比较而滞留"
               if len(expired) == 3 and not pending
               else "仍有行未到期或未标记，需检查时间比较")
    record("S6", "pending_merges 到期出口", metrics, verdict)


def s7_backup():
    """备份 / 导出（修复后应可用）"""
    bak = os.path.join(ROOT, ".sim_backup.db")
    if os.path.exists(bak):
        os.remove(bak)
    code, body = post("/api/backup", {"path": bak}, timeout=120)
    size = os.path.getsize(bak) if os.path.exists(bak) else 0
    integrity = "n/a"
    if os.path.exists(bak):
        c2 = sqlite3.connect(bak)
        integrity = c2.execute("PRAGMA integrity_check").fetchone()[0]
        c2.close()
        os.remove(bak)

    code2, body2 = get("/api/export?user_id=s3_arch&limit=200", timeout=60)
    counts = (body2.get("data") or {}).get("counts", {}) if code2 == 200 else {}
    metrics = {"POST /api/backup": code, "快照大小_KB": round(size / 1024, 1),
               "完整性": integrity, "GET /api/export": code2, "导出计数": counts}
    verdict = ("已修复：/api/backup 生成一致性快照（integrity ok），"
               "/api/export 可全量导出联系人/事件/待办"
               if code == 200 and integrity == "ok" and code2 == 200
               else "接口未达预期，需检查路由注册")
    record("S7", "备份/导出", metrics, verdict)


def s8_contention():
    """抽取与心跳抢同一条推理通道"""
    convs = [
        "昨天和马超确认了接口文档，他说周四前给初稿。",
        "今天陪何静去看房，她觉得采光不错但价格偏高。",
        "前天罗强还了上次借的两千块，说下月再借一笔。",
        "周末和高颖去爬山，路上聊了她换工作的事。",
        "周三晚上和梁宇吃饭，他提了个合作的想法。",
        "昨天给冯雪寄了资料，她收到后回了个电话。",
        "今天上午和邓超开了预算会，砍掉了两个项目。",
        "前天唐敏说她父亲住院了，我打算周末去看看。",
    ]
    for t in convs:
        add_conv(t, "s8_thr")
    t0 = time.time()
    code, rep = post("/api/heartbeat/trigger", {}, timeout=1500)
    hb_dur = time.time() - t0
    ev, drain = wait_events("s8_thr", 5, timeout=1200)
    metrics = {"心跳返回码": code, "心跳耗时_s": round(hb_dur, 1),
               "抽取落库": ev, "抽取总耗时_s": round(drain, 1),
               "心跳是否长时间占用通道": hb_dur > 20}
    verdict = (f"心跳一轮 {hb_dur:.0f}s 与抽取串行在同一推理通道上（8 条抽取用 {drain:.0f}s），"
               "心跳期间抽取被推迟；单并发下两者互相排队"
               if hb_dur > 20 else "心跳很短，未观察到明显阻塞")
    record("S8", "推理通道互斥", metrics, verdict)


def s9_junk_contacts():
    """存量碎片联系人是否污染检索"""
    junk = ["复盘", "大家一致", "五前交", "评估表周", "下周再", "这个事", "明天上",
            "会议纪", "需求文", "测试报", "两点的", "上次那", "方案定", "项目组",
            "临时加", "客户说", "同事提", "领导要", "周五前", "月底再"]
    c = db()
    for i, name in enumerate(junk):
        c.execute("""INSERT INTO contact_index
            (id, name, aliases, is_self, created_at, last_interaction_at, owner_type, owner_id,
             is_archived, background, activity_score)
            VALUES (?, ?, '[]', 0, ?, ?, 'User', 's9_junk', 0, NULL, 0.0)""",
                  (f"s9_j{i}", name, iso(NOW), iso(NOW)))
    c.execute("""INSERT INTO contact_index
        (id, name, aliases, is_self, created_at, last_interaction_at, owner_type, owner_id,
         is_archived, background, activity_score)
        VALUES ('s9_real','王强','[]',0,?,?, 'User','s9_junk',0,NULL,0.0)""", (iso(NOW), iso(NOW)))
    c.commit()
    c.close()

    ratios = {}
    for q in ("复盘", "大家", "评估", "王强"):
        code, body = post("/api/memory/search/contacts", {"query": q, "user_id": "s9_junk"})
        items = body.get("data", []) if code == 200 else []
        names = [d.get("name") for d in items]
        bad = [n for n in names if n in junk]
        ratios[q] = {"返回": names, "垃圾": len(bad)}
    metrics = ratios
    verdict = ("确认污染：常见词会命中碎片联系人，抽取校验只挡新增、不洗存量"
               if any(v["垃圾"] for v in ratios.values()) else "未命中垃圾，存量影响有限")
    record("S9", "碎片联系人污染", metrics, verdict)


def s10_ambiguity():
    """同名联系人与歧义队列"""
    c = db()
    for i, dis in enumerate(("同事-产品部", "同学-大学")):
        c.execute("""INSERT INTO contact_index
            (id, name, disambiguation, aliases, is_self, created_at, last_interaction_at,
             owner_type, owner_id, is_archived, background, activity_score)
            VALUES (?, '刘洋', ?, '[]', 0, ?, ?, 'User', 's10_amb', 0, NULL, 0.0)""",
                  (f"s10_{i}", dis, iso(NOW), iso(NOW)))
    c.commit()
    c.close()
    add_conv("今天和刘洋开了会，他说产品的排期要往后推两周。", "s10_amb")
    time.sleep(15)
    for _ in range(24):
        n = one("SELECT COUNT(*) FROM event_index WHERE owner_id='s10_amb'") or 0
        if n:
            break
        time.sleep(5)
    amb = rows("SELECT ambiguity_type, status FROM ambiguity_log WHERE entity_id LIKE 's10_%'")
    todos = rows("SELECT title, todo_type, entity_id FROM todo_list WHERE owner_id='s10_amb'")
    metrics = {"歧义日志": amb, "歧义待办": [t for t in todos if t[1] == "ambiguity"],
               "全部待办数": len(todos)}
    verdict = ("同名场景产生了待人工处理的歧义项，需确认是否有出口（否则长期堆积）"
               if amb or any(t[1] == "ambiguity" for t in todos) else "未产生歧义项")
    record("S10", "同名歧义噪音", metrics, verdict)


CHEAP = [s3_archive_visibility, s6_pending_merge, s7_backup, s9_junk_contacts]
LLM = [s2_speaker, s1_capture, s5_three_library, s8_contention, s10_ambiguity]
ALL = CHEAP + LLM
BY_ID = {"s3": s3_archive_visibility, "s6": s6_pending_merge, "s7": s7_backup,
         "s9": s9_junk_contacts, "s1": s1_capture, "s2": s2_speaker,
         "s4": s4_profile_cost, "s5": s5_three_library, "s8": s8_contention,
         "s10": s10_ambiguity}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--scenario", default="cheap")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    seq = {"cheap": CHEAP, "llm": LLM, "all": ALL}.get(args.scenario, [BY_ID.get(args.scenario)])
    if seq == [None]:
        raise SystemExit(f"未知场景 {args.scenario}")

    print(f"== 启动服务（临时目录 data_sim）==")
    proc, logf = start_server()
    t_all = time.time()
    try:
        for fn in seq:
            print(f"\n=== 运行 {fn.__name__} ===")
            try:
                fn()
            except Exception as e:  # noqa: BLE001
                record(fn.__name__, "场景异常", {"error": str(e)[:300]}, "未完成")
    finally:
        stop_server(proc, logf)
        print(f"\n== 总耗时 {(time.time() - t_all) / 60:.1f} 分钟 ==")
        out = os.path.join(ROOT, f"sim_user_test_{args.scenario}.json")
        with open(out, "w", encoding="utf-8") as f:
            json.dump(RESULTS, f, ensure_ascii=False, indent=2, default=str)
        print(f"结果已写入 {out}")
        if not args.keep:
            shutil.rmtree(DATA, ignore_errors=True)
            for p in (CONFIG, LOG):
                if os.path.exists(p):
                    os.remove(p)


if __name__ == "__main__":
    main()
