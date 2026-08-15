#!/usr/bin/env python3
"""向 Redis ≥ 8.4 写入若干 Vector Set 样例键，供 RedisME Vector Set 验收。

需 Python 3.8+，无第三方依赖（纯 socket + RESP）。连接参数优先级：命令行 > 环境变量 >
默认本机。环境变量：REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD。

用法::

    python VectorSetSeed.py
    python VectorSetSeed.py 127.0.0.1 6379 hepengju
    python VectorSetSeed.py 127.0.0.1 6379 ""          # 无密码

写入的键前缀均为 test:vset:。覆盖：基础 CRUD、VRANGE 分页、VSIM、FILTER attrs、
高维预览、量化差异、Unicode 元素名等。零向量会归一化失败，种子刻意避开。
"""

from __future__ import annotations

import json
import math
import os
import socket
import sys

PREFIX = "test:vset:"


class RedisCli:
    """最小 RESP 客户端：AUTH / DEL / EXPIRE / V*。"""

    def __init__(self, host: str, port: int) -> None:
        self._sock = socket.create_connection((host, port))
        self._sock.settimeout(30)

    def close(self) -> None:
        self._sock.close()

    def __enter__(self) -> RedisCli:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def auth(self, password: str) -> None:
        self._write_command(b"AUTH", password.encode("utf-8"))
        self._read_ok()

    def delete(self, key: str) -> None:
        self._write_command(b"DEL", key.encode("utf-8"))
        self._read_integer()

    def expire(self, key: str, seconds: int) -> None:
        self._write_command(b"EXPIRE", key.encode("utf-8"), str(seconds).encode("ascii"))
        self._read_integer()

    def type(self, key: str) -> str:
        self._write_command(b"TYPE", key.encode("utf-8"))
        return self._read_simple_string()

    def vadd(
        self,
        key: str,
        vector: list[float],
        element: str | bytes,
        *,
        setattr_json: str | None = None,
        quant: str | None = None,
    ) -> None:
        """VADD key VALUES dim f… element [NOQUANT|Q8|BIN] [SETATTR json].

        量化选项必须在 element 之后（官方语法），不能放在 VALUES 前。
        element 可为 str（UTF-8）或 raw bytes（测非 UTF-8 成员）。
        """
        if not vector:
            raise ValueError("vector must be non-empty")
        if all(abs(x) < 1e-15 for x in vector):
            raise ValueError("zero vector cannot be normalized by Redis VADD")

        elem = element if isinstance(element, bytes) else element.encode("utf-8")
        args: list[bytes] = [key.encode("utf-8"), b"VALUES", str(len(vector)).encode("ascii")]
        for f in vector:
            args.append(_float_arg(f))
        args.append(elem)
        if quant:
            q = quant.upper()
            if q not in ("NOQUANT", "Q8", "BIN"):
                raise ValueError(f"bad quant: {quant}")
            args.append(q.encode("ascii"))
        if setattr_json is not None:
            args.append(b"SETATTR")
            args.append(setattr_json.encode("utf-8"))
        self._write_command(b"VADD", *args)
        # VADD：新元素→1；已存在元素 upsert 向量→0。两者都算成功。
        self._read_integer()

    def vsetattr(self, key: str, element: str | bytes, attrs_json: str) -> None:
        elem = element if isinstance(element, bytes) else element.encode("utf-8")
        self._write_command(
            b"VSETATTR",
            key.encode("utf-8"),
            elem,
            attrs_json.encode("utf-8"),
        )
        self._read_integer()

    def vrange_ping(self, key: str) -> None:
        """用空键探测 VRANGE 是否存在（需 ≥ 8.4）。"""
        self._write_command(b"VRANGE", key.encode("utf-8"), b"-", b"+", b"1")
        # 不存在键 → 空数组；未知命令 → 错误
        self._read_array_or_error()

    def _write_command(self, cmd: bytes, *args: bytes) -> None:
        parts = [cmd, *args]
        buf = bytearray()
        buf.extend(f"*{len(parts)}\r\n".encode("ascii"))
        for p in parts:
            buf.extend(f"${len(p)}\r\n".encode("ascii"))
            buf.extend(p)
            buf.extend(b"\r\n")
        self._sock.sendall(buf)

    def _read_ok(self) -> None:
        line = self._readline()
        if line.startswith(b"+"):
            return
        if line.startswith(b"-"):
            raise OSError(f"Redis error: {line[1:].decode('utf-8', errors='replace')}")
        raise OSError(f"unexpected Redis reply: {line!r}")

    def _read_simple_string(self) -> str:
        line = self._readline()
        if line.startswith(b"+"):
            return line[1:].decode("utf-8", errors="replace")
        if line.startswith(b"-"):
            raise OSError(f"Redis error: {line[1:].decode('utf-8', errors='replace')}")
        raise OSError(f"unexpected Redis reply: {line!r}")

    def _read_integer(self) -> int:
        line = self._readline()
        if line.startswith(b":"):
            return int(line[1:])
        if line.startswith(b"-"):
            raise OSError(f"Redis error: {line[1:].decode('utf-8', errors='replace')}")
        # RESP3 bool
        if line in (b"#t", b"#f"):
            return 1 if line == b"#t" else 0
        raise OSError(f"unexpected Redis reply: {line!r}")

    def _read_array_or_error(self) -> None:
        line = self._readline()
        if line.startswith(b"-"):
            raise OSError(f"Redis error: {line[1:].decode('utf-8', errors='replace')}")
        if line.startswith(b"*"):
            n = int(line[1:])
            for _ in range(max(n, 0)):
                self._discard_one()
            return
        if line == b"*-1" or line == b"_":
            return
        raise OSError(f"unexpected Redis reply: {line!r}")

    def _discard_one(self) -> None:
        line = self._readline()
        if line.startswith(b"$"):
            n = int(line[1:])
            if n >= 0:
                need = n + 2
                while need > 0:
                    chunk = self._sock.recv(need)
                    if not chunk:
                        raise OSError("Redis connection closed")
                    need -= len(chunk)
            return
        if line.startswith((b"+", b"-", b":", b"#", b",", b"_")):
            return
        if line.startswith(b"*"):
            n = int(line[1:])
            for _ in range(max(n, 0)):
                self._discard_one()
            return
        raise OSError(f"unexpected Redis reply while discarding: {line!r}")

    def _readline(self) -> bytes:
        buf = bytearray()
        while True:
            ch = self._sock.recv(1)
            if not ch:
                raise OSError("Redis connection closed")
            if ch == b"\n":
                if buf.endswith(b"\r"):
                    return bytes(buf[:-1])
                return bytes(buf)
            buf.extend(ch)


def _float_arg(f: float) -> bytes:
    # 避免科学计数法歧义；与 redis-cli 常见写法一致
    if isinstance(f, bool):
        raise TypeError("bool is not a float component")
    s = format(float(f), ".17g")
    return s.encode("ascii")


def _unit(angle_deg: float, dim: int = 3) -> list[float]:
    """二维单位圆投影到 dim（其余维填 0.01 避免退化）。"""
    rad = math.radians(angle_deg)
    v = [math.cos(rad), math.sin(rad)]
    while len(v) < dim:
        v.append(0.01 * (len(v) + 1))
    return v


def _highdim(seed: int, dim: int = 64) -> list[float]:
    """确定性伪随机向量，便于重复造数。"""
    out: list[float] = []
    x = float(seed * 17 + 3)
    for i in range(dim):
        x = (x * 1.7 + i * 0.13) % 2.0 - 1.0
        if abs(x) < 1e-6:
            x = 0.05
        out.append(x)
    return out


def seed_all(redis: RedisCli) -> list[str]:
    """写入全部样例，返回键名列表。"""
    keys: list[str] = []

    def take(name: str) -> str:
        key = PREFIX + name
        redis.delete(key)
        keys.append(key)
        return key

    # --- 官方 tutorial 风格：2D 点，测 VSIM ELE / VALUES ---
    k = take("points")
    # 与官方 points 示例同构，便于对照文档手测相似度排序
    redis.vadd(k, [1.0, 1.0], "pt:A")
    redis.vadd(k, [-1.0, -1.0], "pt:B")
    redis.vadd(k, [-1.0, 1.0], "pt:C")
    redis.vadd(k, [1.0, -1.0], "pt:D")
    redis.vadd(k, [1.0, 0.0], "pt:E")
    print(f"VADD {k} ×5 (dim=2)  → TYPE=vectorset；VSIM ELE pt:A / VALUES")

    # --- 带 attrs，测 FILTER ---
    k = take("movies")
    movies = [
        ("blade-runner", [0.9, 0.1, 0.2], {"year": 1982, "genre": "scifi", "rating": 8.1}),
        ("matrix", [0.85, 0.15, 0.25], {"year": 1999, "genre": "scifi", "rating": 8.7}),
        ("inception", [0.7, 0.4, 0.3], {"year": 2010, "genre": "scifi", "rating": 8.8}),
        ("godfather", [0.1, 0.8, 0.2], {"year": 1972, "genre": "crime", "rating": 9.2}),
        ("spirited", [0.2, 0.3, 0.9], {"year": 2001, "genre": "anime", "rating": 8.6}),
        ("dune", [0.75, 0.2, 0.35], {"year": 2021, "genre": "scifi", "rating": 8.0}),
        ("no-attrs", [0.5, 0.5, 0.5], None),
    ]
    for name, vec, attrs in movies:
        redis.vadd(
            k,
            vec,
            name,
            setattr_json=None if attrs is None else json.dumps(attrs, ensure_ascii=False),
        )
    print(f"VADD {k} ×{len(movies)} (dim=3 + attrs)  → VSIM FILTER '.year >= 2000'")

    # --- VRANGE 分页：字典序 elem:000.. ---
    k = take("page")
    for i in range(80):
        # 角度铺开，避免两两重合；dim=3
        redis.vadd(k, _unit(i * 4.5, dim=3), f"elem:{i:03d}")
    print(f"VADD {k} ×80 (elem:000..)  → VRANGE 续页；游标 start=(last")

    # --- 高维：表格截断预览 ---
    k = take("highdim")
    for i, name in enumerate(("alpha", "beta", "gamma", "delta")):
        redis.vadd(k, _highdim(100 + i, dim=64), name)
    print(f"VADD {k} ×4 (dim=64)  → 向量列截断预览")

    # --- Unicode 元素名 ---
    k = take("unicode")
    redis.vadd(k, [1.0, 0.0, 0.0], "苹果")
    redis.vadd(k, [0.95, 0.1, 0.05], "香蕉")
    redis.vadd(k, [0.2, 0.9, 0.1], "火箭🚀")
    redis.vadd(k, [0.1, 0.2, 0.95], "向量集")
    print(f"VADD {k}  → UTF-8 / emoji 元素名 + VSIM ELE 苹果")

    # --- 非 UTF-8 元素名（wire base64 / 精确查 / VRANGE 游标）---
    k = take("binary-elem")
    redis.vadd(k, [1.0, 0.0, 0.0], b"bin\x00\xff")
    redis.vadd(k, [0.0, 1.0, 0.0], b"\x80\x81\xfe\xff")
    redis.vadd(k, [0.0, 0.0, 1.0], "ascii-ok")
    print(f"VADD {k}  → 含 \\0 / 非法 UTF-8 元素名，测 wire 与续页游标")

    # --- 混合 attrs / 事后 VSETATTR / 清空 attrs ---
    k = take("attrs-mixed")
    redis.vadd(k, [1.0, 0.0, 0.0], "with-on-add", setattr_json='{"tier":"gold","n":1}')
    redis.vadd(k, [0.0, 1.0, 0.0], "set-later")
    redis.vsetattr(k, "set-later", '{"tier":"silver","n":2}')
    redis.vadd(k, [0.0, 0.0, 1.0], "cleared", setattr_json='{"tier":"tmp"}')
    redis.vsetattr(k, "cleared", "")  # 空串删除 attrs
    redis.vadd(k, [0.7, 0.7, 0.0], "never-attrs")
    print(f"VADD/VSETATTR {k}  → 有 attrs / 后设 / 清空 / 无 attrs")

    # --- 量化：默认 Q8 vs NOQUANT（分属不同键；量化在首条 VADD 锁定）---
    k = take("quant-q8")
    redis.vadd(k, [1.262185, 1.958231, 0.5], "q8-a", quant="Q8")
    redis.vadd(k, [1.1, 1.8, 0.4], "q8-b", quant="Q8")
    print(f"VADD {k} Q8  → VINFO quant-type≈int8；VEMB 为近似值")

    k = take("quant-noq")
    redis.vadd(k, [1.262185, 1.958231, 0.5], "fp-a", quant="NOQUANT")
    redis.vadd(k, [1.1, 1.8, 0.4], "fp-b", quant="NOQUANT")
    print(f"VADD {k} NOQUANT  → VINFO quant-type≈fp32；对照 VEMB 精度")

    # --- 单元素 / upsert 对照（同名再写）---
    k = take("single")
    redis.vadd(k, [0.3, 0.6, 0.9], "only")
    print(f"VADD {k} only  → 单元素；可在 GUI 再 VADD 同名测 upsert")

    k = take("upsert")
    redis.vadd(k, [1.0, 0.0, 0.0], "same", setattr_json='{"v":1}')
    redis.vadd(k, [0.0, 1.0, 0.0], "same")  # 更新向量；attrs 通常保留
    print(f"VADD {k} same×2  → upsert 向量后 attrs 是否仍在（VGETATTR）")

    # --- 精确查询目标 ---
    k = take("exact")
    redis.vadd(k, [0.11, 0.22, 0.33], "find-me")
    redis.vadd(k, [0.9, 0.1, 0.0], "noise-a")
    redis.vadd(k, [0.1, 0.9, 0.0], "noise-b")
    print(f"VADD {k}  → 精确查元素名 find-me")

    # --- TTL ---
    k = take("ttl-1h")
    redis.vadd(k, [0.4, 0.5, 0.6], "temp")
    redis.expire(k, 3600)
    print(f"VADD+EXPIRE {k} 3600  → TTL 展示")

    # --- 字典序边界：续页 exclusive 游标 ---
    k = take("lex-order")
    for name in ("Redis", "RedisLabs", "apple", "banana", "zebra", "a7", "a70"):
        redis.vadd(k, _unit(hash(name) % 360, dim=3), name)
    print(f"VADD {k}  → 手测 VRANGE [Redis + / (a7 + 续页语义")

    # --- 1 维向量（VALUES 1 必须带关键字，防客户端漏写）---
    k = take("dim1")
    redis.vadd(k, [0.5], "half")
    redis.vadd(k, [0.9], "high")
    redis.vadd(k, [-0.2], "neg")
    print(f"VADD {k} (dim=1)  → VALUES 1 语法；VDIM=1")

    return keys


def redis_conn_from_argv(argv: list[str]) -> tuple[str, int, str]:
    """命令行 > REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD > 本机默认。"""
    host = argv[1] if len(argv) > 1 else os.environ.get("REDIS_SERVER", "127.0.0.1")
    port = int(argv[2] if len(argv) > 2 else os.environ.get("REDIS_PROT", "6379"))
    password = argv[3] if len(argv) > 3 else os.environ.get("REDIS_PASSWORD", "hepengju")
    return host, port, password


def main(argv: list[str]) -> int:
    host, port, password = redis_conn_from_argv(argv)

    with RedisCli(host, port) as redis:
        if password:
            redis.auth(password)
        # 先探测 VRANGE（浏览硬依赖 ≥ 8.4）
        try:
            redis.vrange_ping("__redis_me_vrange_probe__")
        except OSError as e:
            msg = str(e)
            if "unknown command" in msg.lower() or "VRANGE" in msg:
                print(
                    "失败：当前 Redis 似乎不支持 VRANGE（Vector Set 浏览需 ≥ 8.4）。\n"
                    f"  原始错误: {msg}",
                    file=sys.stderr,
                )
                return 1
            raise

        try:
            keys = seed_all(redis)
        except OSError as e:
            msg = str(e)
            low = msg.lower()
            if "unknown command" in low:
                print(
                    "失败：当前 Redis 似乎不支持 Vector Set（需 ≥ 8.0，浏览建议 ≥ 8.4）。\n"
                    f"  原始错误: {msg}",
                    file=sys.stderr,
                )
                return 1
            raise

        # 抽查 TYPE
        sample = keys[0]
        t = redis.type(sample)
        if t != "vectorset":
            print(f"警告：{sample} TYPE={t!r}，期望 vectorset", file=sys.stderr)

    print(f"done. {len(keys)}× {PREFIX}*")
    print("建议验收：")
    print("  points / movies       → VSIM ELE·VALUES；movies 再测 FILTER")
    print("  page / lex-order      → VRANGE 分页与 exclusive 游标")
    print("  highdim               → 64 维截断预览")
    print("  attrs-mixed           → attrs 列 / 清空")
    print("  quant-q8 / quant-noq  → VINFO + VEMB 近似差异")
    print("  exact / unicode       → 精确查询与 UTF-8 元素名")
    print("  binary-elem           → 非 UTF-8 元素名 / wire")
    print("  dim1 / lex-order / ttl-1h → 边界维、VRANGE 语义、TTL")
    print("  single / upsert       → 单元素与 upsert（VADD 返回 0 仍成功）")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
