#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""守心 200 条验收压测

覆盖六类语料：短内容 / 长内容 / 数字公式 / 复杂人物关系 / 多人对话 / 边界输入。

用法：
    python scripts/stress_200.py submit  --base http://127.0.0.1:3000 --user stress200
    python scripts/stress_200.py poll    --base http://127.0.0.1:3000 --user stress200
    python scripts/stress_200.py verify  --base http://127.0.0.1:3000 --user stress200

`submit` 只负责提交并把 conversation_id 落到 .stress200_state.json；
`poll` 轮询抽取状态直到全部终态；
`verify` 做四类检查：联系人是否创建 / 人设是否生成 / 虚拟人物是否可区分 /
联系人详情能否取到关联联系人·事件·记忆。

设计说明：
- 语料全部离线生成，不依赖外部服务，保证可重复；
- 每条语料带 `people`（期望被识别的人名），用于计算召回；
- 不假设模型一定抽出所有字段——本脚本的目的是**度量**，不是断言全对。
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

# 状态文件按用户隔离：不同轮次/不同用户不能共用，
# 否则"断点续传"会把上一轮的 conversation_id 当成本轮的，直接跳过提交，
# 造成"看起来 242 条全提交，实际只提交了新增的几条"。
STATE_FILE = ".stress200_state.json"


def state_path(user: str) -> str:
    return f".stress_state_{user}.json"


# --------------------------------------------------------------------------
# 语料
# --------------------------------------------------------------------------

def short_items():
    """短内容：1-2 句，含 1-2 个人名 + 一个事实/待办。"""
    raw = [
        "张伟说明天上午十点开周会，他要汇报季度数据。",
        "李娜负责这次改版的设计稿，周五前给我初稿。",
        "王强昨天把服务器扩容到 32 核，说这周观察负载。",
        "陈静提醒我下周三前交报销单，她周一统一提交财务。",
        "刘洋周五请假，他的值班我来顶。",
        "赵敏把客户名单发我邮箱了，让我今天下班前回访。",
        "孙磊说数据库索引要重建，预计凌晨两点开始。",
        "周涛下午三点和供应商谈价格，让我准备比价表。",
        "吴倩负责新员工培训，下周一上午九点在第一会议室。",
        "郑凯提醒我合同盖章要走法务，至少提前两天。",
        "冯磊说他下周出差深圳，有事微信找他。",
        "许静把孩子接放学的事托给我，今天下午五点半。",
        "何伟的代码合并请求还挂着，需要我 review。",
        "高翔说测试环境今天下午会停机维护。",
        "林芳让我帮她订周五去杭州的高铁票。",
        "罗军负责对接支付渠道，他说沙箱今天可用。",
        "梁静提交了离职申请，最后工作日是下个月十号。",
        "宋涛说这版需求要先砍掉导出功能。",
        "唐嫣明天来公司面试后端岗位，让我安排会议室。",
        "韩磊上周把日志系统换成了 Loki。",
        "曹阳说预算超了两万，需要我签字确认。",
        "邓超负责双十一的压测方案，周三评审。",
        "范冰提醒我域名快到期了，还有 20 天。",
        "彭勇把接口文档更新到 v2.3 了。",
        "曾毅负责值班表，让我确认下月排班。",
        "蒋雯说客户对现在的配色不满意，要重做。",
        "苏航今天入职，工位在 A 区 12 号。",
        "卢明说线上有个偶发超时，让我看下监控。",
        "蔡琴负责年会节目报名，截止本周五。",
        "贾斌说他电脑坏了，申请换一台。",
        "丁宁让我把周报模板发她一份。",
        "魏强负责机房巡检，每月 15 号。",
        "薛涛说接口返回字段要加个 traceId。",
        "叶琳下周休产假，工作交接给我。",
        "阎磊提醒我下周有消防演练，全员参加。",
        "余静说这批设备采购要走招标流程。",
        "潘伟负责客户端发版，今晚十点灰度。",
        "杜鹏说客户那边换了对接人，姓顾。",
        "戴敏让我核对上个月的考勤异常。",
        "夏雨负责数据看板，她说指标口径要统一。",
        "钟磊说这次的 bug 是缓存穿透引起的。",
        "汪洋明天上午来取合同原件。",
        "田甜负责招聘渠道，让我推荐两个候选人。",
        "姜涛说服务器硬盘快满了，要清理归档。",
        "范晓把会议室改到了 3 楼小会议室。",
        "石磊说他这周在外地，线上问题找赵敏。",
        "龙飞负责新版本的灰度名单，五十人。",
        "段誉说客户投诉响应时间太长，要求两小时内。",
        "侯亮把报价单发过来了，总价 8.6 万。",
        "邵华负责测试用例评审，明天下午两点。",
        "万青说这次发布要回滚，先别更新文档。",
        "顾佳负责客户答谢会，预算五万。",
        "毛磊说接口鉴权要改成 JWT。",
        "严静提醒我明天上午体检，要空腹。",
        "钱伟说他下周调岗，去新的数据组。",
    ]
    items = []
    for i, text in enumerate(raw, 1):
        # 人名取前三个汉字里的姓名部分：这里简单按已知名单匹配
        people = [n for n in KNOWN_NAMES if n in text]
        items.append({
            "id": f"A{i:03d}",
            "cat": "short",
            "text": text,
            "people": people,
        })
    return items


KNOWN_NAMES = [
    "张伟", "李娜", "王强", "陈静", "刘洋", "赵敏", "孙磊", "周涛", "吴倩", "郑凯",
    "冯磊", "许静", "何伟", "高翔", "林芳", "罗军", "梁静", "宋涛", "唐嫣", "韩磊",
    "曹阳", "邓超", "范冰", "彭勇", "曾毅", "蒋雯", "苏航", "卢明", "蔡琴", "贾斌",
    "丁宁", "魏强", "薛涛", "叶琳", "阎磊", "余静", "潘伟", "杜鹏", "戴敏", "夏雨",
    "钟磊", "汪洋", "田甜", "姜涛", "范晓", "石磊", "龙飞", "段誉", "侯亮", "邵华",
    "万青", "顾佳", "毛磊", "严静", "钱伟", "汤敏",
    "小王", "小明", "小张", "老周", "老李", "小陈", "陈总", "张姐", "老张", "小李",
    "阿强", "阿珍", "大刘", "小敏", "王建国",
]

# 去重并保持顺序：重复项会让"期望人名槽位"翻倍，recall 失真
KNOWN_NAMES = list(dict.fromkeys(KNOWN_NAMES))


def long_items():
    """长内容：3000-6000 字的多段会议记录 / 项目周报。"""
    blocks = [
        ("周一项目启动会",
         "周一上午九点半，项目启动会在三号会议室召开。参会的有产品负责人张伟、后端负责人李娜、"
         "测试负责人王强、运维代表陈静，以及外包团队的刘洋。张伟先讲了本季度的目标：把订单系统的"
         "平均响应时间从 800 毫秒压到 300 毫秒以内，同时把移动端的崩溃率降到千分之三以下。"
         "李娜认为瓶颈在数据库，尤其是订单查询那条 SQL 用了三个 JOIN，建议先做读写分离，"
         "再考虑把热点数据放到 Redis。王强提醒说压测环境只有两台机器，压不出真实流量，"
         "需要运维协调资源。陈静当场答应下周三之前把压测环境扩到六台，但要求测试团队提前一天"
         "提交资源申请单。刘洋说他那边负责的接口联调会晚两天，因为上游的支付渠道换了签名算法。"),
        ("周二技术评审",
         "周二下午两点，技术评审会继续。李娜把读写分离的方案讲了一遍：主库负责写入，两个从库"
         "负责读取，用中间件做路由，热点数据缓存 15 分钟。张伟问如果主从延迟超过一秒怎么办，"
         "李娜说会在写入后强制读主库，保证一致性。王强提出异议，认为强制读主库会让峰值流量"
         "打回主库，等于没分离。讨论的结果是先做灰度，把 20% 的读流量切到从库，观察一周。"
         "陈静补充说从库的磁盘是机械盘，随机读性能可能不够，建议换成 SSD。"),
        ("周三客户沟通",
         "周三上午十点，客户方的技术总监陈总带着两个工程师来到公司。陈总对数据安全特别在意，"
         "反复问日志里会不会落用户的手机号。李娜解释说手机号在入库前会做脱敏，只保留后四位，"
         "日志里只记录用户 ID。陈总还是要求出一份书面的数据流转说明，并且要签字确认。"
         "张伟答应本周五之前给初稿。客户的工程师问能不能提供 SDK，刘洋说 Android 和 iOS 的"
         "SDK 都已经在维护，但版本落后主端两个小版本，需要一周时间对齐。"),
        ("周四线上问题",
         "周四凌晨两点十七分，监控告警：订单创建接口的 P99 从 320 毫秒涨到 2.1 秒。值班的"
         "王强先看了数据库，发现慢查询里有大量按用户 ID 排序的全表扫描。他临时加了一个联合索引，"
         "P99 回落到 480 毫秒，但还没到基线。李娜早上八点接手排查，定位到是新上的推荐服务"
         "在每次下单时同步调用了三个外部接口，其中一个超时会拖慢整个链路。改成异步之后恢复。"
         "张伟要求把这次事故写成复盘文档，周五下午过一遍。"),
        ("周五复盘会",
         "周五下午四点，复盘会在线上开。王强先讲时间线：2:17 告警，2:23 定位到慢查询，"
         "2:41 加索引，3:05 恢复基线。李娜补充根因是同步调用外部服务没有设超时和降级。"
         "陈静提出监控的告警阈值定得太松，P99 涨到两秒才报，应该按基线的一倍半报警。"
         "会议定下三条改进：一是所有外部调用必须设 800 毫秒超时并配降级；二是压测纳入发布流程；"
         "三是每周五下午做一次容量回顾。责任人分别是李娜、王强、陈静，完成时间下周五。"),
        ("补充：预算与人力",
         "张伟还提到预算的事。本季度云资源预算是 42 万，目前已经用掉 31 万，剩下 11 万要撑到月底。"
         "陈静算了一笔账：六台压测机按每天 12 小时跑，一个月大概 2.4 万；日志存储按每天 40 GB 增长，"
         "一个月要 1.8 万；剩下的才是生产环境的余量。刘洋说外包团队下个月要加两个人，"
         "每人每月 2.6 万，两个月就是 10.4 万。张伟说要重新排优先级，先把压测机缩到四台。"),
        ("补充：文档与交付",
         "交付物方面，王强负责的测试报告要在下周三之前出，包含性能基线、压测结论和遗留风险。"
         "李娜负责的架构文档要更新读写分离那一章，另外补一份降级预案。刘洋负责接口文档，"
         "版本号从 v2.3 升到 v2.4，主要变化是支付回调的签名算法。陈静负责运维手册，"
         "把这次的排障过程写进去。张伟最后强调，文档不是写给领导看的，是写给三个月后"
         "半夜被叫起来排障的同事看的，所以必须写清楚怎么判断、怎么回滚。"),
    ]

    items = []
    # 组合出 12 条长文本，长度在 3000-6000 字之间
    for i in range(12):
        pick = blocks[i % len(blocks):] + blocks[:i % len(blocks)]
        body = "\n\n".join(f"【{t}】\n{c}" for t, c in pick)
        # 补一段结尾，让长度落在目标区间
        body += ("\n\n【附：补充说明】\n本次会议纪要由张伟整理，如有遗漏请在群里指出。"
                 "下次会议定在下周一上午九点半，地点还是三号会议室。")*1
        if len(body) < 3000:
            body += ("\n\n【附：历史背景】\n上一季度的目标是响应时间 500 毫秒，"
                     "实际达成 620 毫秒，未达标的主要原因是推荐服务上线时间推迟了两周。")*2
        items.append({
            "id": f"B{i+1:03d}",
            "cat": "long",
            "text": body,
            "people": ["张伟", "李娜", "王强", "陈静", "刘洋", "陈总"],
        })
    return items


def math_items():
    """数字公式 / 数学符号 / 单位 / 日期 / 金额。"""
    raw = [
        "项目预算 128000 元，按 3 个模块平均分摊，每个模块 42666.67 元。",
        "质能方程 E = mc²，其中 m = 2.5 kg，c ≈ 3×10⁸ m/s。",
        "样本均值 μ = (1/n)∑xᵢ，标准差 σ = √(∑(xᵢ-μ)²/n)。",
        r"损失函数对参数的偏导：\frac{\partial L}{\partial \theta} = 0",
        r"梯度下降更新：\theta_{t+1} = \theta_t - \eta \nabla L(\theta_t)",
        "增长率 12.5%，从 800 万增长到 900 万，绝对增量 100 万。",
        "汇率 1 USD = 7.18 CNY，100 美元约合 718 元人民币。",
        "二进制 1011 等于十进制 11，十六进制 0x1F 等于十进制 31。",
        "矩阵 A = [[1,2],[3,4]]，行列式 det(A) = 1×4 − 2×3 = −2。",
        "极限 lim(x→0) sin(x)/x = 1，这是重要极限之一。",
        "常用符号：± 正负、≈ 约等于、≠ 不等于、≤ 小于等于、≥ 大于等于、∞ 无穷。",
        "从 2026-11-15 到 2026-12-20 共 35 天，含首尾。",
        "服务器 16 核 32 线程，内存 64 GB，磁盘 2 TB，带宽 100 Mbps。",
        "缓存命中率 92.7%，平均响应时间从 320 ms 降到 48 ms，提升 85%。",
        "复利公式 A = P(1 + r/n)^(nt)，P=10000，r=5%，n=12，t=3 年。",
        "方差 Var(X) = E[X²] − (E[X])²，协方差 Cov(X,Y) = E[XY] − E[X]E[Y]。",
        "正态分布 N(μ, σ²)，68% 的数据落在 μ±σ 内，95% 落在 μ±2σ 内。",
        "面积换算：1 平方米 = 10000 平方厘米，1 公顷 = 10000 平方米。",
        "温度换算：摄氏度 C = (F − 32) × 5/9，212°F 等于 100°C。",
        "排序算法复杂度：快排平均 O(n log n)，最坏 O(n²)；归并稳定 O(n log n)。",
        r"积分：\int_0^1 x^2 dx = 1/3",
        r"求和：\sum_{i=1}^{n} i = \frac{n(n+1)}{2}",
        "概率：掷两枚骰子点数和为 7 的概率是 6/36 = 1/6 ≈ 16.67%。",
        "角度换算：180° = π 弧度，90° = π/2 弧度。",
        "哈希冲突率约 0.03%，在 100 万条数据下约 300 次碰撞。",
        "QPS 从 1200 提升到 4800，是原来的 4 倍；P99 从 800 ms 降到 210 ms。",
        "折扣计算：原价 299 元，打 8.5 折后 254.15 元，再减 20 元券为 234.15 元。",
        "带宽换算：100 Mbps ≈ 12.5 MB/s，传 1 GB 文件约需 82 秒。",
        "税率 13%，含税价 1130 元对应不含税价 1000 元，税额 130 元。",
        "坐标距离：A(1,2) 到 B(4,6) 的距离 = √((4−1)²+(6−2)²) = 5。",
        "对数：log₂1024 = 10，ln(e³) = 3，lg(1000) = 3。",
        "百分比误差：(实测值 − 理论值)/理论值 × 100% = (9.8−10)/10 = −2%。",
        "等差数列 1,4,7,...,100 共 34 项，和为 (1+100)×34/2 = 1717。",
    ]
    return [{"id": f"C{i+1:03d}", "cat": "math", "text": t, "people": []} for i, t in enumerate(raw)]


def relation_items():
    """复杂人物社会关系（含用户给出的标准样例）。"""
    raw = [
        # 用户给出的样例，原样保留
        "小王是小明的同事，小明的姐姐嫁给了小王，小明的姐姐的二姐夫小张是小王和小明的公司领导。",
        "老李是老张的上级，老张的爱人叫秀兰，秀兰的弟弟小舅子在一家做前端的公司上班。",
        "阿强和阿珍是夫妻，阿珍的表哥大刘是阿强的大学同学，大刘的妻子小敏在阿珍的公司做财务。",
        "部门里张伟管着李娜，李娜和刘洋是同批入职的同事，刘洋的妻子是张伟的表妹。",
        "赵敏的母亲和王强的父亲是兄妹，所以赵敏和王强是表兄妹，王强管赵敏叫表姐。",
        "陈静的丈夫是周涛的大学室友，周涛的女儿认陈静做干妈。",
        "孙磊的师父是吴倩的父亲，吴倩后来成了孙磊的部门主管。",
        "郑凯和刘洋是合伙人，郑凯管技术，刘洋管销售，两人的公司被冯磊的公司收购了。",
        "许静的嫂子是何伟的亲姐姐，何伟因此管许静叫嫂子。",
        "高翔和林芳是邻居，高翔的儿子和林芳的女儿在同一所小学同班。",
        "罗军的岳父是梁静的舅舅，所以罗军和梁静算远房亲戚。",
        "宋涛和唐嫣是前同事，唐嫣跳槽后成了宋涛现在公司的客户方负责人。",
        "韩磊的师兄曹阳介绍韩磊进了现在的公司，曹阳是技术委员会成员。",
        "邓超和范冰是双胞胎姐弟，邓超随父姓，范冰随母姓，外人常不知道两人是姐弟。",
        "彭勇是曾毅的担保人，曾毅创业时彭勇借了他三十万。",
        "蒋雯和苏航是夫妻，两人在同一个部门但不汇报给同一个人。",
        "卢明的继母是蔡琴的姑姑，卢明和蔡琴没有血缘关系但按辈分是表亲。",
        "贾斌和丁宁是师徒，丁宁入职时贾斌带了她半年。",
        "魏强的女儿嫁给了薛涛的儿子，两家成了亲家。",
        "叶琳和阎磊是竞争对手，两人都在争同一个总监的位置。",
        "余静是潘伟的前妻，两人离婚后仍在同一家公司，分属不同事业部。",
        "杜鹏的大学室友是戴敏的哥哥，杜鹏和戴敏因此认识。",
        "夏雨的干爹是钟磊的生意伙伴，钟磊对夏雨多有照拂。",
        "汪洋和田甜是合租室友，田甜后来介绍汪洋进了自己的公司。",
        "姜涛是范晓的直属上级，范晓的另一位上级是石磊，两人是矩阵管理关系。",
    ]
    return [{"id": f"D{i+1:03d}", "cat": "relation", "text": t,
             "people": [n for n in KNOWN_NAMES if n in t]} for i, t in enumerate(raw)]


def person_items():
    """多人对话 / 别名 / 称谓混用。"""
    raw = [
        "王建国是我们研发部的后端，大家都叫他老王或者建国。他今天说接口联调完了。",
        "李娜的英文名是 Nina，客户那边都叫她 Nina，本名反而没人记得。",
        "陈静让我叫她静姐，她是财务部的老人，报销的事找她准没错。",
        "刘洋有个外号叫大刘，因为部门里还有个刘洋，两个人靠外号区分。",
        "赵敏是赵总的女儿，但她不喜欢别人提这层关系，坚持让大家叫她小赵。",
        "孙磊和张伟、王强三个人是同一批入职的，号称铁三角。",
        "周涛的助理姓林，我们都叫林助理，具体名字我一直没记住。",
        "吴倩从上海调过来，带着原来的团队三个人一起过来。",
        "郑凯说他老板是冯磊，冯磊上面还有一位分管副总裁姓何。",
        "许静、何伟和梁静组成了新的增长小组，许静任组长。",
        "高翔负责客户端，林芳负责服务端，两人每周三同步一次进度。",
        "罗军和宋涛是师徒关系，罗军带宋涛做支付渠道对接。",
        "唐嫣刚休完产假回来，暂时由韩磊代管她的模块。",
        "曹阳向邓超汇报，邓超向范冰汇报，范冰直接向 CEO 汇报。",
        "彭勇是外部顾问，合同签在曾毅名下，按天计费。",
        "蒋雯和苏航都参加了上周的架构评审，蒋雯提了三条反对意见。",
        "贾斌把丁宁推荐给了魏强，丁宁现在在魏强的组里做数据。",
        "薛涛和叶琳是同一天入职的，工号只差一位。",
        "阎磊负责安全合规，余静负责法务，两人经常一起审合同。",
        "潘伟的团队有八个人，其中三个是实习生，杜鹏是其中之一。",
        "戴敏和夏雨是大学同学，毕业后又进了同一家公司。",
        "钟磊说话很慢，汪洋说话很快，两人搭档做客户演示效果很好。",
        "田甜负责招聘，姜涛负责面试，两人配合招后端。",
        "范晓和石磊是夫妻，但公司里只有 HR 知道这件事。",
        "龙飞是新来的实习生，段誉是他的导师，侯亮是他的组长。",
        "邵华把万青介绍给了顾佳，万青现在在顾佳的团队做运营。",
        "毛磊和严静是同一个项目组的，毛磊写后端，严静写前端。",
        "钱伟和汤敏一起负责这次的迁移，钱伟管数据，汤敏管脚本。",
        "小王今天说小明借了他的车，还没还。",
        "小明说他姐姐下个月结婚，对象就是小王。",
        "小张是小王和小明的领导，两人都得向他汇报。",
        "老周、老李和小陈中午一起吃饭，聊到下周三的评审会。",
        "张姐说她下周出差，组里的事暂时交给小李。",
        "陈总那边催得紧，要求月底前看到方案。",
        "阿强说他老婆阿珍下周生日，问我们有什么建议。",
        "大刘和小敏打算年底买房，正在看学区。",
        "张伟今天请假，他的活暂时交给李娜。",
        "李娜说她下周要去总部汇报，让我替她盯一下值班。",
        "王强把服务器密码改了，新密码发在群里。",
        "陈静提醒报销单要贴发票原件，复印件不行。",
        "刘洋的电脑中病毒了，IT 正在重装系统。",
        "赵敏说客户的合同条款要改，让我们等法务回复。",
        "孙磊把接口的超时时间从 3 秒改成了 800 毫秒。",
        "周涛说他下周出差，周会改到周四。",
        "吴倩把培训材料发群里了，让大家提前看一遍。",
        "郑凯说这次发布不放假，周末要值班。",
        "冯磊在群里表扬了何伟，说他排查问题很快。",
        "许静说新来的实习生下周报到，让我准备工位。",
        "何伟和梁静吵架了，因为一个接口的字段命名。",
        "高翔请假三天，他的代码由林芳接手。",
    ]
    items = []
    for i, text in enumerate(raw, 1):
        items.append({
            "id": f"E{i:03d}",
            "cat": "person",
            "text": text,
            "people": [n for n in KNOWN_NAMES if n in text],
        })
    return items


def contact_items():
    """联系人专项：别名、称谓、同名、婚嫁、关系链、矩阵汇报。

    这一组是"联系人功能"的核心考题：
    - 同一人多种叫法能否归并成一条（本名/英文名/昵称/职务）；
    - 同名不同人能否区分（两个刘洋）；
    - 亲属与姻亲关系能否抽成 related_contacts；
    - 汇报线/矩阵管理能否表达上下级。
    """
    raw = [
        # 别名与称谓
        ("王建国是我们的后端负责人，大家都叫他老王，英文名 Kevin，客户那边叫他王工。",
         ["王建国", "老王", "王工"]),
        ("李娜的英文名是 Nina，海外团队只认这个名字，国内同事还是叫她李娜。",
         ["李娜", "Nina"]),
        ("陈静让我叫她静姐，她是财务部的老人，报销的事找她准没错。",
         ["陈静", "静姐"]),
        ("刘洋有两个，前端刘洋和外号大刘的后端刘洋，别搞混了。",
         ["刘洋", "大刘"]),
        ("张伟私下被叫做张大炮，因为他开会嗓门大。",
         ["张伟", "张大炮"]),
        ("赵敏是赵总的女儿，但她坚持让大家叫她小赵，不要提这层关系。",
         ["赵敏", "赵总", "小赵"]),
        ("孙磊对外用英文名 Leo，内部系统里还是孙磊。",
         ["孙磊", "Leo"]),
        ("周涛升了总监之后，大家都改口叫周总，只有老同事还叫他涛哥。",
         ["周涛", "周总", "涛哥"]),
        # 婚嫁与亲属
        ("小明的姐姐嫁给了小王，所以小王是小明的姐夫。",
         ["小明", "小王"]),
        ("老陈的岳父是老李，老李退休前是车间主任。",
         ["老陈", "老李"]),
        ("阿珍的婆婆姓周，周阿姨每周三来帮忙带孩子。",
         ["阿珍", "周阿姨"]),
        ("何伟的嫂子是许静的亲姐姐，所以何伟管许静叫嫂子。",
         ["何伟", "许静"]),
        ("郑凯的连襟是冯磊，两人娶了一对姐妹。",
         ["郑凯", "冯磊"]),
        ("梁静的儿子今年上小学，班主任姓方。",
         ["梁静", "方老师"]),
        # 关系链
        ("高翔的上司是林芳，林芳汇报给罗军，罗军直接向 CEO 汇报。",
         ["高翔", "林芳", "罗军"]),
        ("宋涛是唐嫣带出来的徒弟，唐嫣的师父又是韩磊。",
         ["宋涛", "唐嫣", "韩磊"]),
        ("曹阳把邓超推荐给了范冰，邓超现在在范冰的团队做数据。",
         ["曹阳", "邓超", "范冰"]),
        ("彭勇和曾毅是大学室友，曾毅的创业合伙人又是蒋雯。",
         ["彭勇", "曾毅", "蒋雯"]),
        ("苏航的直属上级是卢明，虚线汇报给蔡琴，属于矩阵管理。",
         ["苏航", "卢明", "蔡琴"]),
        ("贾斌带过丁宁半年，丁宁后来调到了魏强组里。",
         ["贾斌", "丁宁", "魏强"]),
        ("薛涛和叶琳是同级，都向阎磊汇报，两人互相不服。",
         ["薛涛", "叶琳", "阎磊"]),
        ("余静负责法务，潘伟负责合规，两人共同向杜鹏汇报。",
         ["余静", "潘伟", "杜鹏"]),
        # 同事与项目
        ("戴敏和夏雨是同一个项目组的，戴敏写后端，夏雨写前端。",
         ["戴敏", "夏雨"]),
        ("钟磊是汪洋的替补，汪洋休假期间由钟磊顶班。",
         ["钟磊", "汪洋"]),
        ("田甜负责招聘，姜涛负责终面，两人配合招后端。",
         ["田甜", "姜涛"]),
        ("范晓和石磊是夫妻，但公司里只有 HR 知道。",
         ["范晓", "石磊"]),
        ("龙飞是新来的实习生，段誉带他，侯亮是组长。",
         ["龙飞", "段誉", "侯亮"]),
        ("邵华把万青介绍给了顾佳，万青现在在顾佳的团队做运营。",
         ["邵华", "万青", "顾佳"]),
        ("毛磊和严静一起做迁移，毛磊管数据，严静管脚本。",
         ["毛磊", "严静"]),
        ("钱伟和汤敏是前同事，汤敏跳槽后成了我们的客户方负责人。",
         ["钱伟", "汤敏"]),
        # 同名 / 指代 / 澄清
        ("我们组有两个王强，一个做测试一个做运维，今天说的是做测试的那个王强。",
         ["王强"]),
        ("小张不是张伟，是另一个同事，大家为了区分就叫小张。",
         ["小张", "张伟"]),
        ("老李和李娜没有亲戚关系，只是同姓。",
         ["老李", "李娜"]),
        ("陈总说的是陈静，不是客户那边的陈总，别弄混。",
         ["陈总", "陈静"]),
        # 身份变化
        ("罗军上个月从技术经理升成了技术总监，现在管三个组。",
         ["罗军"]),
        ("梁静提了离职，最后工作日是下个月十号，交接给叶琳。",
         ["梁静", "叶琳"]),
        ("万青从外包转成了正式员工，工号重新分配。",
         ["万青"]),
        ("阎磊调去了新加坡分部，国内的安全合规暂时由余静代管。",
         ["阎磊", "余静"]),
        ("宋涛接替了邓超的岗位，邓超转去做架构。",
         ["宋涛", "邓超"]),
        ("顾佳休产假期间，客户答谢会由田甜牵头。",
         ["顾佳", "田甜"]),
        ("范冰退休返聘，只做顾问，不再管具体项目。",
         ["范冰"]),
        ("彭勇的合同到期不续了，交接给新来的顾问老徐。",
         ["彭勇", "老徐"]),
    ]
    items = []
    for i, (text, people) in enumerate(raw, 1):
        items.append({
            "id": f"G{i:03d}",
            "cat": "contact",
            "text": text,
            "people": people,
        })
    return items


def edge_items():
    """边界输入：emoji、纯英文、代码、URL、标点、超长单句等。"""
    long_single = "今天开会讨论了很多事情" + "包括架构调整和人员安排" * 180 + "最后没有结论"
    raw = [
        ("emoji 混排", "今天和 😀 张伟 👨‍💻 一起 review 了代码，他说 🚀 下周上线，让我 👀 盯着监控。", ["张伟"]),
        ("纯英文", "John and Mary discussed the API redesign. John will send the spec by Friday.", []),
        ("中英混排", "Alice 负责 K8s 集群，她说 Pod 老是 OOM，让我帮忙看下 memory limit。", []),
        ("代码块", "```python\ndef add(a, b):\n    return a + b\n```\n这段代码是张伟写的，他说还要加类型注解。", ["张伟"]),
        ("JSON 文本", '{"name": "李娜", "role": "backend", "team": "order"} 这是李娜的档案。', ["李娜"]),
        ("URL", "文档在这里 https://example.com/docs/api/v2?token=abc123 王强说链接会过期。", ["王强"]),
        ("HTML 标签", "<div class='card'>陈静</div><script>alert(1)</script> 陈静说这个页面要重做。", ["陈静"]),
        ("SQL 片段", "SELECT * FROM users WHERE name = '刘洋' AND status = 1; 刘洋说这条 SQL 没走索引。", ["刘洋"]),
        ("纯标点", "……！！！？？？——、、、", []),
        ("单字", "好", []),
        ("重复串", "哈哈" * 200, []),
        ("表格文本", "| 姓名 | 负责模块 |\n|---|---|\n| 赵敏 | 支付 |\n| 孙磊 | 订单 |\n两人下周开始联调。", ["赵敏", "孙磊"]),
        ("Markdown 列表", "- 周涛 负责网关\n- 吴倩 负责网关\n- 郑凯 负责文档\n三人是一个小组。", ["周涛", "吴倩", "郑凯"]),
        ("数字轰炸", "1234567890 " * 100, []),
        ("超长单句", long_single, []),
        ("换行分隔", "冯磊\n负责\n运维\n他\n说\n周五\n巡检", ["冯磊"]),
        ("引号嵌套", "许静说：\"何伟跟我讲 '这个 bug 不修不行'，你怎么看？\"", ["许静", "何伟"]),
        ("时间密集", "2026-11-01 09:00 高翔开会，2026-11-02 14:30 林芳评审，2026-11-03 18:00 罗军上线。", ["高翔", "林芳", "罗军"]),
        ("全角数字", "预算 １２８０００ 元，宋涛说要砍掉三成。", ["宋涛"]),
        ("特殊符号", "∑∫√≤≥≠±×÷∞ 唐嫣说这些符号在文档里显示乱码。", ["唐嫣"]),
        ("重名场景", "这里有两个刘洋，一个做前端一个做后端，前端刘洋今天请假。", ["刘洋"]),
        ("身份切换", "我既是韩磊的同事，也是他的房东，他租的是我家次卧。", ["韩磊"]),
        ("否定表述", "曹阳不是我的上级，邓超才是；曹阳只是项目上的接口人。", ["曹阳", "邓超"]),
        ("长英文段落", "The quarterly review meeting covered latency, throughput, and cost. " * 40, []),
        ("混合换行与空行", "第一段：范冰说明了目标。\n\n\n第二段：彭勇补充了风险。\n\n第三段：结论是下月再看。", ["范冰", "彭勇"]),
    ]
    return [{"id": f"F{i+1:03d}", "cat": "edge", "text": t, "people": p, "note": n}
            for i, (n, t, p) in enumerate(raw)]


def build_corpus():
    items = []
    items += short_items()
    items += long_items()
    items += math_items()
    items += relation_items()
    items += person_items()
    items += contact_items()
    items += edge_items()
    return items


# --------------------------------------------------------------------------
# HTTP
# --------------------------------------------------------------------------

def _req(method, url, payload=None, timeout=120):
    data = None
    headers = {"Accept": "application/json"}
    if payload is not None:
        data = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            body = resp.read().decode("utf-8", "replace")
            return resp.status, json.loads(body) if body.strip() else {}
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", "replace")
        try:
            return e.code, json.loads(body)
        except Exception:
            return e.code, {"raw": body}
    except Exception as e:
        return 0, {"error": str(e)}


def post_add(base, text, user, max_retries=12):
    """提交一条对话，遇到限流按节奏重试。

    服务端默认限流 60 次/分钟（[security] rate_limit），
    批量压测必须遵守，否则后面的请求全部 200+success:false 而不会被处理。
    """
    last = None
    for attempt in range(max_retries):
        code, resp = _req("POST", f"{base}/api/memory/add",
                          {"conversation": text, "user_id": user})
        if isinstance(resp, dict) and resp.get("success") and (resp.get("data") or {}).get("conversation_id"):
            return code, resp
        msg = str(resp)
        last = resp
        if "Rate limit" in msg or "限流" in msg:
            time.sleep(2.5)
            continue
        # 其他错误不重试（内容非法等）
        return code, resp
    return 0, last


def submit(base, user, items):
    # 断点续传：已拿到 conversation_id 的条目跳过，避免重复入库
    sf = state_path(user)
    if os.path.exists(sf):
        with open(sf, encoding="utf-8") as f:
            state = json.load(f)
        state["user"] = user
        state["base"] = base
    else:
        state = {"user": user, "base": base, "items": {}}
    ok = 0
    for i, it in enumerate(items, 1):
        prev = state["items"].get(it["id"])
        if prev and prev.get("cid"):
            ok += 1
            continue
        code, resp = post_add(base, it["text"], user)
        cid = None
        if isinstance(resp, dict):
            data = resp.get("data") or {}
            cid = data.get("conversation_id")
        if code == 200 and cid:
            state["items"][it["id"]] = {
                "cid": cid, "cat": it["cat"], "chars": len(it["text"]),
                "people": it["people"], "text": it["text"][:200],
            }
            ok += 1
        else:
            state["items"][it["id"]] = {
                "cid": None, "cat": it["cat"], "chars": len(it["text"]),
                "people": it["people"], "text": it["text"][:200],
                "error": str(resp)[:200],
            }
        if i % 20 == 0:
            print(f"  已处理 {i}/{len(items)}（已入库 {ok}）", flush=True)
        time.sleep(0.05)
    with open(sf, "w", encoding="utf-8") as f:
        json.dump(state, f, ensure_ascii=False, indent=1)
    print(f"提交完成：成功 {ok} / {len(items)}，状态写入 {sf}")
    return state


def poll(base, state, interval=20, timeout_min=180):
    items = state["items"]
    deadline = time.time() + timeout_min * 60
    last_done = -1
    while time.time() < deadline:
        done = failed = pending = 0
        for aid, info in items.items():
            cid = info.get("cid")
            if not cid:
                failed += 1
                info["status"] = "submit_failed"
                continue
            st = info.get("status")
            if st in ("completed", "failed"):
                if st == "completed":
                    done += 1
                else:
                    failed += 1
                continue
            code, resp = _req("GET", f"{base}/api/memory/extract_status/{cid}", timeout=30)
            status = None
            if isinstance(resp, dict):
                status = (resp.get("data") or {}).get("status")
            if status in ("completed", "failed"):
                info["status"] = status
                if status == "completed":
                    done += 1
                else:
                    failed += 1
            else:
                pending += 1
                info["status"] = status or "unknown"
        print(f"[{time.strftime('%H:%M:%S')}] 完成 {done} 失败 {failed} 进行中 {pending}", flush=True)
        with open(state_path(state.get("user", "")), "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=1)
        if pending == 0:
            print("全部到达终态")
            return state
        if done == last_done and pending > 0:
            print("  警告：本轮无进展，抽取可能在排队或卡住", flush=True)
        last_done = done
        time.sleep(interval)
    print("轮询超时，仍有未完成项")
    return state


# --------------------------------------------------------------------------
# 校验
# --------------------------------------------------------------------------

def fetch_contacts(base, user, limit=200, include_archived=True):
    code, resp = _req("POST", f"{base}/api/memory/search/contacts",
                      {"query": "", "limit": limit, "user_id": user,
                       "include_archived": include_archived})
    return (resp.get("data") or []) if isinstance(resp, dict) else []


def verify(base, user, state):
    items = state["items"]
    corpus_text = "\n".join(v.get("text", "") for v in items.values())
    # 需要完整原文判断"是否凭空捏造"，用 build_corpus 重新生成
    full_text = "\n".join(it["text"] for it in build_corpus())

    report = {"generated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
              "user": user, "checks": {}}

    # ---- 0. 抽取完成情况 ----
    total = len(items)
    done = sum(1 for v in items.values() if v.get("status") == "completed")
    failed = sum(1 for v in items.values() if v.get("status") == "failed")
    report["checks"]["extraction"] = {
        "total": total, "completed": done, "failed": failed,
        "pass": done == total,
    }

    # ---- 1. 联系人是否创建 ----
    contacts = fetch_contacts(base, user)
    names = [c.get("name", "") for c in contacts]
    name_set = set(names)
    # 别名也算命中
    alias_map = {}
    for c in contacts:
        for a in (c.get("aliases") or []):
            alias_map.setdefault(a, c.get("name"))

    expect_total = hit_total = 0
    missing = []
    for aid, info in items.items():
        for p in (info.get("people") or []):
            expect_total += 1
            if p in name_set or p in alias_map:
                hit_total += 1
            else:
                missing.append(f"{aid}:{p}")
    recall = (hit_total / expect_total) if expect_total else 0.0

    # 虚构联系人：名字（含别名）在任何原文中都不出现
    suspicious = []
    for c in contacts:
        n = c.get("name", "")
        if not n:
            continue
        if n in full_text:
            continue
        if any(a and a in full_text for a in (c.get("aliases") or [])):
            continue
        suspicious.append(n)

    report["checks"]["contacts"] = {
        "contact_count": len(contacts),
        "expected_person_slots": expect_total,
        "matched": hit_total,
        "recall": round(recall, 4),
        "missing_examples": missing[:30],
        "suspicious_count": len(suspicious),
        "suspicious_examples": suspicious[:20],
        "is_virtual_flags": sum(1 for c in contacts if c.get("is_virtual")),
    }

    # ---- 2. 人设是否生成 ----
    with_profile = [c for c in contacts if (c.get("background") or "").strip()]
    structured = 0
    for c in with_profile:
        try:
            d = json.loads(c["background"])
            if isinstance(d, dict) and ("personal_info" in d or "social_relations" in d):
                structured += 1
        except Exception:
            pass
    with_related = [c for c in contacts if (c.get("related_contacts") or [])]
    with_tags = [c for c in contacts if (c.get("relationship_tags") or [])]
    report["checks"]["profiles"] = {
        "contacts": len(contacts),
        "with_background": len(with_profile),
        "structured_json": structured,
        "with_related_contacts": len(with_related),
        "with_relationship_tags": len(with_tags),
        "coverage": round(len(with_profile) / len(contacts), 4) if contacts else 0,
        "samples": [
            {"name": c["name"], "len": len(c.get("background") or ""),
             "tags": c.get("relationship_tags"), "related": c.get("related_contacts")}
            for c in sorted(with_profile, key=lambda x: -len(x.get("background") or ""))[:8]
        ],
    }

    # ---- 3. 虚拟人物区分 ----
    vname = "苏晚晴"
    code, resp = _req("POST", f"{base}/api/virtual_entities",
                      {"name": vname,
                       "aliases": [{"name": "晚晴", "alias_type": "nickname"}],
                       "user_id": user})
    vid = None
    if isinstance(resp, dict):
        vid = (resp.get("data") or {}).get("entity_id")
    # 让一段真实对话提到这个虚拟角色，检查两件事：
    # 1) 是否被当成新联系人重复创建（同名两条）
    # 2) is_virtual 标记是否被抽取链路覆盖成 false
    v_cid = None
    if vid:
        code, vresp = post_add(base, f"{vname}今天来公司了，她说下周要出差三天，让我帮她订票。", user)
        if isinstance(vresp, dict):
            v_cid = (vresp.get("data") or {}).get("conversation_id")
        for _ in range(30):
            if not v_cid:
                break
            c2, r2 = _req("GET", f"{base}/api/memory/extract_status/{v_cid}", timeout=30)
            if (r2.get("data") or {}).get("status") in ("completed", "failed"):
                break
            time.sleep(6)

    contacts2 = fetch_contacts(base, user)
    v_matches = [c for c in contacts2 if c.get("name") == vname]
    vrec = v_matches[0] if v_matches else None
    real = next((c for c in contacts2 if c.get("name") not in (vname,) and not c.get("is_virtual")), None)

    # 详情接口对虚拟人物也要标出来
    v_detail_virtual = None
    if vrec:
        code, dresp = _req("GET", f"{base}/api/contacts/{vrec['contact_id']}/detail?user_id={user}")
        v_detail_virtual = ((dresp.get("data") or {}).get("contact") or {}).get("is_virtual")

    report["checks"]["virtual"] = {
        "create_ok": vid is not None,
        "entity_id": vid,
        "found_in_contact_list": vrec is not None,
        "is_virtual_flag": (vrec or {}).get("is_virtual"),
        "detail_is_virtual": v_detail_virtual,
        "real_contact_is_virtual": (real or {}).get("is_virtual"),
        "mention_conversation": v_cid,
        "duplicates_after_mention": len(v_matches),
        "is_virtual_after_mention": (vrec or {}).get("is_virtual"),
        "pass": bool(vrec and vrec.get("is_virtual") and v_detail_virtual
                     and real and not real.get("is_virtual")
                     and len(v_matches) == 1),
    }

    # ---- 4. 联系人详情：关联联系人 / 事件 / 记忆 ----
    # 优先挑一个有 related_contacts 的联系人
    target = None
    for c in contacts:
        if (c.get("related_contacts") or []) and c.get("name") != vname:
            target = c
            break
    if not target:
        target = next((c for c in contacts if c.get("name") != vname), None)

    detail = {}
    if target:
        code, dresp = _req(
            "GET", f"{base}/api/contacts/{target['contact_id']}/detail"
                   f"?user_id={user}&event_limit=20&memory_limit=10")
        detail = dresp.get("data") or {}
    report["checks"]["contact_detail"] = {
        "target": target.get("name") if target else None,
        "contact_id": target.get("contact_id") if target else None,
        "has_contact_block": bool(detail.get("contact")),
        "related_contacts": len(detail.get("related_contacts") or []),
        "events": len(detail.get("events") or []),
        "memories": len(detail.get("memories") or []),
        "stats": detail.get("stats"),
        "sample_event": ((detail.get("events") or [{}])[0].get("title") or "")[:40],
        "sample_memory": ((detail.get("memories") or [{}])[0].get("snippet") or "")[:60],
        "pass": bool(detail.get("contact")) and (
            len(detail.get("events") or []) > 0 or len(detail.get("memories") or []) > 0
        ),
    }

    # ---- 5. 关系样例专项（用户给的例子） ----
    sample = "小王是小明的同事，小明的姐姐嫁给了小王，小明的姐姐的二姐夫小张是小王和小明的公司领导。"
    idx = next((aid for aid, v in items.items() if sample[:20] in v.get("text", "")), None)
    rel = {"item": idx}
    if idx:
        rel["status"] = items[idx].get("status")
        rel["people"] = items[idx].get("people")
        rel["created"] = [n for n in ("小王", "小明", "小张") if n in name_set]
        # 小王/小明/小张 之间的关系
        for who in ("小王", "小明", "小张"):
            c = next((x for x in contacts if x.get("name") == who), None)
            rel[who] = {
                "exists": c is not None,
                "aliases": (c or {}).get("aliases"),
                "tags": (c or {}).get("relationship_tags"),
                "related": (c or {}).get("related_contacts"),
                "background_len": len((c or {}).get("background") or ""),
            }
    report["checks"]["relation_sample"] = rel

    # ---- 6. 待办与事件量 ----
    code, tresp = _req("GET", f"{base}/api/todos?user_id={user}&limit=200")
    todos = (tresp.get("data") or []) if isinstance(tresp, dict) else []
    code, eresp = _req("POST", f"{base}/api/memory/search/events",
                       {"limit": 200, "user_id": user})
    events = (eresp.get("data") or []) if isinstance(eresp, dict) else []
    report["checks"]["volume"] = {
        "todos": len(todos),
        "events": len(events),
        "contacts": len(contacts),
    }

    out = "docs/stress200_report.json"
    os.makedirs("docs", exist_ok=True)
    with open(out, "w", encoding="utf-8") as f:
        json.dump(report, f, ensure_ascii=False, indent=2)
    print(json.dumps(report, ensure_ascii=False, indent=2))
    print(f"\n报告写入 {out}")
    return report


# --------------------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["submit", "poll", "verify", "all", "list"])
    ap.add_argument("--base", default="http://127.0.0.1:3000")
    ap.add_argument("--user", default="stress200")
    ap.add_argument("--timeout-min", type=int, default=180)
    ap.add_argument("--interval", type=int, default=20)
    args = ap.parse_args()

    corpus = build_corpus()
    if args.cmd == "list":
        from collections import Counter
        c = Counter(it["cat"] for it in corpus)
        print("条目数:", len(corpus), dict(c))
        for it in corpus[:3]:
            print(it["id"], it["cat"], len(it["text"]), it["text"][:40])
        return

    if args.cmd == "submit":
        submit(args.base, args.user, corpus)
        return

    sf = state_path(args.user)
    if not os.path.exists(sf):
        print(f"缺少 {sf}，请先 submit")
        sys.exit(1)
    with open(sf, encoding="utf-8") as f:
        state = json.load(f)

    if args.cmd == "poll":
        poll(args.base, state, args.interval, args.timeout_min)
    elif args.cmd == "verify":
        verify(args.base, args.user, state)
    elif args.cmd == "all":
        if not state["items"]:
            state = submit(args.base, args.user, corpus)
        poll(args.base, state, args.interval, args.timeout_min)
        verify(args.base, args.user, state)


if __name__ == "__main__":
    main()
