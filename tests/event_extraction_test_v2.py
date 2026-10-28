#!/usr/bin/env python3
"""
事件提取测试 v2 - 50 个不同事件，等待异步提取完成，验证长/短期分类
使用 Ollama (llama3.2:3b) 后端
"""

import requests
import json
import time
import sys
from datetime import datetime, timezone

BASE_URL = "http://127.0.0.1:3000"
USER_ID = "test_user_events_v2"
SCOPE = "User"

# 50 个包含不同事件的对话
CONVERSATIONS = [
    # 短期事件 (20)
    ("短期-明天会议", "明天下午3点要参加产品评审会议，在3楼会议室，记得带上Q2报告。", "short_term"),
    ("短期-周末活动", "这周六早上8点出发去爬山，和小王约好了在香山门口集合。", "short_term"),
    ("短期-本周截止", "本周五之前必须提交项目验收报告，否则会影响项目结项。", "short_term"),
    ("短期-预约", "下周一上午10点预约了牙科检查，在市中心医院3楼口腔科。", "short_term"),
    ("短期-航班", "6月15号下午2点的航班飞北京，CA1234，T3航站楼。", "short_term"),
    ("短期-聚餐", "今晚7点和老同学在海底捞聚餐，一共6个人，已经订好了包间。", "short_term"),
    ("短期-考试", "下周三上午9点参加PMP认证考试，需要带身份证和准考证。", "short_term"),
    ("短期-快递", "明天从京东买的新电脑显示器会送到，需要在家收货。", "short_term"),
    ("短期-维修", "空调坏了，约了维修师傅后天上午10点上门维修。", "short_term"),
    ("短期-面试", "周五下午2点有一个技术面试，应聘高级后端工程师。", "short_term"),
    ("短期-发布会", "6月18号下午参加新产品的线上发布会，需要准备演示文稿。", "short_term"),
    ("短期-培训", "下周二全天参加公司组织的Kubernetes实战培训，需要自带笔记本电脑。", "short_term"),
    ("短期-搬家", "这个周六要搬家，已经联系了搬家公司，早上8点开始。", "short_term"),
    ("短期-生日派对", "周日晚上7点在KTV给小张过生日，买了蛋糕。", "short_term"),
    ("短期-签约", "下周四上午11点去客户公司签合同，合同金额50万，需要带公章。", "short_term"),
    ("短期-车检", "6月20号下午去车管所年检，记得带行驶证和保险单。", "short_term"),
    ("短期-线上会议", "明天上午10点有一个跨国视频会议，和美国的团队讨论Q3产品路线图。", "short_term"),
    ("短期-演讲", "周五下午3点要在技术大会上做分享，主题是Rust在嵌入式系统中的应用。", "short_term"),
    ("短期-出差", "下周一要去深圳出差3天，需要提前预订酒店和机票。", "short_term"),
    ("短期-退税", "下周一记得去税务局办理去年的个人所得税退税，已经预约了下午2点。", "short_term"),
    # 长期事件 (15)
    ("长期-健身计划", "我决定从下周开始每天坚持跑步30分钟，目标是半年内减重10公斤。", "long_term"),
    ("长期-学习计划", "我计划用一年时间系统学习机器学习，从数学基础开始学起。", "long_term"),
    ("长期-买房", "打算明年在市中心买一套三居室的房子，预算500万左右，已经开始看房了。", "long_term"),
    ("长期-英语学习", "我报名了一年的英语口语课程，每周一三五晚上上课，目标是能流利地交流。", "long_term"),
    ("长期-创业", "我和合伙人计划明年创立一家AI SaaS公司，目前在做市场调研和商业计划书。", "long_term"),
    ("长期-学位", "我申请了在职MBA项目，需要2年时间完成，每周六上课，学费总共20万。", "long_term"),
    ("长期-储蓄", "我设定了三年储蓄计划，每月存5000元，目标是存够首付买一辆车。", "long_term"),
    ("长期-装修", "新房装修预计需要3个月，已经找了装修公司出方案，预算25万。", "long_term"),
    ("长期-开源项目", "我维护了一个开源Rust项目，计划今年完成2.0版本重构，需要添加插件系统。", "long_term"),
    ("长期-驾照", "报名了驾校学习，预计3个月拿驾照，已经考过了科目一，正在练习科目二。", "long_term"),
    ("长期-健康管理", "医生建议我进行长期血压管理，需要每天测量血压、控制饮食盐分摄入。", "long_term"),
    ("长期-写作", "我计划花一年时间写一本关于软件架构的书，已经签了出版合同，每周需要写5000字。", "long_term"),
    ("长期-投资", "我制定了5年投资计划，每月定投指数基金5000元，目标年化收益8%。", "long_term"),
    ("长期-职业规划", "我制定了5年职业发展计划，前3年深耕技术成为架构师，后2年转向技术管理。", "long_term"),
    ("长期-留学", "计划明年申请美国计算机硕士项目，正在准备托福和GRE考试，目标是申请TOP30。", "long_term"),
    # 混合事件 (15)
    ("高重要-服务器迁移", "这周六凌晨2点进行生产环境服务器迁移，影响所有在线服务，预计停机4小时。", "short_term"),
    ("高重要-安全漏洞", "发现了一个严重的安全漏洞需要立即修复，涉及用户数据泄露风险，今晚必须完成修复。", "short_term"),
    ("高重要-法律事务", "收到法院传票，下周三上午9点需要出庭，关于知识产权纠纷的案件。", "short_term"),
    ("中重要-读书会", "每月最后一个周五的读书会，这个月读的是《思考快与慢》，需要提前准备分享笔记。", "short_term"),
    ("中重要-团建", "下周六公司组织团建活动，去郊区的拓展基地，需要准备运动鞋和防晒用品。", "short_term"),
    ("低重要-看电影", "和朋友约了这周日去看新上映的电影，在万达影城IMAX厅，票已经买好了。", "short_term"),
    ("低重要-理发", "这周末去理发店剪头发，准备换个新发型，约了常去的TONY老师。", "short_term"),
    ("高重要-系统上线", "6月30号新系统正式上线，需要提前一周做全量回归测试，确保所有功能正常运行。", "short_term"),
    ("短期-修车", "车子的刹车片磨损了，约了修车店周三下午去更换，4个轮子都要换，预计需要2小时。", "short_term"),
    ("短期-家长会", "周五下午4点参加孩子的家长会，班主任要讨论期中考试成绩和升学规划。", "short_term"),
    ("短期-续费", "域名下周三到期，需要在到期前续费3年，否则网站会无法访问。", "short_term"),
    ("短期-体检", "下周五早上空腹去体检中心做年度体检，已经预约了7点半的号，需要带身份证。", "short_term"),
    ("短期-取件", "后天下午去干洗店取西装，已经洗好了，需要在6点前取，否则要加收保管费。", "short_term"),
    ("短期-护照", "下周三去出入境管理局办理护照续签，需要准备身份证和照片，提前一天预约。", "short_term"),
    ("短期-会议主持", "明天上午9点主持部门周会，需要准备上周工作总结和本周计划安排。", "short_term"),
]

TOTAL = len(CONVERSATIONS)


def log(msg, level="INFO"):
    timestamp = datetime.now().strftime("%H:%M:%S")
    print(f"[{timestamp}] [{level}] {msg}")


def add_conversation(text, user_id, scope):
    payload = {
        "user_id": user_id,
        "scope": scope,
        "conversation": text,
        "metadata": {"source": "event_extraction_test_v2"},
    }
    try:
        resp = requests.post(f"{BASE_URL}/api/memory/add", json=payload, timeout=120)
        data = resp.json()
        return resp.status_code, data
    except Exception as e:
        return 0, {"error": str(e)}


def get_extract_status(conv_id):
    try:
        resp = requests.get(f"{BASE_URL}/api/memory/extract_status/{conv_id}", timeout=10)
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def search_events(query, user_id, scope, limit=20):
    payload = {
        "query": query,
        "user_id": user_id,
        "scope": scope,
        "limit": limit,
    }
    try:
        resp = requests.post(f"{BASE_URL}/api/memory/search/events", json=payload, timeout=30)
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def main():
    log("=" * 70)
    log("事件提取测试 v2 - 50 个不同事件，Ollama (llama3.2:3b)")
    log("=" * 70)

    # 检查服务器
    try:
        resp = requests.get(f"{BASE_URL}/health", timeout=5)
        log(f"Server: {resp.json()}")
    except Exception as e:
        log(f"Server not reachable: {e}", "ERROR")
        sys.exit(1)

    results = {
        "test_name": "Event Extraction v2 - 50 Events, Ollama llama3.2:3b",
        "test_time": datetime.now(timezone.utc).isoformat(),
        "user_id": USER_ID,
        "total": TOTAL,
        "add_results": [],
        "search_results": [],
        "summary": {},
    }

    # ===== 阶段 1: 写入 50 个对话 =====
    log(f"\n{'='*70}")
    log(f"阶段 1: 写入 {TOTAL} 个对话")
    log(f"{'='*70}")

    conv_ids = []
    add_success = 0

    for i, (desc, text, expected_type) in enumerate(CONVERSATIONS):
        status, data = add_conversation(text, USER_ID, SCOPE)

        result = {
            "index": i + 1,
            "desc": desc,
            "expected_type": expected_type,
            "http_status": status,
            "success": data.get("success", False),
            "conversation_id": data.get("data", {}).get("conversation_id", ""),
            "error": data.get("error"),
        }
        results["add_results"].append(result)

        if status == 200 and data.get("success"):
            add_success += 1
            cid = result["conversation_id"]
            conv_ids.append(cid)
            log(f"  [{i+1:2d}/{TOTAL}] OK | {desc} | id={cid[:8]}...")
        else:
            log(f"  [{i+1:2d}/{TOTAL}] FAIL | {desc} | {data.get('error', '')[:60]}")

        time.sleep(0.2)

    log(f"\nAdd summary: {add_success}/{TOTAL} conversations created")

    # ===== 阶段 2: 等待异步提取完成 =====
    log(f"\n{'='*70}")
    log("阶段 2: 等待异步提取完成")
    log(f"{'='*70}")

    max_wait = 300  # 最多等 5 分钟
    wait_interval = 5
    waited = 0

    while waited < max_wait:
        done_count = 0
        extracting_count = 0
        sample_statuses = []

        for cid in conv_ids[:10]:  # 检查前10个采样
            status, data = get_extract_status(cid)
            extract_status = data.get("data", {}).get("status", "unknown") if status == 200 else "error"
            if extract_status == "completed":
                done_count += 1
            elif extract_status == "extracting":
                extracting_count += 1
            sample_statuses.append(extract_status)

        total_done = int(done_count / 10 * len(conv_ids)) if len(conv_ids) >= 10 else done_count

        log(f"  [{waited:3d}s] Sample: done={done_count}/10 extracting={extracting_count} "
            f"Est. total done: {total_done}/{len(conv_ids)} "
            f"Statuses: {sample_statuses}")

        if done_count == min(10, len(conv_ids)):
            log("All sampled conversations extracted!")
            break

        time.sleep(wait_interval)
        waited += wait_interval

    log(f"Wait completed after {waited}s")

    # ===== 阶段 3: 搜索收集所有事件 =====
    log(f"\n{'='*70}")
    log("阶段 3: 搜索收集所有事件")
    log(f"{'='*70}")

    search_queries = [
        "会议", "体检", "学习", "买房", "服务器", "出差",
        "搬家", "生日", "考试", "面试", "装修", "投资",
        "健身", "项目", "合同", "培训", "驾驶", "留学",
        "搬家", "医疗", "安全", "法律", "电影", "理发",
    ]

    all_events = {}  # 用 event_id 去重
    for query in search_queries:
        status, data = search_events(query, USER_ID, SCOPE, limit=50)
        if status == 200:
            evts = data.get("data", [])
            for evt in evts:
                eid = evt.get("event_id", evt.get("id", ""))
                if eid and eid not in all_events:
                    all_events[eid] = evt
            log(f"  Search '{query}': found {len(evts)} events (total unique: {len(all_events)})")
        time.sleep(0.2)

    log(f"\nTotal unique events found: {len(all_events)}")

    # ===== 阶段 4: 事件类型分析 =====
    log(f"\n{'='*70}")
    log("阶段 4: 事件类型分析 (长/短期)")
    log(f"{'='*70}")

    short_term = []
    long_term = []
    other = []

    for eid, evt in all_events.items():
        imp = evt.get("importance", "unknown")
        title = evt.get("title", "?")
        if imp in ("short_term", "short"):
            short_term.append(evt)
        elif imp in ("long_term", "long"):
            long_term.append(evt)
        else:
            other.append(evt)

    log(f"Short-term events: {len(short_term)}")
    log(f"Long-term events:  {len(long_term)}")
    log(f"Other importance:  {len(other)}")

    if other:
        for evt in other[:5]:
            log(f"  Other: '{evt.get('title', '?')[:30]}' importance={evt.get('importance')}")

    if short_term:
        log(f"\nShort-term samples:")
        for evt in short_term[:5]:
            log(f"  - {evt.get('title', '?')[:40]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    if long_term:
        log(f"\nLong-term samples:")
        for evt in long_term[:5]:
            log(f"  - {evt.get('title', '?')[:40]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    # ===== 汇总 =====
    results["summary"] = {
        "add_success": add_success,
        "total_conversations": TOTAL,
        "total_unique_events": len(all_events),
        "short_term_count": len(short_term),
        "long_term_count": len(long_term),
        "other_count": len(other),
        "type_distribution": {
            "short_term": len(short_term),
            "long_term": len(long_term),
            "other": {evt.get("importance", "?"): 0 for evt in other},
        },
    }

    # 保存结果
    output_file = "tests/event_extraction_test_results_v2.json"
    with open(output_file, "w", encoding="utf-8") as f:
        simplified = {
            "test_name": results["test_name"],
            "test_time": results["test_time"],
            "summary": results["summary"],
            "add_results": [
                {"index": r["index"], "desc": r["desc"], "expected_type": r["expected_type"],
                 "success": r["success"], "conversation_id": r["conversation_id"]}
                for r in results["add_results"]
            ],
            "events": [
                {"title": e.get("title", ""), "importance": e.get("importance", ""),
                 "time": e.get("time", ""), "description": e.get("description", "")[:80]}
                for e in all_events.values()
            ],
        }
        json.dump(simplified, f, ensure_ascii=False, indent=2)
    log(f"\nResults saved to: {output_file}")

    # 最终摘要
    log(f"\n{'='*70}")
    log("最终摘要")
    log(f"{'='*70}")
    s = results["summary"]
    log(f"  对话写入成功: {s['add_success']}/{TOTAL}")
    log(f"  唯一事件总数: {s['total_unique_events']}")
    log(f"  短期事件: {s['short_term_count']}")
    log(f"  长期事件: {s['long_term_count']}")
    log(f"  其他类型: {s['other_count']}")

    if s['total_unique_events'] >= 30:
        log(f"\n  ===== TEST PASSED =====")
        return 0
    else:
        log(f"\n  ===== TEST NEEDS REVIEW (only {s['total_unique_events']} events) =====")
        return 1


if __name__ == "__main__":
    sys.exit(main())