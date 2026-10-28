#!/usr/bin/env python3
"""
事件提取测试 - 通过对话写入 50 个不同事件，验证提取正确性和长/短期分类
使用 Ollama (qwen3.5:4b) 后端
"""

import requests
import json
import time
import sys
from datetime import datetime, timezone

BASE_URL = "http://127.0.0.1:3000"
USER_ID = "test_user_events_50"
SCOPE = "User"

# 50 个包含不同事件的对话，覆盖长短期、不同重要性
CONVERSATIONS = [
    # ---- 短期事件 (short_term) - 具体日期、一次性活动 ----
    {
        "desc": "短期-明天会议",
        "text": "明天下午3点要参加产品评审会议，在3楼会议室，记得带上Q2报告。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-周末活动",
        "text": "这周六早上8点出发去爬山，和小王、小李约好了，在香山门口集合。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-本周截止",
        "text": "本周五之前必须提交项目验收报告，否则会影响项目结项。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-预约",
        "text": "下周一上午10点预约了牙科检查，在市中心医院3楼口腔科。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-航班",
        "text": "6月15号下午2点的航班飞北京，CA1234，T3航站楼，记得提前2小时到机场。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-聚餐",
        "text": "今晚7点和老同学在海底捞聚餐，一共6个人，已经订好了包间。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-考试",
        "text": "下周三上午9点参加PMP认证考试，考点在市中心考试中心，需要带身份证和准考证。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-快递",
        "text": "明天从京东买的新电脑显示器会送到，需要在家收货，下午3点前送到。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-维修",
        "text": "空调坏了，约了维修师傅后天上午10点上门维修，说是压缩机问题需要更换。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-面试",
        "text": "周五下午2点有一个技术面试，应聘高级后端工程师，准备了一些系统设计问题。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-发布会",
        "text": "6月18号下午参加新产品的线上发布会，需要提前准备演示文稿和技术问答。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-培训",
        "text": "下周二全天参加公司组织的Kubernetes实战培训，在培训中心3楼，需要自带笔记本电脑。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-搬家",
        "text": "这个周六要搬家，已经联系了搬家公司，早上8点开始，预计下午3点搬完。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-生日派对",
        "text": "周日晚上7点在KTV给小张过生日，买了蛋糕，已经通知了10个同事参加。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-签约",
        "text": "下周四上午11点去客户公司签合同，合同金额50万，需要带公章和营业执照。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-车检",
        "text": "6月20号下午去车管所年检，先预约了检车服务，记得带行驶证和保险单。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-线上会议",
        "text": "明天上午10点有一个跨国视频会议，和美国的团队讨论Q3产品路线图，需要准备英文PPT。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-取件",
        "text": "后天下午去干洗店取西装，已经洗好了，需要在6点前取，否则要加收保管费。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-演讲",
        "text": "周五下午3点要在技术大会上做分享，主题是Rust在嵌入式系统中的应用，准备了40分钟的内容。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-出差",
        "text": "下周一要去深圳出差3天，拜访3家供应商，需要提前预订酒店和机票。",
        "expected_type": "short_term",
    },
    # ---- 长期事件 (long_term) - 持续/周期性/人生大事 ----
    {
        "desc": "长期-健身计划",
        "text": "我决定从下周开始每天坚持跑步30分钟，目标是半年内减重10公斤，已经办了健身卡。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-学习计划",
        "text": "我计划用一年时间系统学习机器学习，从数学基础开始，然后学Python和TensorFlow，最后做项目实践。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-买房",
        "text": "打算明年在市中心买一套三居室的房子，预算500万左右，已经开始看房和咨询贷款了。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-英语学习",
        "text": "我报名了一年的英语口语课程，每周一三五晚上上课，目标是能流利地和外国人交流。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-创业",
        "text": "我和合伙人计划明年创立一家AI SaaS公司，目前在做市场调研和商业计划书，预计需要18个月产品上线。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-学位",
        "text": "我申请了在职MBA项目，需要2年时间完成，每周六上课，学费总共20万。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-储蓄",
        "text": "我设定了三年储蓄计划，每月存5000元，目标是存够首付买一辆30万的车。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-装修",
        "text": "新房装修预计需要3个月，已经找了装修公司出方案，预算25万，下个月开始动工。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-开源项目",
        "text": "我维护了一个开源Rust项目，计划今年完成2.0版本重构，需要重构核心模块和添加插件系统。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-驾照",
        "text": "报名了驾校学习，预计3个月拿驾照，已经考过了科目一，正在练习科目二倒车入库。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-健康管理",
        "text": "医生建议我进行长期血压管理，需要每天测量血压、控制饮食盐分摄入、定期复查，这是一个持续的过程。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-写作",
        "text": "我计划花一年时间写一本关于软件架构的书，已经签了出版合同，每周需要写5000字。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-投资",
        "text": "我制定了5年投资计划，每月定投指数基金5000元，目标年化收益8%，已经开了证券账户。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-健身教练",
        "text": "我打算考取健身教练资格证书，需要3个月培训加考试，已经报名了周末培训班。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-子女教育",
        "text": "孩子明年上小学，我们正在考察附近几所学校的教学质量，计划长期关注教育规划。",
        "expected_type": "long_term",
    },
    # ---- 混合类型 (high/medium/low importance) ----
    {
        "desc": "高重要-服务器迁移",
        "text": "这周六凌晨2点进行生产环境服务器迁移，影响所有在线服务，需要提前通知用户，预计停机4小时。",
        "expected_type": "short_term",
    },
    {
        "desc": "高重要-安全漏洞",
        "text": "发现了一个严重的安全漏洞需要立即修复，涉及用户数据泄露风险，已经通知安全团队紧急响应，今晚必须完成修复。",
        "expected_type": "short_term",
    },
    {
        "desc": "高重要-法律事务",
        "text": "收到法院传票，下周三上午9点需要出庭，关于知识产权纠纷的案件，已经联系了律师准备材料。",
        "expected_type": "short_term",
    },
    {
        "desc": "中重要-读书会",
        "text": "每月最后一个周五的读书会，这个月读的是《思考快与慢》，需要提前读完并准备分享笔记。",
        "expected_type": "short_term",
    },
    {
        "desc": "中重要-团建",
        "text": "下周六公司组织团建活动，去郊区的拓展基地，需要准备运动鞋和防晒用品。",
        "expected_type": "short_term",
    },
    {
        "desc": "低重要-看电影",
        "text": "和朋友约了这周日去看新上映的电影《星际穿越2》，在万达影城IMAX厅，票已经买好了。",
        "expected_type": "short_term",
    },
    {
        "desc": "低重要-理发",
        "text": "这周末去理发店剪头发，准备换个新发型，约了常去的TONY老师。",
        "expected_type": "short_term",
    },
    {
        "desc": "高重要-系统上线",
        "text": "6月30号新系统正式上线，需要提前一周做全量回归测试，确保所有功能正常运行，这是今年的重点KPI项目。",
        "expected_type": "short_term",
    },
    {
        "desc": "长期-职业规划",
        "text": "我制定了5年职业发展计划，前3年深耕技术成为架构师，后2年转向技术管理，目前已经开始学习管理课程。",
        "expected_type": "long_term",
    },
    {
        "desc": "长期-留学",
        "text": "计划明年申请美国计算机硕士项目，正在准备托福和GRE考试，目标是申请TOP30的学校，需要准备1年时间。",
        "expected_type": "long_term",
    },
    # ---- 更多短期事件 ----
    {
        "desc": "短期-退税",
        "text": "下周一记得去税务局办理去年的个人所得税退税，已经预约了下午2点的号，需要带上收入证明。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-修车",
        "text": "车子的刹车片磨损了，约了修车店周三下午去更换，4个轮子都要换，预计需要2小时。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-家长会",
        "text": "周五下午4点参加孩子的家长会，班主任要讨论期中考试成绩和升学规划。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-续费",
        "text": "域名下周三到期，需要在到期前续费3年，否则网站会无法访问，需要尽快处理。",
        "expected_type": "short_term",
    },
    {
        "desc": "短期-体检",
        "text": "下周五早上空腹去体检中心做年度体检，已经预约了7点半的号，需要带身份证。",
        "expected_type": "short_term",
    },
]

TOTAL = len(CONVERSATIONS)


def log(msg, level="INFO"):
    timestamp = datetime.now().strftime("%H:%M:%S")
    print(f"[{timestamp}] [{level}] {msg}")


def add_conversation(content, user_id, scope):
    """发送对话到记忆系统，LLM 自动提取事件"""
    payload = {
        "user_id": user_id,
        "scope": scope,
        "conversation": content,
        "metadata": {"source": "event_extraction_test"},
    }
    try:
        resp = requests.post(
            f"{BASE_URL}/api/memory/add",
            json=payload,
            timeout=120,  # Ollama 可能需要较长时间
        )
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def search_events(query, user_id, scope):
    """搜索事件"""
    payload = {
        "query": query,
        "user_id": user_id,
        "scope": scope,
        "limit": 10,
    }
    try:
        resp = requests.post(
            f"{BASE_URL}/api/memory/search/events",
            json=payload,
            timeout=30,
        )
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def list_events(user_id, scope, limit=100):
    """列出所有事件"""
    try:
        resp = requests.get(
            f"{BASE_URL}/api/memory/events",
            params={"user_id": user_id, "scope": scope, "limit": limit},
            timeout=30,
        )
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def main():
    log("=" * 70)
    log("事件提取测试 - 50 个不同事件，Ollama (qwen3.5:4b)")
    log("=" * 70)

    # 检查服务器
    try:
        resp = requests.get(f"{BASE_URL}/health", timeout=5)
        log(f"Server: {resp.json()}")
    except Exception as e:
        log(f"Server not reachable: {e}", "ERROR")
        sys.exit(1)

    results = {
        "test_name": "Event Extraction - 50 Events, Ollama qwen3.5:4b",
        "test_time": datetime.now(timezone.utc).isoformat(),
        "user_id": USER_ID,
        "total": TOTAL,
        "add_results": [],
        "search_results": [],
        "list_results": {},
        "summary": {},
    }

    # ===== 阶段 1: 写入 50 个对话 =====
    log("\n" + "=" * 70)
    log(f"阶段 1: 写入 {TOTAL} 个对话")
    log("=" * 70)

    created_events = []
    add_success = 0
    add_fail = 0

    for i, conv in enumerate(CONVERSATIONS):
        status, data = add_conversation(conv["text"], USER_ID, SCOPE)

        result = {
            "index": i + 1,
            "desc": conv["desc"],
            "expected_type": conv["expected_type"],
            "http_status": status,
            "success": data.get("success", False),
            "events": data.get("data", {}).get("extracted_events", []),
            "facts": data.get("data", {}).get("extracted_facts", []),
            "error": data.get("error"),
        }

        results["add_results"].append(result)

        if status == 200 and data.get("success"):
            add_success += 1
            events = result["events"]
            event_types = [e.get("importance", "?") for e in events]
            event_titles = [e.get("title", "?")[:30] for e in events]
            log(f"  [{i+1:2d}/{TOTAL}] OK | {conv['desc']} | events={len(events)} | types={event_types} | titles={event_titles}")
            created_events.extend(events)
        else:
            add_fail += 1
            log(f"  [{i+1:2d}/{TOTAL}] FAIL | {conv['desc']} | status={status} | {data.get('error', '')[:80]}")

        # 间隔避免请求过快
        time.sleep(0.3)

    log(f"\nAdd summary: {add_success} success, {add_fail} failed")
    log(f"Total extracted events: {len(created_events)}")

    # ===== 阶段 2: 列出所有事件 =====
    log("\n" + "=" * 70)
    log("阶段 2: 列出所有事件")
    log("=" * 70)

    status, list_data = list_events(USER_ID, SCOPE, limit=200)
    results["list_results"] = {"status": status, "data": list_data}

    if status == 200:
        all_events = list_data.get("data", [])
        total = list_data.get("total", len(all_events))
        log(f"List status: {status}, total events: {total}, returned: {len(all_events)}")

        # 统计事件类型
        type_counts = {}
        for evt in all_events:
            imp = evt.get("importance", "unknown")
            type_counts[imp] = type_counts.get(imp, 0) + 1

        log(f"Event type distribution: {type_counts}")
    else:
        log(f"List failed: {list_data.get('error', '')[:100]}", "ERROR")

    # ===== 阶段 3: 搜索事件 =====
    log("\n" + "=" * 70)
    log("阶段 3: 搜索事件验证")
    log("=" * 70)

    search_queries = [
        ("会议", "会议相关"),
        ("体检", "医疗健康相关"),
        ("学习", "学习计划相关"),
        ("买房", "买房相关"),
        ("服务器", "技术运维相关"),
        ("出差", "差旅相关"),
    ]

    search_ok = 0
    for query, desc in search_queries:
        status, data = search_events(query, USER_ID, SCOPE)
        sr = {"query": query, "desc": desc, "status": status, "data": data}
        results["search_results"].append(sr)

        evt_count = len(data.get("data", [])) if status == 200 else 0
        log(f"  Search '{query}' ({desc}): found {evt_count} events")
        if evt_count > 0:
            search_ok += 1

    log(f"Search: {search_ok}/{len(search_queries)} queries returned results")

    # ===== 阶段 4: 事件类型分析 =====
    log("\n" + "=" * 70)
    log("阶段 4: 事件类型分析 (长/短期)")
    log("=" * 70)

    short_term = []
    long_term = []
    other = []

    for evt in created_events:
        imp = evt.get("importance", "unknown")
        if imp in ("short_term", "short"):
            short_term.append(evt)
        elif imp in ("long_term", "long"):
            long_term.append(evt)
        else:
            other.append(evt)

    log(f"Short-term events: {len(short_term)}")
    log(f"Long-term events:  {len(long_term)}")
    log(f"Other importance:  {len(other)} ({[e.get('importance') for e in other[:5]]})")

    # 检查预期与实际匹配
    mismatch = 0
    for evt in created_events:
        title = evt.get("title", "")
        imp = evt.get("importance", "unknown")
        # 查找对应的预期
        for conv in CONVERSATIONS:
            # 简单匹配：检查标题关键词是否在对话文本中
            if title[:6] and title[:6] in conv["text"]:
                if (imp in ("short_term", "short") and conv["expected_type"] != "short_term") or \
                   (imp in ("long_term", "long") and conv["expected_type"] != "long_term"):
                    mismatch += 1
                    log(f"  MISMATCH: '{title[:40]}' expected={conv['expected_type']} actual={imp}", "WARN")
                break

    if mismatch == 0:
        log("All event type classifications look consistent!")

    # ===== 汇总 =====
    results["summary"] = {
        "add_success": add_success,
        "add_failed": add_fail,
        "total_extracted_events": len(created_events),
        "short_term_count": len(short_term),
        "long_term_count": len(long_term),
        "other_count": len(other),
        "search_queries_ok": search_ok,
        "type_mismatches": mismatch,
    }

    # 保存结果
    output_file = "tests/event_extraction_test_results.json"
    with open(output_file, "w", encoding="utf-8") as f:
        # 简化输出，不保存完整数据
        simplified = {
            "test_name": results["test_name"],
            "test_time": results["test_time"],
            "summary": results["summary"],
            "add_results": [
                {
                    "index": r["index"],
                    "desc": r["desc"],
                    "expected_type": r["expected_type"],
                    "http_status": r["http_status"],
                    "success": r["success"],
                    "event_count": len(r.get("events", [])),
                    "event_types": [e.get("importance") for e in r.get("events", [])],
                    "event_titles": [e.get("title")[:40] for e in r.get("events", [])],
                }
                for r in results["add_results"]
            ],
            "search_results": [
                {
                    "query": r["query"],
                    "desc": r["desc"],
                    "result_count": len(r.get("data", {}).get("data", [])),
                }
                for r in results["search_results"]
            ],
        }
        json.dump(simplified, f, ensure_ascii=False, indent=2)
    log(f"\nResults saved to: {output_file}")

    # 打印最终摘要
    log("\n" + "=" * 70)
    log("最终摘要")
    log("=" * 70)
    s = results["summary"]
    log(f"  对话写入成功: {s['add_success']}/{TOTAL}")
    log(f"  提取事件总数: {s['total_extracted_events']}")
    log(f"  短期事件: {s['short_term_count']}")
    log(f"  长期事件: {s['long_term_count']}")
    log(f"  其他类型: {s['other_count']}")
    log(f"  搜索命中: {s['search_queries_ok']}/6")
    log(f"  类型偏差: {s['type_mismatches']}")
    log(f"\n  ===== TEST {'PASSED' if add_success >= 45 else 'NEEDS REVIEW'} =====")

    return 0 if add_success >= 45 else 1


if __name__ == "__main__":
    sys.exit(main())