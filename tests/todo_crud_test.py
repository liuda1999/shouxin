#!/usr/bin/env python3
"""
待办事项 CRUD 测试 - 创建 20 个不同待办任务并依次完成
使用真实数据，正规方式测试
"""

import requests
import json
import time
import sys
from datetime import datetime, timedelta, timezone

BASE_URL = "http://127.0.0.1:3000"
USER_ID = "test_crud_20260606"
SCOPE = "User"

# 20 个完全不同的待办任务
TODOS = [
    {
        "title": "完成Q2季度财务报告",
        "description": "汇总Q2季度所有财务数据，包括收入、支出、利润分析，并撰写报告提交给董事会",
        "deadline": "2026-06-15T17:00:00+08:00",
        "importance": "critical",
    },
    {
        "title": "修复用户登录页面样式错乱",
        "description": "登录页面在Chrome最新版出现CSS样式错乱，需要排查flex布局兼容性问题并修复",
        "deadline": "2026-06-08T12:00:00+08:00",
        "importance": "high",
    },
    {
        "title": "预约牙科检查",
        "description": "半年一次的常规牙科检查，提前预约市口腔医院王医生",
        "deadline": "2026-06-20T09:00:00+08:00",
        "importance": "low",
    },
    {
        "title": "编写新员工入职培训材料",
        "description": "为下个月入职的5名新员工准备培训PPT和文档，涵盖公司文化、开发流程、代码规范",
        "deadline": "2026-06-25T18:00:00+08:00",
        "importance": "medium",
    },
    {
        "title": "数据库索引优化与慢查询分析",
        "description": "分析线上数据库慢查询日志，为高频查询添加合适的索引，目标将P99响应时间降低50%",
        "deadline": "2026-06-10T10:00:00+08:00",
        "importance": "high",
    },
    {
        "title": "购买生日礼物给妈妈",
        "description": "妈妈下周三生日，需要挑选一份礼物，考虑护肤品套装或按摩仪",
        "deadline": "2026-06-09T20:00:00+08:00",
        "importance": "high",
    },
    {
        "title": "重构API错误处理中间件",
        "description": "统一所有API接口的错误响应格式，添加错误码枚举和国际化错误消息支持",
        "deadline": "2026-06-18T17:00:00+08:00",
        "importance": "medium",
    },
    {
        "title": "整理年度书单并采购",
        "description": "根据技术趋势和个人发展需要，整理今年要读的12本书，并在当当网统一下单采购",
        "deadline": "2026-06-12T23:59:00+08:00",
        "importance": "low",
    },
    {
        "title": "部署CI/CD流水线自动化测试",
        "description": "在GitHub Actions中配置自动化测试、代码检查、构建和部署流水线，确保每次PR自动触发",
        "deadline": "2026-06-14T15:00:00+08:00",
        "importance": "critical",
    },
    {
        "title": "更新项目依赖库到最新版本",
        "description": "检查Cargo.toml中所有依赖的最新版本，升级并解决可能的兼容性问题，运行完整测试套件",
        "deadline": "2026-06-16T12:00:00+08:00",
        "importance": "medium",
    },
    {
        "title": "设计并实现消息通知模块",
        "description": "设计统一的消息通知接口，支持邮件、站内信、Webhook三种通知方式，实现邮件通知功能",
        "deadline": "2026-06-22T18:00:00+08:00",
        "importance": "high",
    },
    {
        "title": "清理家中闲置物品挂闲鱼",
        "description": "整理家中不再使用的电子产品、书籍和衣物，拍照上传到闲鱼平台出售",
        "deadline": "2026-06-11T14:00:00+08:00",
        "importance": "low",
    },
    {
        "title": "撰写技术博客：Rust异步编程实践",
        "description": "总结项目中Rust异步编程的经验，包括tokio、async-trait、Stream等使用技巧，发布到个人博客",
        "deadline": "2026-06-28T23:59:00+08:00",
        "importance": "medium",
    },
    {
        "title": "性能压测与容量规划",
        "description": "使用JMeter对核心API进行压力测试，确定系统QPS上限，输出容量规划报告",
        "deadline": "2026-06-17T16:00:00+08:00",
        "importance": "critical",
    },
    {
        "title": "办理护照续签手续",
        "description": "护照即将到期，准备身份证、照片等材料，前往出入境管理局办理续签",
        "deadline": "2026-06-13T11:00:00+08:00",
        "importance": "high",
    },
    {
        "title": "学习Kubernetes基础操作",
        "description": "完成K8s官方教程的基础部分，掌握Pod、Service、Deployment的创建和管理",
        "deadline": "2026-06-30T20:00:00+08:00",
        "importance": "medium",
    },
    {
        "title": "修复生产环境内存泄漏问题",
        "description": "线上服务出现缓慢的内存增长，使用valgrind和heaptrack分析内存泄漏点并修复",
        "deadline": "2026-06-08T18:00:00+08:00",
        "importance": "critical",
    },
    {
        "title": "组织团队团建活动",
        "description": "策划本季度团队建设活动，包括场地预订、活动流程设计、预算审批和人员统计",
        "deadline": "2026-06-19T17:00:00+08:00",
        "importance": "medium",
    },
    {
        "title": "备份重要数据到NAS",
        "description": "将工作文档、项目代码、照片等数据备份到家庭NAS，并验证备份完整性",
        "deadline": "2026-06-07T22:00:00+08:00",
        "importance": "low",
    },
    {
        "title": "实现多租户数据隔离方案",
        "description": "设计并实现基于owner_id+owner_type的多租户数据隔离方案，确保不同用户数据完全隔离",
        "deadline": "2026-06-24T17:00:00+08:00",
        "importance": "high",
    },
]


def log(msg, level="INFO"):
    timestamp = datetime.now().strftime("%H:%M:%S")
    print(f"[{timestamp}] [{level}] {msg}")


def create_todo(todo_data, index):
    """创建单个待办任务"""
    payload = {
        "title": todo_data["title"],
        "description": todo_data["description"],
        "deadline": todo_data["deadline"],
        "importance": todo_data["importance"],
        "scope": SCOPE,
        "user_id": USER_ID,
    }
    try:
        resp = requests.post(
            f"{BASE_URL}/api/todos",
            json=payload,
            timeout=30,
        )
        result = resp.json()
        return {
            "index": index,
            "title": todo_data["title"],
            "http_status": resp.status_code,
            "success": result.get("success", False),
            "data": result.get("data"),
            "error": result.get("error"),
            "request": payload,
        }
    except Exception as e:
        return {
            "index": index,
            "title": todo_data["title"],
            "http_status": 0,
            "success": False,
            "data": None,
            "error": str(e),
            "request": payload,
        }


def complete_todo(todo_id, title):
    """完成单个待办任务"""
    try:
        resp = requests.put(
            f"{BASE_URL}/api/todos/{todo_id}/complete",
            params={"scope": SCOPE, "user_id": USER_ID},
            timeout=30,
        )
        result = resp.json()
        return {
            "todo_id": todo_id,
            "title": title,
            "http_status": resp.status_code,
            "success": result.get("success", False),
            "error": result.get("error"),
        }
    except Exception as e:
        return {
            "todo_id": todo_id,
            "title": title,
            "http_status": 0,
            "success": False,
            "error": str(e),
        }


def list_todos(status=None):
    """获取待办列表"""
    params = {"scope": SCOPE, "user_id": USER_ID, "limit": 100}
    if status:
        params["status"] = status
    try:
        resp = requests.get(
            f"{BASE_URL}/api/todos",
            params=params,
            timeout=30,
        )
        result = resp.json()
        return result.get("data", [])
    except Exception as e:
        log(f"Failed to list todos: {e}", "ERROR")
        return []


def main():
    log("=" * 60)
    log("待办事项 CRUD 测试开始")
    log("=" * 60)

    # 检查服务器连接
    try:
        resp = requests.get(f"{BASE_URL}/health", timeout=5)
        log(f"Server health: {resp.json()}")
    except Exception as e:
        log(f"Server not reachable: {e}", "ERROR")
        sys.exit(1)

    # 清理之前可能存在的测试数据
    log("Cleaning up existing test data...")
    existing = list_todos()
    for todo in existing:
        if todo.get("owner_id") == USER_ID:
            todo_id = todo.get("todo_id")
            requests.delete(
                f"{BASE_URL}/api/todos/{todo_id}",
                params={"scope": SCOPE, "user_id": USER_ID},
                timeout=10,
            )
    log(f"Cleaned up {len(existing)} existing todos")

    results = {
        "test_name": "Todo CRUD Test - 20 Todos",
        "test_time": datetime.now(timezone.utc).isoformat(),
        "user_id": USER_ID,
        "scope": SCOPE,
        "create_results": [],
        "complete_results": [],
        "summary": {},
    }

    # ===== 阶段 1: 创建 20 个待办 =====
    log("\n" + "=" * 60)
    log("阶段 1: 创建 20 个待办任务")
    log("=" * 60)

    created_ids = []
    create_success = 0
    create_fail = 0

    for i, todo_data in enumerate(TODOS):
        result = create_todo(todo_data, i + 1)
        results["create_results"].append(result)

        if result["success"]:
            todo_id = result["data"]["todo_id"]
            created_ids.append((todo_id, todo_data["title"]))
            create_success += 1
            log(
                f"  [{i+1:2d}/20] CREATED  | {result['http_status']} | {todo_id[:8]}... | {todo_data['title']}"
            )
        else:
            create_fail += 1
            log(
                f"  [{i+1:2d}/20] FAILED   | {result['http_status']} | {todo_data['title']} | {result.get('error', '')}"
            )

        # 短暂间隔避免请求过快
        time.sleep(0.1)

    log(f"\nCreate summary: {create_success} success, {create_fail} failed")

    # ===== 阶段 2: 验证待办列表 =====
    log("\n" + "=" * 60)
    log("阶段 2: 验证待办列表")
    log("=" * 60)

    pending_todos = list_todos(status="pending")
    log(f"Pending todos count: {len(pending_todos)}")

    results["verify_pending_count"] = len(pending_todos)

    # ===== 阶段 3: 依次完成所有待办 =====
    log("\n" + "=" * 60)
    log("阶段 3: 依次完成所有待办任务")
    log("=" * 60)

    complete_success = 0
    complete_fail = 0

    for i, (todo_id, title) in enumerate(created_ids):
        result = complete_todo(todo_id, title)
        results["complete_results"].append(result)

        if result["success"]:
            complete_success += 1
            log(
                f"  [{i+1:2d}/{len(created_ids)}] COMPLETED | {result['http_status']} | {todo_id[:8]}... | {title}"
            )
        else:
            complete_fail += 1
            log(
                f"  [{i+1:2d}/{len(created_ids)}] FAILED   | {result['http_status']} | {title} | {result.get('error', '')}"
            )

        time.sleep(0.1)

    log(f"\nComplete summary: {complete_success} success, {complete_fail} failed")

    # ===== 阶段 4: 最终验证 =====
    log("\n" + "=" * 60)
    log("阶段 4: 最终验证")
    log("=" * 60)

    final_pending = list_todos(status="pending")
    all_todos = list_todos()

    log(f"Final pending todos: {len(final_pending)}")
    log(f"All todos (including archived): {len(all_todos)}")

    results["summary"] = {
        "total_created": create_success + create_fail,
        "create_success": create_success,
        "create_failed": create_fail,
        "total_completed": complete_success + complete_fail,
        "complete_success": complete_success,
        "complete_failed": complete_fail,
        "final_pending_count": len(final_pending),
        "final_all_count": len(all_todos),
    }

    # 保存结果
    output_file = "tests/todo_crud_test_results_v2.json"
    with open(output_file, "w", encoding="utf-8") as f:
        json.dump(results, f, ensure_ascii=False, indent=2)
    log(f"\nTest results saved to: {output_file}")

    # 打印摘要
    log("\n" + "=" * 60)
    log("测试摘要")
    log("=" * 60)
    for key, value in results["summary"].items():
        status = "PASS" if value > 0 or key.endswith("_failed") else "FAIL"
        log(f"  {key}: {value} [{status}]")

    # 判断整体是否通过
    if create_success == 20 and complete_success == 20:
        log("\n*** ALL TESTS PASSED ***")
        return 0
    else:
        log(f"\n*** TEST FAILED: {create_success}/20 created, {complete_success}/20 completed ***")
        return 1


if __name__ == "__main__":
    sys.exit(main())