#!/usr/bin/env python3
"""向 Redis 写入若干 pickle 样例键，供 RedisME「Pickle」查看验证。

需 Python 3.8+，无第三方依赖（纯 socket + RESP）。默认连本仓库 docker-compose 单机：
localhost:6379，密码 hepengju。

用法::

    python PickleSeed.py
    python PickleSeed.py 127.0.0.1 6379 hepengju
    python PickleSeed.py 127.0.0.1 6379 ""          # 无密码

写入的键前缀均为 encoding:pickle:，便于与其它编码测试键聚在一起；值页选 Pickle / Auto 查看。
"""

from __future__ import annotations

import pickle
import socket
import sys
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta, timezone
from decimal import Decimal
from enum import Enum
from typing import Any

PREFIX = "encoding:pickle:"
# 与应用内置 pickleparser 覆盖范围对齐；protocol 4 覆盖主流 Python 3
PROTOCOL = 4


class DemoRole(Enum):
    ADMIN = "admin"
    USER = "user"
    GUEST = "guest"


@dataclass
class DemoPoint:
    x: int
    y: int


@dataclass
class DemoPerson:
    name: str
    age: int
    home: DemoPoint


class DemoUser:
    """普通可 pickle 的自定义类（属性写在 __dict__）。"""

    def __init__(
        self,
        id: int,
        name: str,
        active: bool,
        birthday: date,
        created_at: datetime,
        role: DemoRole,
    ) -> None:
        self.id = id
        self.name = name
        self.active = active
        self.birthday = birthday
        self.created_at = created_at
        self.role = role


def build_samples() -> dict[str, Any]:
    """覆盖常见标量 / 容器 / bytes / datetime / Decimal / 自定义类。"""
    m: dict[str, Any] = {}
    m[PREFIX + "string"] = "hello-pickle"
    m[PREFIX + "int"] = 42
    m[PREFIX + "long"] = 9_876_543_210
    m[PREFIX + "float"] = 3.14159
    m[PREFIX + "bool"] = True
    m[PREFIX + "none"] = None
    m[PREFIX + "bytes"] = b"abc\x00\xff"
    m[PREFIX + "bytearray"] = bytearray(b"xyz")
    m[PREFIX + "complex"] = 1 + 2j
    m[PREFIX + "decimal"] = Decimal("12345.6789")
    m[PREFIX + "enum"] = DemoRole.ADMIN

    m[PREFIX + "date"] = date(2024, 6, 1)
    m[PREFIX + "time"] = time(14, 30, 0)
    m[PREFIX + "datetime"] = datetime(2024, 6, 1, 14, 30, 0)
    m[PREFIX + "datetime-utc"] = datetime(2024, 6, 1, 6, 30, 0, tzinfo=timezone.utc)
    m[PREFIX + "timedelta"] = timedelta(hours=1, minutes=30, seconds=5)

    m[PREFIX + "point"] = DemoPoint(3, 4)
    m[PREFIX + "person"] = DemoPerson("Bob", 20, DemoPoint(1, 2))
    m[PREFIX + "user"] = DemoUser(
        id=1001,
        name="Alice",
        active=True,
        birthday=date(1990, 1, 15),
        created_at=datetime(2024, 6, 1, 10, 0, 0),
        role=DemoRole.USER,
    )

    m[PREFIX + "list"] = ["a", "b", "中文", 1]
    m[PREFIX + "tuple"] = (1, "x", True)
    m[PREFIX + "set"] = {1, 2, 3}
    m[PREFIX + "frozenset"] = frozenset({"s1", "s2"})
    m[PREFIX + "dict"] = {"k1": "v1", "k2": 7, "day": date(2024, 12, 25)}
    m[PREFIX + "nested"] = {
        "list": [1, 2, {"n": "中文"}],
        "tuple": (b"raw", None),
        "point": DemoPoint(9, 0),
    }

    # 不同协议各写一条，便于对照 Auto / Pickle 识别
    m[PREFIX + "proto2-dict"] = {"proto": 2, "msg": "ok"}
    m[PREFIX + "proto5-list"] = [1, "x", True]
    return m


def dumps(obj: Any, *, protocol: int = PROTOCOL) -> bytes:
    return pickle.dumps(obj, protocol=protocol)


class RedisCli:
    """最小 RESP 客户端：AUTH / SET 二进制值。"""

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

    def set(self, key: str, value: bytes) -> None:
        self._write_command(b"SET", key.encode("utf-8"), value)
        self._read_ok()

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


def main(argv: list[str]) -> int:
    host = argv[1] if len(argv) > 1 else "127.0.0.1"
    port = int(argv[2]) if len(argv) > 2 else 6379
    password = argv[3] if len(argv) > 3 else "hepengju"

    samples = build_samples()
    with RedisCli(host, port) as redis:
        if password:
            redis.auth(password)
        for key, value in samples.items():
            if key.endswith("proto2-dict"):
                payload = dumps(value, protocol=2)
            elif key.endswith("proto5-list"):
                payload = dumps(value, protocol=5)
            else:
                payload = dumps(value)
            redis.set(key, payload)
            proto = payload[1] if len(payload) >= 2 and payload[0] == 0x80 else "?"
            print(f"SET {key} ({len(payload)} bytes, proto={proto})")

    print("done. Switch codec to Pickle (or Auto) in RedisME to inspect.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
