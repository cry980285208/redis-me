#!/usr/bin/env python3
"""向 Redis 8.8+ 写入若干 Array 样例键，供 RedisME Array 浏览/读写验收。

需 Python 3.8+，无第三方依赖（纯 socket + RESP）。连接参数优先级：命令行 > 环境变量 >
默认本机。环境变量：REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD。

用法::

    python ArraySeed.py
    python ArraySeed.py 127.0.0.1 6379 hepengju
    python ArraySeed.py 127.0.0.1 6379 ""          # 无密码

写入的键前缀均为 test:array:。覆盖场景见各 seed_* 函数注释（稀疏 / 分页 /
索引范围 / LastN / 二进制 / 高索引 / TTL 等）。
"""

from __future__ import annotations

import os
import socket
import sys

PREFIX = "test:array:"


class RedisCli:
    """最小 RESP 客户端：AUTH / DEL / EXPIRE / AR*。"""

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

    def arset(self, key: str, index: int, *values: bytes) -> None:
        """ARSET key index value [value ...]（连续槽）。"""
        args = [key.encode("utf-8"), str(index).encode("ascii"), *values]
        self._write_command(b"ARSET", *args)
        self._read_integer()

    def armset(self, key: str, pairs: list[tuple[int, bytes]]) -> None:
        """ARMSET key index value [index value ...]（可稀疏、可乱序）。"""
        args: list[bytes] = [key.encode("utf-8")]
        for idx, val in pairs:
            args.append(str(idx).encode("ascii"))
            args.append(val)
        self._write_command(b"ARMSET", *args)
        self._read_integer()

    def arinsert(self, key: str, *values: bytes) -> None:
        """ARINSERT key value [value ...]（顺序追加，供 LastN）。"""
        args = [key.encode("utf-8"), *values]
        self._write_command(b"ARINSERT", *args)
        self._read_integer()

    def ardel(self, key: str, *indexes: int) -> None:
        args = [key.encode("utf-8"), *[str(i).encode("ascii") for i in indexes]]
        self._write_command(b"ARDEL", *args)
        self._read_integer()

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

    def _read_integer(self) -> None:
        line = self._readline()
        if line.startswith(b":"):
            return
        if line.startswith(b"-"):
            raise OSError(f"Redis error: {line[1:].decode('utf-8', errors='replace')}")
        raise OSError(f"unexpected Redis reply: {line!r}")

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


def _b(s: str) -> bytes:
    return s.encode("utf-8")


def seed_all(redis: RedisCli) -> list[str]:
    """写入全部样例，返回键名列表。"""
    keys: list[str] = []

    def take(name: str) -> str:
        key = PREFIX + name
        redis.delete(key)
        keys.append(key)
        return key

    # --- 基础读写 ---
    k = take("tiny")
    redis.arset(k, 0, _b("a"), _b("b"), _b("c"))
    print(f"ARSET {k} 0 a b c  → 稠密小数组，测基础表格")

    k = take("single")
    redis.arset(k, 0, _b("only-one"))
    print(f"ARSET {k} 0 only-one  → 单元素")

    k = take("empty-ready")
    # 不写任何槽：先 ARMSET 再全删，留下空 Array（TYPE 仍为 array）
    redis.armset(k, [(0, _b("tmp"))])
    redis.ardel(k, 0)
    print(f"ARMSET+ARDEL {k}  → 空 Array（ARCOUNT=0）")

    # --- ARCOUNT vs ARLEN（稀疏）---
    k = take("sparse-basic")
    redis.armset(
        k,
        [
            (0, _b("idx-0")),
            (10, _b("idx-10")),
            (100, _b("idx-100")),
        ],
    )
    print(f"ARMSET {k} 0/10/100  → ARCOUNT=3，ARLEN=101，测底部长度展示")

    k = take("sparse-holes")
    redis.armset(k, [(i, _b(f"v{i}")) for i in range(0, 20, 2)])
    redis.ardel(k, 4, 8, 12)
    print(f"ARMSET+ARDEL {k}  → 偶索引有值再删部分槽，留空洞")

    # --- 索引范围过滤 ---
    k = take("range-clusters")
    pairs = []
    for i in range(5):
        pairs.append((i, _b(f"low-{i}")))  # 0–4
    for i in range(50, 55):
        pairs.append((i, _b(f"mid-{i}")))  # 50–54
    for i in range(200, 205):
        pairs.append((i, _b(f"high-{i}")))  # 200–204
    redis.armset(k, pairs)
    print(f"ARMSET {k} 三簇 0–4 / 50–54 / 200–204  → 测工具栏索引范围")

    # --- ARSCAN 分页（填充槽较多）---
    k = take("page-dense")
    redis.arset(k, 0, *[_b(f"page-{i:03d}") for i in range(80)])
    print(f"ARSET {k} 0..79  → 80 个稠密槽，测 fieldScan 续页")

    k = take("page-sparse")
    # 每隔 7 填一个，约 60 个槽，跨度到 ~420
    redis.armset(k, [(i * 7, _b(f"sp-{i:03d}")) for i in range(60)])
    print(f"ARMSET {k} 0,7,14,...  ×60  → 稀疏分页 + 范围扫")

    # --- ARLASTITEMS / ARINSERT ---
    k = take("lastn")
    # 分批插入，顺序可对照 LastN 插入序 / 最近优先
    for word in ("first", "second", "third", "fourth", "fifth", "第六", "seventh", "eighth"):
        redis.arinsert(k, _b(word))
    print(f"ARINSERT {k} ×8  → 测 LastN（插入序 / REV）")

    k = take("lastn-many")
    redis.arinsert(k, *[_b(f"evt-{i:03d}") for i in range(30)])
    print(f"ARINSERT {k} ×30  → LastN 改数量")

    # --- 值形态 ---
    k = take("unicode")
    redis.armset(
        k,
        [
            (0, _b("你好")),
            (1, _b("Array 类型")),
            (2, _b("emoji 🚀")),
            (5, _b("中文空洞后")),
        ],
    )
    print(f"ARMSET {k}  → UTF-8 / emoji")

    k = take("binary")
    redis.armset(
        k,
        [
            (0, b"abc\x00\xff"),
            (1, bytes(range(0, 32))),
            (2, b"\x80\x81\xfe\xff"),
        ],
    )
    print(f"ARMSET {k}  → 含 \\0 / 非法 UTF-8 字节，测值编码")

    k = take("json-str")
    redis.arset(
        k,
        0,
        _b('{"id":1,"name":"alice"}'),
        _b("[1,2,3]"),
        _b("plain-not-json"),
    )
    print(f"ARSET {k}  → JSON 字符串当普通值")

    # --- 高索引（仍在 JS Number 安全整数内，避免误伤）---
    k = take("high-index")
    redis.armset(
        k,
        [
            (0, _b("zero")),
            (1_000_000, _b("million")),
            (9_000_000_000_000_000, _b("near-safe-int")),  # 9e15 < 2^53-1
        ],
    )
    print(f"ARMSET {k} 0 / 1e6 / 9e15  → 高索引展示与精确查")

    # --- 乱序 ARMSET ---
    k = take("armset-shuffle")
    redis.armset(
        k,
        [
            (99, _b("last-written-first")),
            (1, _b("one")),
            (50, _b("fifty")),
            (2, _b("two")),
        ],
    )
    print(f"ARMSET {k} 乱序索引  → 复制命令 ARMSET 对照")

    # --- 带 TTL ---
    k = take("ttl-1h")
    redis.arset(k, 0, _b("expires-in-1h"))
    redis.expire(k, 3600)
    print(f"ARSET+EXPIRE {k} 3600  → 测 TTL 展示")

    # --- 单槽大值（预览/截断相关时可用）---
    k = take("big-value")
    redis.arset(k, 0, (_b("x") * 5000) + _b("-tail"))
    redis.arset(k, 1, _b("small"))
    print(f"ARSET {k}  → 5KB 大值 + 小值")

    # --- 混合写入路径：ARSET 连续 + ARINSERT ---
    k = take("mixed-write")
    redis.arset(k, 10, _b("at-10"), _b("at-11"), _b("at-12"))
    redis.arinsert(k, _b("insert-a"), _b("insert-b"))
    print(f"ARSET@10 + ARINSERT {k}  → 混写后 ARSCAN / LastN")

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
        try:
            keys = seed_all(redis)
        except OSError as e:
            msg = str(e)
            if "unknown command" in msg.lower() or "ARSET" in msg or "array" in msg.lower():
                print(
                    "失败：当前 Redis 似乎不支持 Array（需 ≥ 8.8）。\n"
                    f"  原始错误: {msg}",
                    file=sys.stderr,
                )
                return 1
            raise

    print(f"done. {len(keys)}× {PREFIX}*")
    print("建议验收：")
    print("  tiny / page-dense     → 基础表格与分页")
    print("  sparse-basic          → ARCOUNT vs ARLEN")
    print("  range-clusters        → 索引范围 0-4 / 50-54 / 200-204")
    print("  lastn / lastn-many    → LastN 弹框")
    print("  binary / unicode      → 值编码")
    print("  high-index            → 精确查索引 / 高位展示")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
