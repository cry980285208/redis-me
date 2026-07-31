#!/usr/bin/env python3
"""向 Redis 写入 100 个「键名为非法 UTF-8 bytes」的样例，供验证 RedisKey 仍返回 bytes。

需 Python 3.8+，无第三方依赖（纯 socket + RESP）。连接参数优先级：命令行 > 环境变量 >
默认本机。环境变量：REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD。

用法::

    python BinKeySeed.py
    python BinKeySeed.py 127.0.0.1 6379 hepengju
    python BinKeySeed.py 127.0.0.1 6379 ""          # 无密码

写入的键前缀均为 test:binkey:（ASCII 前缀 + 非法 UTF-8 后缀，经 RESP 二进制发出）。
扫描后 RedisKey 应带非空 bytes（key 仅为 lossy 展示）；删改/打开应走 bytes。
"""

from __future__ import annotations

import os
import socket
import sys

PREFIX = b"test:binkey:"
COUNT = 100


def build_binary_keys() -> list[bytes]:
    """100 个非法 UTF-8 键（多样式），全部以 bytes 形式写入 Redis。"""
    keys: list[bytes] = []

    # 0–19：单字节非法（0x80–0xFF 续字节/非法起始）
    for i in range(20):
        keys.append(PREFIX + f"solo:{i:03d}:".encode("ascii") + bytes([0x80 + i]))

    # 20–39：截断的多字节序列（缺续字节）
    for i in range(10):
        # 2 字节起始 0xC2–0xDF，无续字节
        keys.append(PREFIX + f"trunc2:{i:03d}:".encode("ascii") + bytes([0xC2 + i]))
    for i in range(10):
        # 3 字节起始后只跟 1 个续字节
        keys.append(PREFIX + f"trunc3:{i:03d}:".encode("ascii") + bytes([0xE0, 0x80 + i]))

    # 40–59：overlong / 非法续字节
    for i in range(10):
        keys.append(PREFIX + f"overlong:{i:03d}:".encode("ascii") + bytes([0xC0, 0x80 + i]))
    for i in range(10):
        # 续字节位置放了非 10xxxxxx
        keys.append(PREFIX + f"badcont:{i:03d}:".encode("ascii") + bytes([0xE2, 0x28 + i, 0xA1]))

    # 60–79：混入合法 UTF-8 中间夹非法字节
    for i in range(10):
        keys.append(
            PREFIX
            + f"mix-mid:{i:03d}:".encode("ascii")
            + "中".encode("utf-8")
            + bytes([0xFF])
            + "文".encode("utf-8")
        )
    for i in range(10):
        keys.append(
            PREFIX
            + f"mix-end:{i:03d}:".encode("ascii")
            + f"尾-{i}-".encode("utf-8")
            + bytes([0xFE, 0xFF])
        )

    # 80–89：代理区 / 超出 Unicode（UTF-8 非法）
    for i in range(5):
        # U+D800.. 代理对编码形态（非法）
        keys.append(PREFIX + f"surrogate:{i:03d}:".encode("ascii") + bytes([0xED, 0xA0, 0x80 + i]))
    for i in range(5):
        # 4 字节超出 U+10FFFF
        keys.append(PREFIX + f"toolarge:{i:03d}:".encode("ascii") + bytes([0xF4, 0x90, 0x80, 0x80 + i]))

    # 90–99：较长二进制尾巴
    for i in range(10):
        tail = bytes((0x80 + ((i * 7 + j) % 0x80) for j in range(8 + i)))
        keys.append(PREFIX + f"long:{i:03d}:".encode("ascii") + tail)

    assert len(keys) == COUNT, f"expected {COUNT} keys, got {len(keys)}"
    for k in keys:
        try:
            k.decode("utf-8")
            raise AssertionError(f"expected invalid utf-8: {k!r}")
        except UnicodeDecodeError:
            pass
    return keys


class RedisCli:
    """最小 RESP 客户端：AUTH / SET / DEL；键与值均可为 bytes。"""

    def __init__(self, host: str, port: int) -> None:
        self._sock = socket.create_connection((host, port))
        self._sock.settimeout(10)

    def close(self) -> None:
        self._sock.close()

    def __enter__(self) -> RedisCli:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def auth(self, password: str) -> None:
        self._write_command(b"AUTH", password.encode("utf-8"))
        self._read_ok()

    def set(self, key: bytes, value: bytes) -> None:
        self._write_command(b"SET", key, value)
        self._read_ok()

    def delete(self, key: bytes) -> None:
        self._write_command(b"DEL", key)
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


def redis_conn_from_argv(argv: list[str]) -> tuple[str, int, str]:
    """命令行 > REDIS_SERVER / REDIS_PROT / REDIS_PASSWORD > 本机默认。"""
    host = argv[1] if len(argv) > 1 else os.environ.get("REDIS_SERVER", "127.0.0.1")
    port = int(argv[2] if len(argv) > 2 else os.environ.get("REDIS_PROT", "6379"))
    password = argv[3] if len(argv) > 3 else os.environ.get("REDIS_PASSWORD", "hepengju")
    return host, port, password


def main(argv: list[str]) -> int:
    host, port, password = redis_conn_from_argv(argv)

    keys = build_binary_keys()

    with RedisCli(host, port) as redis:
        if password:
            redis.auth(password)

        for i, key in enumerate(keys):
            value = f"binary-keep-bytes#{i}".encode("utf-8")
            redis.set(key, value)
            preview = key.decode("utf-8", errors="replace")
            print(f"SET [{i:03d}] {preview!r} ({len(key)} bytes, invalid-utf8)")

    print(f"done. {COUNT}× {PREFIX.decode('ascii')}* → SCAN 后应仍带非空 bytes。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
