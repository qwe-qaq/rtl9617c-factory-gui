//! 生成一份 raw（上游固定偏移）格式样例镜像，用于与上游 CLI 做逐字节比对。
//!
//! ```text
//! cargo run --no-default-features --example raw_sample -- <out.bin> [0x10000|0x800000]
//! ```
//!
//! 字段数据与 `scripts/verify_upstream_compat.py` 中的 golden 输入保持一致。

use std::env;

use rtl9617c_factory::layout::{self, Section};

fn parse_size(s: &str) -> Result<usize, String> {
    if let Some(hex) = s.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).map_err(|e| e.to_string())
    } else {
        s.parse::<usize>().map_err(|e| e.to_string())
    }
}

fn main() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let out = args.next().ok_or("usage: raw_sample <out.bin> [size]")?;
    let size = args
        .next()
        .map(|s| parse_size(&s))
        .transpose()?
        .unwrap_or(0x0001_0000);

    let mac = [0x68u8, 0x7d, 0x00, 0x5a, 0x19, 0xbe];
    let mut base_mac = mac.to_vec();
    base_mac.extend_from_slice(&11u32.to_le_bytes());
    let sections = vec![
        Section { tag: 0x0001, data: base_mac },
        Section { tag: 0x0002, data: mac.to_vec() },
        Section { tag: 0x0010, data: b"YHTC005A19BE\0".to_vec() },
        Section { tag: 0x0011, data: b"0E863687D005A19BE\0".to_vec() },
        Section { tag: 0x0110, data: vec![0x5a; 64] },
    ];

    let bin = layout::build_raw(size, &sections)?;
    std::fs::write(&out, &bin).map_err(|e| format!("cannot write {out}: {e}"))?;
    for entry in layout::summarize_raw(&sections)? {
        println!(
            "  0x{:06x}  {:<22} size={:<8} crc=0x{:08x}",
            entry.offset, entry.name, entry.size, entry.crc
        );
    }
    println!("wrote {} bytes ({}) format=raw", bin.len(), out);
    Ok(())
}
