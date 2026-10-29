"""守心验收语料 v2 —— 210 条，全新构造，不复用 stress_200.py 的任何用例。

设计目标（对应验收审计的六项）：
1. 联系人创建：每条都带期望人名，可算召回与捏造率；
2. 别名识别：20+ 条一人多称，统计"别名能否归并到同一联系人 / 用别名能否搜到本人"；
3. 人物画像：同一人多次提及且信息递进，用于验证画像生成与更新；
4. 多联系人关系：35 条显式关系陈述，验证 related_contact_ids；
5. 时间线索引：同一人跨时间多事件（近 7 天 / 30 天内 / 90 天前 / 去年），
   用于验证"查某人近一个月的事"；
6. API 全功能：待办、事件、长文、边界、同名、虚拟角色。

字段说明：
- people   期望抽到的姓名（主名）
- aliases  [(主名, 别名)]，用于别名识别率
- events   [(关键词, 天数偏移)]，偏移为负表示"多少天前"，None 表示时间不明
- todos    期望抽到的待办关键词
- relations [(A, B)]，期望 A 的关联联系人里出现 B（或反向）
"""

# 人物池：全部新造，与旧语料不重复
PEOPLE = [
    "沈亦舟", "程澜", "傅思远", "钟毓", "邵北", "黎晚", "霍云开", "薛宁",
    "房晓", "秦朗", "尹柯", "路遥", "章鸣", "阮青", "包蓉", "苗禾",
    "简宁", "岑野", "柯明", "滕飞", "屈洋", "汤沐", "闻笙", "岑溪",
    "商陆", "迟野", "柏川", "容与", "江照", "温言", "白露", "顾青禾",
    "沈砚", "宋辞", "陆知遥", "许愿", "周砚", "叶知秋", "唐棣", "苏木",
]

EVENTS = [
    "季度复盘会", "需求评审", "灰度发布", "线上故障排查", "客户回访",
    "预算答辩", "招聘终面", "合同评审", "数据迁移", "接口联调",
    "安全渗透测试", "产品灰度", "供应商比价", "年度体检", "团队建设",
    "专利申报", "服务器扩容", "版本冻结", "绩效面谈", "竞品分析",
]

TIMES = [
    "今天上午", "昨天下午", "上周三", "本周五", "下周一", "下个月初",
    "月底", "三天后", "两周前", "上个月中旬", "去年十一月", "明天上午十点",
]

TODOS = [
    "提交接口文档", "准备季度报表", "回复客户邮件", "更新部署脚本", "整理会议纪要",
    "完成预算表", "预约体检", "提交专利材料", "修复登录缺陷", "核对合同条款",
    "安排面试", "导出埋点数据", "写复盘总结", "采购服务器", "更新值班表",
]

FACTS = [
    "更喜欢书面沟通，重要结论要落到文档里",
    "习惯早上七点看邮件，紧急事直接打电话",
    "对数字很敏感，报表小数点必须对齐",
    "不太用即时通讯，找他走邮件最稳",
    "出差频繁，每周有一半时间在客户现场",
    "中文写作很讲究，不喜欢口语化表达",
    "负责预算口径，任何对外承诺先过他这一关",
    "对新方案保守，必须有对比数据才点头",
    "记性特别好，一年前说过的细节都能复述",
    "带团队偏放手，只在大方向上把关",
    "喜欢当面沟通，重要事情约会议室",
    "对截止时间极敏感，延期一天都会追问",
    "擅长处理客户投诉，话术很稳",
    "技术出身，讨论问题喜欢先看日志",
    "习惯把风险写在最前面，方案里必须有兜底",
]

ALIAS_STYLES = [
    lambda n, a: f"{n}，大家都叫他{a}。",
    lambda n, a: f"{n}让人叫他{a}，说名字太正式。",
    lambda n, a: f"{n}（{a}）今天来公司了。",
    lambda n, a: f"{n}外号{a}，因为说话特别快。",
    lambda n, a: f"客户那边一直称{n}为{a}。",
]

EN_NAMES = ["Ethan", "Iris", "Owen", "Cora", "Felix", "Nora", "Hugo", "Vera", "Milo", "Jade",
            "Rex", "Luna", "Neil", "Ada", "Cole", "Rita", "Dean", "Zoe", "Karl", "Mona"]

REL_TEMPLATES = [
    ("直属上级", lambda a, b: f"{a}是{b}的直属上级，{b}的周报直接发给{a}。"),
    ("同项目组", lambda a, b: f"{a}和{b}在同一个项目组，{a}负责后端，{b}负责前端。"),
    ("大学室友", lambda a, b: f"{a}和{b}是大学室友，毕业十年还每年聚一次。"),
    ("客户方负责人", lambda a, b: f"{a}是我们这边的对接人，{b}是客户方的负责人。"),
    ("带过徒弟", lambda a, b: f"{a}带过{b}半年，{b}现在能独立负责模块了。"),
    ("表亲", lambda a, b: f"{a}的爱人是{b}的表姐，两家逢年过节都一起吃饭。"),
]


def _case(cid, cat, text, people=None, aliases=None, events=None,
          todos=None, relations=None, note=""):
    return {
        "id": cid, "cat": cat, "text": text,
        "people": people or [],
        "aliases": aliases or [],
        "events": events or [],
        "todos": todos or [],
        "relations": relations or [],
        "note": note,
    }


def handwritten():
    """手写核心用例：别名、关系、时间线、同名、虚拟角色、边界、长文。"""
    out = []

    # ---------- 别名（每人多称，用于统计别名识别率） ----------
    alias_cases = [
        ("沈亦舟", "老沈"), ("程澜", "小程"), ("傅思远", "老傅"),
        ("钟毓", "阿毓"), ("邵北", "北哥"), ("黎晚", "晚晚"),
        ("霍云开", "老霍"), ("薛宁", "宁姐"), ("房晓", "晓晓"),
        ("秦朗", "朗哥"), ("尹柯", "小柯"), ("路遥", "老路"),
        ("章鸣", "鸣哥"), ("阮青", "青姐"), ("包蓉", "蓉蓉"),
        ("苗禾", "禾苗"), ("简宁", "简明"), ("岑野", "野哥"),
        ("柯明", "老柯"), ("滕飞", "飞哥"),
    ]
    for i, (name, alias) in enumerate(alias_cases):
        style = ALIAS_STYLES[i % len(ALIAS_STYLES)]
        text = style(name, alias) + f"这次{TIMES[i % 6]}的{EVENTS[i % len(EVENTS)]}由他跟进。"
        out.append(_case(f"A{i+1:03d}", "alias", text,
                         people=[name], aliases=[(name, alias)],
                         events=[(EVENTS[i % len(EVENTS)], None)]))

    # 英文名 + 中文名并存（同一人，需要归并）
    for i, name in enumerate(PEOPLE[:10]):
        en = EN_NAMES[i]
        text = f"{name}的英文名是{en}，海外团队只认这个，国内同事还是叫{name}。"
        out.append(_case(f"A1{i+1:02d}", "alias", text,
                         people=[name], aliases=[(name, en)]))

    # ---------- 关系（显式陈述） ----------
    pairs = [
        ("沈亦舟", "程澜"), ("傅思远", "钟毓"), ("邵北", "黎晚"),
        ("霍云开", "薛宁"), ("房晓", "秦朗"), ("尹柯", "路遥"),
        ("章鸣", "阮青"), ("包蓉", "苗禾"), ("简宁", "岑野"),
        ("柯明", "滕飞"), ("屈洋", "汤沐"), ("闻笙", "岑溪"),
        ("商陆", "迟野"), ("柏川", "容与"), ("江照", "温言"),
    ]
    for i, (a, b) in enumerate(pairs):
        kind, tpl = REL_TEMPLATES[i % len(REL_TEMPLATES)]
        text = tpl(a, b) + f" 最近一起忙{EVENTS[i % len(EVENTS)]}。"
        out.append(_case(f"R{i+1:03d}", "relation", text,
                         people=[a, b], events=[(EVENTS[i % len(EVENTS)], None)],
                         relations=[(a, b)], note=kind))

    # ---------- 时间线：同一人跨时间多事件（近一月 vs 更早） ----------
    timeline = [
        ("沈亦舟", "上周三沈亦舟和我一起过了季度复盘，他负责数据部分。", "季度复盘", -10),
        ("沈亦舟", "昨天沈亦舟说预算答辩材料还差一页图表。", "预算答辩", -1),
        ("沈亦舟", "今天上午沈亦舟把接口文档发我了，让我下午给反馈。", "接口文档", 0),
        ("沈亦舟", "三个月前沈亦舟刚入职时，是我带他熟悉系统的。", "入职", -90),
        ("沈亦舟", "去年十一月沈亦舟还在杭州分部，年底才调过来。", "杭州分部", -330),
        ("程澜", "本周五程澜要主持需求评审，让我提前把材料发群里。", "需求评审", 2),
        ("程澜", "两周前程澜提过一次流程优化，后来没下文了。", "流程优化", -14),
        ("程澜", "去年十一月程澜拿过一次季度之星。", "季度之星", -330),
    ]
    for i, (name, text, kw, offset) in enumerate(timeline):
        out.append(_case(f"T{i+1:03d}", "timeline", text,
                         people=[name], events=[(kw, offset)]))

    # ---------- 同名：两个刘洋 / 两个王强（跨对话用身份线索区分） ----------
    dup = [
        ("刘洋", "我们组有两个刘洋，前端刘洋负责页面改版，后端刘洋负责接口联调。", "前端"),
        ("刘洋", "刘洋是前端工程师，今天在评审页面方案。", "前端"),
        ("刘洋", "另一个刘洋是后端工程师，接口这块归他。", "后端"),
        ("王强", "王强在测试组，负责回归用例。", "测试"),
        ("王强", "王强是运维，负责发布窗口。", "运维"),
        ("刘洋", "后端刘洋说联调环境又挂了。", "后端"),
        ("王强", "运维王强把发布推迟到周五。", "运维"),
        ("刘洋", "前端刘洋把设计稿的间距调了一遍。", "前端"),
    ]
    for i, (name, text, tag) in enumerate(dup):
        out.append(_case(f"D{i+1:03d}", "dupname", text,
                         people=[name], note=tag))

    # ---------- 虚拟角色（应走独立表，不进真人联系人） ----------
    virtual = [
        ("孙悟空", "孙悟空在取经路上又打了一个妖怪，唐僧念了紧箍咒。"),
        ("林黛玉", "林黛玉葬花那天，贾宝玉一直在旁边劝。"),
        ("哆啦A梦", "哆啦A梦从口袋里掏出了任意门，大雄高兴坏了。"),
        ("福尔摩斯", "福尔摩斯看了一眼鞋底的泥，就说出了对方去过哪里。"),
    ]
    for i, (name, text) in enumerate(virtual):
        out.append(_case(f"V{i+1:03d}", "virtual", text, people=[name]))

    # ---------- 边界（不应导致崩溃或脏数据） ----------
    edge = [
        ("空对话", ""),
        ("纯标点", "。。。！！！？？？"),
        ("纯数字", "1234567890 " * 60),
        ("重复字符", "哈" * 300),
        ("超长无标点", "今天开会讨论了很多事情" + "包括架构调整和人员安排" * 60),
        ("中英混排", "OK 那我们 next step 就是 follow up 这个 issue，deadline 下周三。"),
        ("表情符号", "今天和张总开会 😀 讨论了预算 🚀 下周上线 👀"),
        ("数学公式", "μ = (1/n)∑xᵢ，σ = √(∑(xᵢ-μ)²/n)"),
    ]
    for i, (tag, text) in enumerate(edge):
        out.append(_case(f"E{i+1:03d}", "edge", text, note=tag))

    # ---------- 长文（500–1200 字） ----------
    long1 = (
        "今天下午跟沈亦舟、程澜、傅思远开了个长会，主要三件事。"
        "第一，季度复盘的时间定了，下周三下午两点，沈亦舟负责数据部分，"
        "程澜负责流程回顾，傅思远在海外连线参加。"
        "第二，预算答辩材料还差一页图表，沈亦舟说他明天上午补，补完发我审。"
        "第三，客户回访的安排，傅思远提到客户那边的负责人钟毓对交付节奏有意见，"
        "希望我们提前一周给计划。会上还聊到灰度发布的窗口，"
        "程澜建议避开月底的版本冻结，改到本周五。"
        "最后定了三件事：沈亦舟补图表、程澜改发布窗口、傅思远约钟毓下周沟通。"
    )
    long2 = (
        "记录一下这周的情况。周一上午和邵北对了安全渗透测试的范围，"
        "他担心测试会影响生产，要求先在预发环境跑一轮。"
        "周二黎晚把招聘终面的候选人名单发过来了，一共四个人，"
        "其中两个是她之前带过的，她说背景都查过了没问题。"
        "周三霍云开从客户现场回来，说合同评审卡在付款条款上，"
        "客户那边的负责人薛宁要求改成季度付，我们需要法务确认。"
        "周四房晓提了数据迁移的方案，分三批走，第一批先迁历史订单，"
        "他说月底前能完成第一批。周五上午开周会，秦朗汇报了服务器扩容的进度，"
        "采购单已经提交，预计三天后到货。"
    )
    out.append(_case("L001", "long", long1,
                     people=["沈亦舟", "程澜", "傅思远", "钟毓"],
                     events=[("季度复盘", 5), ("预算答辩", -1), ("客户回访", None)],
                     todos=["补图表", "改发布窗口", "约钟毓"],
                     relations=[("沈亦舟", "程澜")]))
    out.append(_case("L002", "long", long2,
                     people=["邵北", "黎晚", "霍云开", "薛宁", "房晓", "秦朗"],
                     events=[("安全渗透测试", None), ("招聘终面", None), ("数据迁移", -30),
                             ("服务器扩容", 3)],
                     todos=["确认付款条款", "提交采购单"],
                     relations=[("霍云开", "薛宁")]))

    return out


def generated():
    """模板生成：单人事实、事件、待办、多人、关系、别名。"""
    out = []
    n = 0

    # 单人事实 30 条（用于画像素材：同一人出现多次，信息递进）
    for i in range(30):
        name = PEOPLE[i % len(PEOPLE)]
        fact = FACTS[i % len(FACTS)]
        text = f"{name}这个人{fact}。"
        if i % 3 == 0:
            text += f" 这次{EVENTS[i % len(EVENTS)]}也是他牵头。"
        n += 1
        out.append(_case(f"G{n:03d}", "fact", text, people=[name],
                         events=[(EVENTS[i % len(EVENTS)], None)] if i % 3 == 0 else []))

    # 事件 30 条
    for i in range(30):
        name = PEOPLE[(i * 3 + 1) % len(PEOPLE)]
        ev = EVENTS[(i * 7) % len(EVENTS)]
        tm = TIMES[i % len(TIMES)]
        text = f"{tm}{name}要参加{ev}，让我提前把材料准备好。"
        n += 1
        out.append(_case(f"G{n:03d}", "event", text, people=[name],
                         events=[(ev, None)]))

    # 待办 25 条
    for i in range(25):
        name = PEOPLE[(i * 5 + 2) % len(PEOPLE)]
        todo = TODOS[i % len(TODOS)]
        tm = TIMES[(i * 3) % len(TIMES)]
        text = f"{name}让我{tm}之前{todo}，别忘了。"
        n += 1
        out.append(_case(f"G{n:03d}", "todo", text, people=[name],
                         todos=[todo], events=[]))

    # 多人 30 条（3 人同场，测试参与者归属）
    # 分两种句式，避免模运算周期（20）导致的文本重复
    for i in range(30):
        a = PEOPLE[(i * 2) % len(PEOPLE)]
        b = PEOPLE[(i * 2 + 7) % len(PEOPLE)]
        c = PEOPLE[(i * 2 + 13) % len(PEOPLE)]
        ev = EVENTS[(i * 3) % len(EVENTS)]
        if i < 15:
            text = (f"{a}、{b}和{c}一起参加了{ev}，{a}负责记录，"
                    f"{b}负责数据，{c}负责对外沟通。")
        else:
            text = (f"{ev}上，{a}先讲了整体思路，{b}补充了数据口径，"
                    f"最后由{c}整理会后纪要。")
        n += 1
        out.append(_case(f"G{n:03d}", "multi", text, people=[a, b, c],
                         events=[(ev, None)]))

    # 关系 20 条（补充，覆盖不同关系类型）
    for i in range(20):
        a = PEOPLE[(i * 4) % len(PEOPLE)]
        b = PEOPLE[(i * 4 + 11) % len(PEOPLE)]
        kind, tpl = REL_TEMPLATES[i % len(REL_TEMPLATES)]
        text = tpl(a, b)
        n += 1
        out.append(_case(f"G{n:03d}", "relation", text, people=[a, b],
                         relations=[(a, b)], note=kind))

    # 别名 15 条（补充）
    for i in range(15):
        name = PEOPLE[(i * 6 + 3) % len(PEOPLE)]
        alias = f"老{name[-1]}" if i % 2 == 0 else f"小{name[0]}"
        text = f"{name}，同事都叫他{alias}，今天{EVENTS[i % len(EVENTS)]}上他说了两句。"
        n += 1
        out.append(_case(f"G{n:03d}", "alias", text, people=[name],
                         aliases=[(name, alias)],
                         events=[(EVENTS[i % len(EVENTS)], None)]))

    return out


def build_cases():
    return handwritten() + generated()


if __name__ == "__main__":
    import collections
    cases = build_cases()
    print(f"总条数: {len(cases)}")
    for k, v in collections.Counter(c["cat"] for c in cases).most_common():
        print(f"  {k:10s} {v}")
    print(f"期望人名槽位: {sum(len(c['people']) for c in cases)}")
    print(f"别名对: {sum(len(c['aliases']) for c in cases)}")
    print(f"关系对: {sum(len(c['relations']) for c in cases)}")
    print(f"事件槽位: {sum(len(c['events']) for c in cases)}")
    print(f"待办槽位: {sum(len(c['todos']) for c in cases)}")
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "用例 ID 重复"
    texts = [c["text"] for c in cases if c["text"]]
    assert len(texts) == len(set(texts)), "对话文本重复"
    print("ID / 文本唯一性: OK")
