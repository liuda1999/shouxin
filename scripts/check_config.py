#!/usr/bin/env python3
"""配置文件与代码结构一致性检查

做三件事：
  1. TOML 语法校验；
  2. **死配置检查**：配置文件里出现、但代码结构体里没有的键（写了不生效）；
  3. **漏配置提示**：代码里有、配置文件没写的键（会用默认值，列出来供确认）。

用法：
    python scripts/check_config.py                 # 检查 config.toml
    python scripts/check_config.py config.3b.toml  # 检查指定文件
    python scripts/check_config.py config.toml config.3b.toml

退出码：有死配置或语法错误时非 0。
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

try:
    import tomllib  # Python >= 3.11
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None

ROOT = Path(__file__).resolve().parent.parent
SETTINGS_RS = ROOT / "src" / "config" / "settings.rs"

# 配置节 -> 对应的 Rust 结构体
SECTION_TO_STRUCT = {
    "llm": "LlmSettings",
    "llm.gguf": "GgufBackendSettings",
    "llm.ollama": "OllamaBackendSettings",
    "llm.api": "ApiBackendSettings",
    "llm.heartbeat": "HeartbeatLlmSettings",
    "embedder": "EmbedderSettings",
    "heartbeat": "HeartbeatSettings",
    "storage": "StorageSettings",
    "logging": "LoggingSettings",
    "agent": "AgentSettings",
    "server": "ServerSettings",
    "event": "EventSettings",
    "extraction": "ExtractionSettings",
    "todo": "TodoSettings",
}


def parse_structs(rs_text: str) -> dict[str, list[str]]:
    """从 settings.rs 提取 {结构体名: [字段名, ...]}"""
    out: dict[str, list[str]] = {}
    for m in re.finditer(r"pub struct (\w+)\s*\{(.*?)\n\}", rs_text, re.S):
        name, body = m.group(1), m.group(2)
        fields = re.findall(r"^\s*pub (\w+)\s*:", body, re.M)
        if fields:
            out[name] = fields
    return out


def flatten(d: dict, prefix: str = "") -> dict[str, object]:
    """把嵌套 dict 展平成 {'a.b': value}"""
    flat: dict[str, object] = {}
    for k, v in d.items():
        key = f"{prefix}.{k}" if prefix else k
        if isinstance(v, dict):
            # 既是「节」也可能是「表数组」；这里只处理表
            flat[key] = v
            flat.update(flatten(v, key))
        else:
            flat[key] = v
    return flat


def check(path: Path, structs: dict[str, list[str]]) -> int:
    print(f"\n== 检查 {path.name} ==")
    if tomllib is None:
        print("  [SKIP] 需要 Python >= 3.11（tomllib）")
        return 0
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except Exception as e:  # noqa: BLE001
        print(f"  [FAIL] TOML 语法错误: {e}")
        return 1

    flat = flatten(data)
    problems = 0

    # 1) 死配置：叶子键的父节必须存在，且键名在结构体里
    for key, value in flat.items():
        if isinstance(value, dict):
            # 节本身：必须在映射表里，或者是某个已映射节的前缀
            if key not in SECTION_TO_STRUCT and not any(
                s.startswith(key + ".") for s in SECTION_TO_STRUCT
            ):
                print(f"  [WARN] 未知配置节 [{key}]（代码里没有对应结构体）")
                problems += 0
            continue

        parent = key.rsplit(".", 1)[0] if "." in key else None
        leaf = key.rsplit(".", 1)[-1]
        if parent is None:
            print(f"  [WARN] 顶层键 '{leaf}' 未归属任何结构体")
            continue
        struct = SECTION_TO_STRUCT.get(parent)
        if struct is None:
            continue
        if struct not in structs:
            print(f"  [WARN] 结构体 {struct} 未在 settings.rs 中找到")
            continue
        if leaf not in structs[struct]:
            print(f"  [FAIL] 死配置: [{parent}] {leaf} = {value!r} —— 结构体 {struct} 里没有该字段，写了不生效")
            problems += 1

    # 2) 漏配置：结构体字段没写（用默认值）
    for section, struct in SECTION_TO_STRUCT.items():
        if struct not in structs:
            continue
        section_data = data
        ok = True
        for part in section.split("."):
            if isinstance(section_data, dict) and part in section_data:
                section_data = section_data[part]
            else:
                ok = False
                break
        if not ok:
            print(f"  [INFO] 缺少配置节 [{section}]，将全部使用默认值")
            continue
        missing = [f for f in structs[struct] if f not in section_data]
        nested = [f for f in missing if f in SECTION_TO_STRUCT_VALUES]
        plain = [f for f in missing if f not in nested]
        if plain:
            print(f"  [INFO] [{section}] 未显式配置（用默认值）: {', '.join(plain)}")

    if problems == 0:
        print("  [OK] 无死配置，语法正常")
    else:
        print(f"  [FAIL] 发现 {problems} 处死配置")
    return 1 if problems else 0


# 子节字段（llm.gguf / llm.ollama / ... ）不按"漏配置"提示
SECTION_TO_STRUCT_VALUES = {s.split(".")[-1] for s in SECTION_TO_STRUCT if "." in s}


def main(argv: list[str]) -> int:
    files = [Path(a) for a in argv[1:]] or [ROOT / "config.toml"]
    structs = parse_structs(SETTINGS_RS.read_text(encoding="utf-8"))
    rc = 0
    for f in files:
        if not f.exists():
            print(f"[FAIL] 文件不存在: {f}")
            rc = 1
            continue
        rc |= check(f, structs)
    return rc


if __name__ == "__main__":
    sys.exit(main(sys.argv))
