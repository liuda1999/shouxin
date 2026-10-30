"""以使用者身份跑 12 轮真实对话（agent_live），并记录每条 conversation_id。"""
import json
import time
import urllib.request

BASE = "http://127.0.0.1:3000"
USER = "agent_live"

ROUNDS = [
    ("今天上午和张伟、李娜开了 Q4 目标会，定了三个方向：留存、企业版、成本优化。", None),
    ("下午产品评审把登录改版又往后推了两周，设计稿下周给。", None),
    ("我妈上周体检结果出来了，血脂偏高，医生让三个月后复查。", None),
    ("晚上把上月差旅报销单提交了，财务说七个工作日内到账。", None),
    ("王强下周搬家，让我周六上午过去帮忙搬书。", None),
    ("赵磊提了离职，交接期一个月，他手上有三个服务要转给我。", None),
    ("这周读完了《深度工作》，做了两页笔记，准备下周整理成文。", None),
    ("跑步时右膝有点疼，可能是上周加量太快，先停三天看看。", None),
    ("下周三之前要把 Q4 预算表交给陈静，她说董事会要用。", None),
    ("十一去成都的机票订好了，10 月 3 号早上八点的航班。", None),
    ("我爸下月 15 号做心脏支架手术，我准备请一周假回去陪护。", None),
]

MULTI_TURN = [
    {"role": "user", "content": "孙婷，合同什么时候能给我？"},
    {"role": "other", "content": "我周五前发给你，条款按上次谈的。"},
]


def post(path, payload):
    req = urllib.request.Request(
        BASE + path, data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"}, method="POST")
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode())


def main():
    ids = []
    for i, (text, _) in enumerate(ROUNDS, 1):
        r = post("/api/memory/add", {"conversation": text, "user_id": USER})
        cid = (r.get("data") or {}).get("conversation_id")
        ids.append({"round": i, "id": cid, "text": text[:30]})
        print(f"[{i:02d}] {cid}  {text[:24]}")
        time.sleep(0.4)

    r = post("/api/memory/add", {"user_id": USER, "messages": MULTI_TURN})
    cid = (r.get("data") or {}).get("conversation_id")
    ids.append({"round": 12, "id": cid, "text": "[messages] 孙婷合同"})
    print(f"[12] {cid}  [messages] 孙婷合同")

    with open(".agent_convs.json", "w", encoding="utf-8") as f:
        json.dump(ids, f, ensure_ascii=False, indent=2)
    print("saved .agent_convs.json")


if __name__ == "__main__":
    main()
