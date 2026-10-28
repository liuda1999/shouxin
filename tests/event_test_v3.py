#!/usr/bin/env python3
"""
事件提取测试 v3 - 50 个不同事件，支持 Ollama / GGUF 双后端对比测试
用法: python tests/event_test_v3.py --backend ollama|gguf [--user-id xxx]
"""
import requests
import json
import time
import sys
import argparse
import os
from datetime import datetime, timezone

BASE_URL = "http://127.0.0.1:3000"

# 50 个不同的事件对话
CONVERSATIONS = [
    # === 短期事件 (25 个) ===
    ("短期-项目评审会", "明天下午3点参加Q2项目评审会，在A栋3楼大会议室，需要准备PPT和演示数据。", "short_term"),
    ("短期-续费域名", "网站域名下周三到期，必须在到期前续费3年，否则服务会中断影响业务。", "short_term"),
    ("短期-牙科预约", "下周一上午10点预约了牙科检查，在市中心医院3楼口腔科，需要带医保卡。", "short_term"),
    ("短期-航班出行", "6月15号下午2点飞北京的航班CA1234，T3航站楼登机，需要提前2小时到机场。", "short_term"),
    ("短期-同学聚餐", "今晚7点和大学同学在海底捞聚餐，一共8个人，已经订好了大包间，AA制。", "short_term"),
    ("短期-证书考试", "下周三上午9点参加AWS架构师认证考试，需要带身份证和准考证，考试地点在科技园。", "short_term"),
    ("短期-家电送货", "明天从京东买的新冰箱会送到，需要在家收货，送货时间上午9点到12点。", "short_term"),
    ("短期-空调维修", "客厅空调不制冷了，约了维修师傅后天上午10点上门检修，可能要加氟利昂。", "short_term"),
    ("短期-技术面试", "周五下午2点有一个高级后端工程师面试，需要准备系统设计题和白板编程。", "short_term"),
    ("短期-产品发布会", "6月18号下午参加新产品的线上发布会，需要准备演示文稿和产品Demo视频。", "short_term"),
    ("短期-K8s培训", "下周二全天参加公司组织的Kubernetes实战培训，需要自带笔记本电脑做实验。", "short_term"),
    ("短期-搬家事宜", "这个周六要搬家，已经联系了货拉拉搬家公司，早上8点开始，预计下午3点结束。", "short_term"),
    ("短期-生日派对", "周日晚上7点去KTV给小张过生日，买了一个慕斯蛋糕，准备了礼物。", "short_term"),
    ("短期-合同签约", "下周四上午11点去客户公司签合同，合同金额50万，需要带公章和法人章。", "short_term"),
    ("短期-车辆年检", "6月20号下午去车管所办理车辆年检，记得带行驶证和交强险保单副本。", "short_term"),
    ("短期-跨国会议", "明天上午10点和美国团队开跨国视频会议，讨论Q3产品路线图，需要准备英文汇报。", "short_term"),
    ("短期-技术演讲", "周五下午3点要在Rust技术大会上做分享，主题是Rust在嵌入式系统中的应用实践。", "short_term"),
    ("短期-深圳出差", "下周一去深圳出差3天，需要提前预订酒店和机票，拜访3家客户公司。", "short_term"),
    ("短期-个税退税", "下周一去税务局办理个人所得税退税，已经网上预约了下午2点，需要带收入证明。", "short_term"),
    ("短期-汽车保养", "周六上午去4S店做汽车保养，已经超过保养里程500公里了，需要换机油和机滤。", "short_term"),
    ("短期-家长会", "周五下午4点参加孩子学校的家长会，班主任要讨论期中考试成绩和下学期的选课规划。", "short_term"),
    ("短期-体检预约", "下周五早上空腹去体检中心做年度体检，预约了7点半的号，需要带身份证和体检卡。", "short_term"),
    ("短期-干洗取件", "后天下午去干洗店取西装，已经洗好了，需要在6点前取，否则要加收保管费。", "short_term"),
    ("短期-护照续签", "下周三去出入境管理局办理护照续签，需要身份证和近6个月白底照片，提前一天在线预约。", "short_term"),
    ("短期-部门周会", "明天上午9点主持部门周会，需要总结上周工作进展、本周计划安排和风险项。", "short_term"),

    # === 长期事件 (15 个) ===
    ("长期-健身目标", "我决定从下周开始每天坚持跑步30分钟，目标是半年内减重10公斤，改善体脂率。", "long_term"),
    ("长期-机器学习", "我计划用一年时间系统学习机器学习，从数学基础到深度学习，每周至少投入10小时。", "long_term"),
    ("长期-购房计划", "打算明年在市中心买一套三居室，预算500万，已经开始看房和比较贷款方案。", "long_term"),
    ("长期-英语口语", "报名了一年的英语口语课程，每周一三五晚上上课，目标是能流利地进行商务英语交流。", "long_term"),
    ("长期-创业规划", "我和合伙人计划明年创立一家AI SaaS公司，目前在做市场调研和商业计划书撰写。", "long_term"),
    ("长期-MBA学位", "申请了在职MBA项目，需要2年时间完成学业，每周六上课，学费总共20万。", "long_term"),
    ("长期-储蓄计划", "设定了三年储蓄计划，每月存5000元到理财账户，目标是存够首付买一辆车。", "long_term"),
    ("长期-新房装修", "新房装修预计需要3个月，已经找了装修公司出方案，预算25万，需要盯工地进度。", "long_term"),
    ("长期-开源项目", "我维护了一个开源Rust框架，计划今年完成2.0版本重构，添加插件系统和WebAssembly支持。", "long_term"),
    ("长期-驾照考试", "报名了驾校学习，预计3个月拿驾照，已经考过了科目一理论，正在练习科目二倒车入库。", "long_term"),
    ("长期-血压管理", "医生建议进行长期血压管理，需要每天早晚测量血压、控制饮食盐分摄入、定期复查。", "long_term"),
    ("长期-写作出书", "计划花一年时间写一本关于软件架构设计的书，已签出版合同，每周需要完成5000字。", "long_term"),
    ("长期-投资理财", "制定了5年投资计划，每月定投指数基金5000元，目标年化收益8%，每季度做一次资产再平衡。", "long_term"),
    ("长期-职业发展", "制定了5年职业发展计划，前3年深耕技术成为架构师，后2年转向技术管理带团队。", "long_term"),
    ("长期-留学申请", "计划明年申请美国计算机硕士项目，正在准备托福和GRE考试，目标是申请TOP30学校。", "long_term"),

    # === 混合/高重要事件 (10 个) ===
    ("高重要-服务器迁移", "这周六凌晨2点进行生产环境服务器迁移，影响所有在线服务，预计停机4小时，需要制定回滚方案。", "short_term"),
    ("高重要-安全漏洞", "发现了严重的安全漏洞需要立即修复，涉及用户数据泄露风险，今晚必须完成补丁部署。", "short_term"),
    ("高重要-法律诉讼", "收到法院传票，下周三上午9点需要出庭，关于知识产权纠纷的案件，律师费已经预付。", "short_term"),
    ("高重要-系统上线", "6月30号新系统正式上线，需要提前一周做全量回归测试，确保所有核心功能正常运行。", "short_term"),
    ("中重要-读书分享", "每月最后一个周五的读书分享会，这个月读的是《思考快与慢》，需要准备PPT分享笔记。", "short_term"),
    ("中重要-公司团建", "下周六公司组织团建活动，去郊区拓展基地，需要准备运动鞋和防晒用品，两天一夜。", "short_term"),
    ("低重要-看电影", "和朋友约了这周日去看新上映的科幻片，在万达影城IMAX厅，票已经在线买好了。", "short_term"),
    ("低重要-理发换发型", "这周末去理发店剪头发，准备换个新发型，约了常去的TONY老师下午3点。", "short_term"),
    ("低重要-网购退货", "前天买的蓝牙耳机音质不好，明天需要去快递点退货，已经申请了退货退款。", "short_term"),
    ("低重要-取快递", "小区快递柜有一个包裹已经放了2天了，今天下班记得去取，是买的书到了。", "short_term"),
]

TOTAL = len(CONVERSATIONS)


def log(msg, level="INFO"):
    timestamp = datetime.now().strftime("%H:%M:%S")
    print(f"[{timestamp}] [{level}] {msg}", flush=True)


def add_conversation(text, user_id, scope):
    payload = {
        "user_id": user_id,
        "scope": scope,
        "conversation": text,
        "metadata": {"source": "event_test_v3"},
    }
    try:
        resp = requests.post(f"{BASE_URL}/api/memory/add", json=payload, timeout=120)
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def get_extract_status(conv_id):
    try:
        resp = requests.get(f"{BASE_URL}/api/memory/extract_status/{conv_id}", timeout=10)
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def search_events(query, user_id, scope, limit=50):
    payload = {"query": query, "user_id": user_id, "scope": scope, "limit": limit}
    try:
        resp = requests.post(f"{BASE_URL}/api/memory/search/events", json=payload, timeout=30)
        return resp.status_code, resp.json()
    except Exception as e:
        return 0, {"error": str(e)}


def main():
    parser = argparse.ArgumentParser(description="Event Extraction Test v3")
    parser.add_argument("--backend", required=True, choices=["ollama", "gguf"],
                        help="LLM backend to test")
    parser.add_argument("--user-id", default=None,
                        help="User ID (default: test_user_v3_{backend})")
    args = parser.parse_args()

    backend = args.backend
    user_id = args.user_id or f"test_user_v3_{backend}"
    scope = "User"

    log("=" * 70)
    log(f"事件提取测试 v3 - 50 个事件 | 后端: {backend.upper()}")
    log("=" * 70)

    # 检查服务器
    try:
        resp = requests.get(f"{BASE_URL}/health", timeout=5)
        log(f"Server: {resp.json()}")
    except Exception as e:
        log(f"服务器不可达: {e}", "ERROR")
        sys.exit(1)

    results = {
        "test_name": f"Event Extraction v3 - 50 Events, {backend.upper()}",
        "test_time": datetime.now(timezone.utc).isoformat(),
        "backend": backend,
        "user_id": user_id,
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
    add_fail = 0

    for i, (desc, text, expected_type) in enumerate(CONVERSATIONS):
        status, data = add_conversation(text, user_id, scope)
        result = {
            "index": i + 1, "desc": desc, "expected_type": expected_type,
            "http_status": status, "success": data.get("success", False),
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
            add_fail += 1
            log(f"  [{i+1:2d}/{TOTAL}] FAIL | {desc} | {data.get('error', '')[:60]}")

        time.sleep(0.3)

    log(f"\n写入完成: {add_success}/{TOTAL} 成功, {add_fail} 失败")

    if add_success == 0:
        log("没有成功写入任何对话，停止测试", "ERROR")
        sys.exit(1)

    # ===== 阶段 2: 等待异步提取完成 =====
    log(f"\n{'='*70}")
    log("阶段 2: 等待异步事件提取完成")
    log(f"{'='*70}")

    max_wait = 600  # 最多等 10 分钟
    wait_interval = 5
    waited = 0

    while waited < max_wait:
        done_count = 0
        extracting_count = 0
        sample_size = min(10, len(conv_ids))

        for cid in conv_ids[:sample_size]:
            status, data = get_extract_status(cid)
            es = data.get("data", {}).get("status", "unknown") if status == 200 else "error"
            if es == "completed":
                done_count += 1
            elif es == "extracting":
                extracting_count += 1

        total_done = int(done_count / sample_size * len(conv_ids)) if sample_size > 0 else 0

        log(f"  [{waited:3d}s] 采样: done={done_count}/{sample_size} extracting={extracting_count} "
            f"预估总完成: {total_done}/{len(conv_ids)}")

        if done_count == sample_size:
            log("所有采样对话提取完成！")
            break

        time.sleep(wait_interval)
        waited += wait_interval

    log(f"等待结束，耗时 {waited}s")

    # ===== 阶段 3: 搜索收集所有事件 =====
    log(f"\n{'='*70}")
    log("阶段 3: 搜索收集所有事件")
    log(f"{'='*70}")

    search_queries = [
        "会议", "体检", "学习", "买房", "服务器", "出差",
        "搬家", "生日", "考试", "面试", "装修", "投资",
        "健身", "项目", "合同", "培训", "驾驶", "留学",
        "医疗", "安全", "法律", "电影", "理发", "快递",
        "飞机", "维修", "发布会", "储蓄", "开源", "创业",
        "英语", "血压", "MBA", "写作", "职业", "理财",
    ]

    all_events = {}
    for query in search_queries:
        status, data = search_events(query, user_id, scope, limit=50)
        if status == 200:
            evts = data.get("data", [])
            for evt in evts:
                eid = evt.get("event_id", evt.get("id", ""))
                if eid and eid not in all_events:
                    all_events[eid] = evt
        time.sleep(0.15)

    log(f"搜索完成，唯一事件总数: {len(all_events)}")

    # ===== 阶段 4: 事件类型分析 =====
    log(f"\n{'='*70}")
    log("阶段 4: 事件类型分析 (长/短期)")
    log(f"{'='*70}")

    short_term = []
    long_term = []
    critical = []
    low_importance = []
    unknown = []

    for eid, evt in all_events.items():
        imp = evt.get("importance", "unknown")
        title = evt.get("title", evt.get("summary", "?"))
        if isinstance(imp, (int, float)):
            imp_score = float(imp)
            if imp_score >= 0.95:
                critical.append(evt)
            elif imp_score > 0.7:
                long_term.append(evt)
            elif imp_score >= 0.5:
                short_term.append(evt)
            else:
                low_importance.append(evt)
        elif imp in ("short_term", "short"):
            short_term.append(evt)
        elif imp in ("long_term", "long"):
            long_term.append(evt)
        elif imp == "critical":
            critical.append(evt)
        elif imp == "high":
            # API 返回 "high" 表示 importance >= 0.8，可能是 long_term 或 critical
            long_term.append(evt)
        elif imp == "medium":
            # API 返回 "medium" 表示 importance >= 0.5 (short_term)
            short_term.append(evt)
        elif imp == "low":
            low_importance.append(evt)
        else:
            unknown.append(evt)

    log(f"Critical (>=0.95):    {len(critical)}")
    log(f"Long-term (>0.7):     {len(long_term)}")
    log(f"Short-term (>=0.5):   {len(short_term)}")
    log(f"Low importance (<0.5):{len(low_importance)}")
    log(f"Unknown:              {len(unknown)}")

    if unknown:
        log(f"\nUnknown importance types:")
        for evt in unknown[:5]:
            log(f"  '{evt.get('title', '?')[:40]}' importance={evt.get('importance')}")

    if critical:
        log(f"\nCritical events:")
        for evt in critical[:5]:
            log(f"  - {evt.get('title', '?')[:50]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    if long_term:
        log(f"\nLong-term samples:")
        for evt in long_term[:5]:
            log(f"  - {evt.get('title', '?')[:50]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    if short_term:
        log(f"\nShort-term samples:")
        for evt in short_term[:5]:
            log(f"  - {evt.get('title', '?')[:50]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    if low_importance:
        log(f"\nLow importance samples:")
        for evt in low_importance[:5]:
            log(f"  - {evt.get('title', '?')[:50]} | {evt.get('time', '?')[:20]} | {evt.get('importance')}")

    # ===== 阶段 5: 分类准确性验证 =====
    log(f"\n{'='*70}")
    log("阶段 5: 分类准确性验证")
    log(f"{'='*70}")

    # 建立事件标题到预期类型的映射
    expected_map = {desc: expected_type for desc, _, expected_type in CONVERSATIONS}

    correct = 0
    incorrect = 0
    unmatched = 0
    mismatches = []

    for i, (desc, text, expected_type) in enumerate(CONVERSATIONS):
        # 尝试通过标题匹配
        matched = False
        for eid, evt in all_events.items():
            evt_title = (evt.get("title") or evt.get("summary") or "").lower()
            evt_desc = (evt.get("description") or "").lower()
            desc_keywords = desc.split("-", 1)[-1].lower() if "-" in desc else desc.lower()

            # 简单关键词匹配
            kw_match = any(kw in evt_title or kw in evt_desc
                           for kw in desc_keywords.split())

            if kw_match:
                matched = True
                imp = evt.get("importance", "unknown")
                if isinstance(imp, (int, float)):
                    imp = float(imp)
                    if imp >= 0.95:
                        actual = "critical"
                    elif imp > 0.7:
                        actual = "long_term"
                    elif imp >= 0.5:
                        actual = "short_term"
                    else:
                        actual = "low"
                else:
                    imp_str = str(imp)
                    # API 返回: "high" → >=0.8 (long_term), "medium" → >=0.5 (short_term), "low" → <0.5
                    if imp_str == "high":
                        actual = "long_term"
                    elif imp_str == "medium":
                        actual = "short_term"
                    elif imp_str == "low":
                        actual = "low"
                    else:
                        actual = imp_str

                if expected_type == "long_term" and actual in ("long_term", "critical", "high"):
                    correct += 1
                elif expected_type == "short_term" and actual in ("short_term", "medium", "low"):
                    correct += 1
                elif expected_type == actual:
                    correct += 1
                else:
                    incorrect += 1
                    mismatches.append({
                        "desc": desc,
                        "expected": expected_type,
                        "actual": actual,
                        "title": evt.get("title", "")[:50],
                        "importance": evt.get("importance"),
                    })
                break

        if not matched:
            unmatched += 1

    log(f"匹配成功: {correct + incorrect}")
    log(f"  分类正确: {correct}")
    log(f"  分类错误: {incorrect}")
    log(f"  未匹配到: {unmatched}")

    accuracy = correct / (correct + incorrect) * 100 if (correct + incorrect) > 0 else 0
    log(f"  分类准确率: {accuracy:.1f}%")

    if mismatches:
        log(f"\n分类错误详情:")
        for mm in mismatches[:10]:
            log(f"  [{mm['desc']}] expected={mm['expected']} actual={mm['actual']} "
                f"title='{mm['title']}' imp={mm['importance']}")

    # ===== 汇总 =====
    results["summary"] = {
        "add_success": add_success,
        "add_fail": add_fail,
        "total_conversations": TOTAL,
        "total_unique_events": len(all_events),
        "critical_count": len(critical),
        "long_term_count": len(long_term),
        "short_term_count": len(short_term),
        "low_importance_count": len(low_importance),
        "unknown_count": len(unknown),
        "categories": {
            "critical": len(critical),
            "long_term": len(long_term),
            "short_term": len(short_term),
            "low": len(low_importance),
            "unknown": len(unknown),
        },
        "accuracy_matched": correct + incorrect,
        "accuracy_correct": correct,
        "accuracy_incorrect": incorrect,
        "accuracy_unmatched": unmatched,
        "accuracy_pct": round(accuracy, 1),
        "mismatches": mismatches,
    }

    # 保存结果
    output_file = f"tests/event_test_results_v3_{backend}.json"
    with open(output_file, "w", encoding="utf-8") as f:
        simplified = {
            "test_name": results["test_name"],
            "test_time": results["test_time"],
            "backend": backend,
            "summary": results["summary"],
            "add_results": [
                {"index": r["index"], "desc": r["desc"],
                 "expected_type": r["expected_type"],
                 "success": r["success"], "conversation_id": r["conversation_id"]}
                for r in results["add_results"]
            ],
            "events": [
                {
                    "title": e.get("title", ""),
                    "summary": e.get("summary", ""),
                    "importance": e.get("importance", ""),
                    "time": e.get("time", ""),
                    "description": (e.get("description", "") or "")[:100],
                }
                for e in all_events.values()
            ],
        }
        json.dump(simplified, f, ensure_ascii=False, indent=2)
    log(f"\n结果已保存: {output_file}")

    # 最终摘要
    log(f"\n{'='*70}")
    log(f"最终摘要 - {backend.upper()}")
    log(f"{'='*70}")
    s = results["summary"]
    log(f"  对话写入: {s['add_success']}/{TOTAL} 成功")
    log(f"  唯一事件: {s['total_unique_events']}")
    log(f"  Critical:  {s['critical_count']}")
    log(f"  Long-term: {s['long_term_count']}")
    log(f"  Short-term:{s['short_term_count']}")
    log(f"  Low:       {s['low_importance_count']}")
    log(f"  分类准确率: {s['accuracy_pct']}%")

    if s['total_unique_events'] >= 30:
        log(f"\n  ===== {backend.upper()} TEST PASSED =====")
        return 0
    else:
        log(f"\n  ===== {backend.upper()} TEST NEEDS REVIEW =====")
        return 1


if __name__ == "__main__":
    sys.exit(main())