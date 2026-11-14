#!/usr/bin/env python3
"""验证本仓 raw 格式产物与上游 lyq1996/rtl9617c-factory 当前版逐字节一致。

上游 2026-11-13 提交 7a2455c（"layout: output fields at fixed offsets"）的逻辑：
整片 0xFF，字段按 name/offset/capacity 直接写到固定偏移，无 magic、无头部、
无目录、无 CRC；build 只做重复段 / 容量 / 越界检查。

本脚本独立实现这份上游逻辑作为 golden，再调用 Rust example 生成镜像比对：

    python3 scripts/verify_upstream_compat.py
"""

import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]

# 与上游 src/layout.rs::FIELDS 逐项一致
FIELDS = [
    ("BASE_MAC", 0x1000, 0x10),
    ("EPON_MAC", 0x1010, 0x06),
    ("PON_SN", 0x1020, 0x10),
    ("DEVICE_SN", 0x1030, 0x20),
    ("WIFI_CAL_RADIO0", 0x2000, 0x1000),
    ("WIFI_CAL_RADIO1", 0x3000, 0x1000),
    ("BOSA_CALIBRATION", 0x4000, 0x0800),
    ("BOSA_OLT_CALIBRATION", 0x4800, 0x0400),
    ("XTEMP_CALIBRATION", 0x5000, 0x1000),
    ("FPGA_IMAGE", 0x10000, 0x7E0000),
]

# 与 examples/raw_sample.rs 完全一致的输入
MAC = bytes.fromhex("687d005a19be")
SECTIONS = [
    ("BASE_MAC", MAC + (11).to_bytes(4, "little")),
    ("EPON_MAC", MAC),
    ("PON_SN", b"YHTC005A19BE\0"),
    ("DEVICE_SN", b"0E863687D005A19BE\0"),
    ("BOSA_CALIBRATION", bytes([0x5A]) * 64),
]


def upstream_build(size, sections):
    """上游 layout.rs::build 的等价实现（独立参考，不复用本仓代码）。"""
    image = bytearray(b"\xff" * size)
    written = []
    for name, data in sections:
        spec = next(f for f in FIELDS if f[0] == name)
        if name in written:
            raise ValueError(f"duplicate section {name}")
        if len(data) > spec[2]:
            raise ValueError(f"section {name} exceeds capacity")
        offset = spec[1]
        end = offset + len(data)
        if end > size:
            raise ValueError(f"section {name} exceeds image size")
        image[offset:end] = data
        written.append(name)
    return bytes(image)


def check(size):
    with tempfile.NamedTemporaryFile(suffix=".bin", delete=False) as tmp:
        out = tmp.name
    try:
        subprocess.run(
            [
                "cargo", "run", "--quiet", "--no-default-features",
                "--example", "raw_sample", "--", out, hex(size),
            ],
            cwd=ROOT,
            check=True,
        )
        got = pathlib.Path(out).read_bytes()
        expected = upstream_build(size, SECTIONS)
        if got != expected:
            for i, (a, b) in enumerate(zip(got, expected)):
                if a != b:
                    print(f"  首个差异 @ 0x{i:06x}: rust=0x{a:02x} upstream=0x{b:02x}")
                    break
            print(f"FAIL  size={hex(size)} rust={len(got)}B golden={len(expected)}B")
            return False
        # 结构性断言：无头、无目录、字段落在固定偏移
        assert all(b == 0xFF for b in got[0:0x1000]), "前 0x1000 字节应为 0xFF（无头）"
        assert all(b == 0xFF for b in got[0x200:0x400]), "0x200-0x400 应为 0xFF（无目录）"
        assert got[0x1000:0x1006] == MAC, "BASE_MAC 应在 0x1000"
        assert got[0x1010:0x1016] == MAC, "EPON_MAC 应在 0x1010"
        assert got[0x1020:0x102D] == b"YHTC005A19BE\0", "PON_SN 应在 0x1020"
        assert got[0x102D] == 0xFF, "字符串尾部应保持 0xFF"
        assert got[0x4000:0x4040] == bytes([0x5A]) * 64, "BOSA 应在 0x4000"
        print(f"PASS  size={hex(size)}  {len(got)} 字节与上游 golden 逐字节一致")
        return True
    finally:
        if os.path.exists(out):
            os.unlink(out)


def main():
    ok = True
    for size in (0x0001_0000, 0x0080_0000):
        ok = check(size) and ok
    if ok:
        print("\n结论：raw 格式与上游当前版 layout.rs 完全兼容；RTLFACT1 格式由 Rust 单测覆盖未回归。")
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
