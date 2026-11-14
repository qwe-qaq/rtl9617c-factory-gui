use crate::crc::{register, zlib, SEED};

pub const IMAGE_SIZE: usize = 0x0080_0000;
pub const MAGIC: &[u8; 8] = b"RTLFACT1";
pub const VERSION: u32 = 1;
pub const MODEL_TYFZA1: u32 = 1;
pub const MODEL_PT1500_H10: u32 = 2;
pub const MODEL_PT1570: u32 = 3;
pub const MODEL_PT1570_R10: u32 = 4;

const DIR_OFFSET: usize = 0x0200;
const DIR_COUNT: usize = 32;
const ENTRY_SIZE: usize = 16;
const IMAGE_CRC_OFFSET: usize = 0x14;
const TABLE_CRC_OFFSET: usize = 0x20;
const HEADER_CRC_OFFSET: usize = 0x24;

pub struct FieldSpec {
    pub tag: u16,
    pub name: &'static str,
    pub offset: u32,
    pub capacity: u32,
}

pub const FIELDS: &[FieldSpec] = &[
    FieldSpec { tag: 0x0001, name: "BASE_MAC", offset: 0x1000, capacity: 0x10 },
    FieldSpec { tag: 0x0002, name: "EPON_MAC", offset: 0x1010, capacity: 0x06 },
    FieldSpec { tag: 0x0010, name: "PON_SN", offset: 0x1020, capacity: 0x10 },
    FieldSpec { tag: 0x0011, name: "DEVICE_SN", offset: 0x1030, capacity: 0x20 },
    FieldSpec { tag: 0x0100, name: "WIFI_CAL_RADIO0", offset: 0x2000, capacity: 0x1000 },
    FieldSpec { tag: 0x0101, name: "WIFI_CAL_RADIO1", offset: 0x3000, capacity: 0x1000 },
    FieldSpec { tag: 0x0110, name: "BOSA_CALIBRATION", offset: 0x4000, capacity: 0x0800 },
    FieldSpec { tag: 0x0111, name: "BOSA_OLT_CALIBRATION", offset: 0x4800, capacity: 0x0400 },
    FieldSpec { tag: 0x0112, name: "XTEMP_CALIBRATION", offset: 0x5000, capacity: 0x1000 },
    // env 机型（pt1500-h10 / pt1570）的 PON 口校准不在 flash 里，而在光模块
    // A2h EEPROM（SFF-8472，I2C 0x51）。这里存用户从原厂固件 i2cdump 得到的
    // 转储：0x0000-0x00FF 为 A2h 基页（0x5F 为 CC_EXT 校验和），其后可按
    // README 追加私有页（0x80 / 0x81 / T2 / T3，各 128B）。
    FieldSpec { tag: 0x0102, name: "PON_MODULE_EEPROM", offset: 0x6000, capacity: 0x400 },
    // pt1570 的 Wi-Fi 校准字段在 mtd3_static_conf 的 NV_CFG 区（/** NV_CFG **/ +
    // KEY=VALUE\0 列表），缺 TYFC0003 组装所需的 TSSI DIFF 字段，故按原文存档，
    // 不组装。消费端（WLAN 驱动）直接读 NV 分区，此处为存档用途。
    FieldSpec { tag: 0x0103, name: "WIFI_NV_CFG", offset: 0x7000, capacity: 0x400 },
    FieldSpec { tag: 0x0200, name: "FPGA_IMAGE", offset: 0x10000, capacity: 0x7E0000 },
];

pub fn spec(tag: u16) -> Result<&'static FieldSpec, String> {
    FIELDS
        .iter()
        .find(|f| f.tag == tag)
        .ok_or_else(|| format!("unknown factory tag 0x{tag:04x}"))
}

// ---------------------------------------------------------------------------
// 上游 2026-11-13 之后的布局（commit 7a2455c, "layout: output fields at fixed
// offsets"）：无 magic / 无头部 / 无目录 / 无 CRC，字段按固定偏移直接写入，
// 其余区域全部 0xFF。这里作为第二种输出格式保留，字段表与上游
// `layout.rs::FIELDS` 逐项一致，生成的镜像与上游 CLI 逐字节相同。
// 原有的 RTLFACT1 build / build_compact / summary 完全不动。
// ---------------------------------------------------------------------------

/// 输出格式。`RtlFact1` 是本仓（及上游 10-28 初版）的带头部/目录/CRC 格式，
/// 支持紧凑镜像、model id 与本仓新增字段；`UpstreamRaw` 是上游 11-13 之后的
/// 纯固定偏移格式，字段表固定、无校验、镜像大小由机型决定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputFormat {
    RtlFact1,
    UpstreamRaw,
}

impl OutputFormat {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "rtl1" | "rtlfact1" | "gui" => Ok(OutputFormat::RtlFact1),
            "raw" | "upstream" | "upstream-raw" | "fixed" => Ok(OutputFormat::UpstreamRaw),
            other => Err(format!("unknown output format: {other} (use rtl1 or raw)")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::RtlFact1 => "rtl1",
            OutputFormat::UpstreamRaw => "raw",
        }
    }
}

/// 与上游 `layout.rs::FIELDS` 逐项一致（name / offset / capacity）。
pub struct RawField {
    pub name: &'static str,
    pub offset: u32,
    pub capacity: u32,
}

pub const RAW_FIELDS: &[RawField] = &[
    RawField { name: "BASE_MAC", offset: 0x1000, capacity: 0x10 },
    RawField { name: "EPON_MAC", offset: 0x1010, capacity: 0x06 },
    RawField { name: "PON_SN", offset: 0x1020, capacity: 0x10 },
    RawField { name: "DEVICE_SN", offset: 0x1030, capacity: 0x20 },
    RawField { name: "WIFI_CAL_RADIO0", offset: 0x2000, capacity: 0x1000 },
    RawField { name: "WIFI_CAL_RADIO1", offset: 0x3000, capacity: 0x1000 },
    RawField { name: "BOSA_CALIBRATION", offset: 0x4000, capacity: 0x0800 },
    RawField { name: "BOSA_OLT_CALIBRATION", offset: 0x4800, capacity: 0x0400 },
    RawField { name: "XTEMP_CALIBRATION", offset: 0x5000, capacity: 0x1000 },
    RawField { name: "FPGA_IMAGE", offset: 0x10000, capacity: 0x7E0000 },
];

pub fn raw_field(name: &str) -> Result<&'static RawField, String> {
    RAW_FIELDS
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("unknown upstream factory field {name}"))
}

/// 本仓 tag → 上游字段名。返回 `None` 表示本仓新增字段（0x0102 / 0x0103）：
/// 上游固定字段表没有对应位置，raw 模式必须显式报错，绝不静默丢弃数据。
pub fn upstream_name(tag: u16) -> Option<&'static str> {
    match tag {
        0x0001 => Some("BASE_MAC"),
        0x0002 => Some("EPON_MAC"),
        0x0010 => Some("PON_SN"),
        0x0011 => Some("DEVICE_SN"),
        0x0100 => Some("WIFI_CAL_RADIO0"),
        0x0101 => Some("WIFI_CAL_RADIO1"),
        0x0110 => Some("BOSA_CALIBRATION"),
        0x0111 => Some("BOSA_OLT_CALIBRATION"),
        0x0112 => Some("XTEMP_CALIBRATION"),
        0x0200 => Some("FPGA_IMAGE"),
        _ => None,
    }
}

/// 按上游 `layout::build` 的语义生成镜像：整片 0xFF，字段直接落在固定偏移，
/// 不写 magic、不写头、不写目录、不算 CRC。检查项（重复段 / 容量 / 越界）
/// 与上游一致。
pub fn build_raw(image_size: usize, sections: &[Section]) -> Result<Vec<u8>, String> {
    let mut image = vec![0xffu8; image_size];
    let mut written: Vec<&'static str> = Vec::new();
    for section in sections {
        let name = upstream_name(section.tag).ok_or_else(|| {
            format!(
                "section tag 0x{:04x} has no upstream-layout counterpart; use --format rtl1",
                section.tag
            )
        })?;
        let spec = raw_field(name)?;
        if written.contains(&name) {
            return Err(format!("duplicate section {name}"));
        }
        if section.data.len() > spec.capacity as usize {
            return Err(format!(
                "section {} exceeds capacity: {} > {}",
                name,
                section.data.len(),
                spec.capacity
            ));
        }
        let offset = spec.offset as usize;
        let end = offset + section.data.len();
        if end > image_size {
            return Err(format!(
                "section {} exceeds image size: 0x{end:x} > 0x{image_size:x}",
                name
            ));
        }
        image[offset..end].copy_from_slice(&section.data);
        written.push(name);
    }
    Ok(image)
}

/// raw 格式的段表（供 CLI / GUI 打印，字段与上游 `main.rs` 的 build 输出一致）。
pub fn summarize_raw(sections: &[Section]) -> Result<Vec<SummaryEntry>, String> {
    let mut entries: Vec<SummaryEntry> = sections
        .iter()
        .map(|s| {
            let name = upstream_name(s.tag).ok_or_else(|| {
                format!(
                    "section tag 0x{:04x} has no upstream-layout counterpart; use --format rtl1",
                    s.tag
                )
            })?;
            let spec = raw_field(name)?;
            Ok(SummaryEntry {
                name: spec.name,
                offset: spec.offset,
                size: s.data.len() as u32,
                crc: zlib(&s.data),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by_key(|e| e.offset);
    Ok(entries)
}

fn header_crc(image: &[u8]) -> u32 {
    let seed = register(&image[0x28..0x7C], SEED);
    let seed = register(&image[0x18..0x24], seed);
    register(&image[0x00..0x14], seed)
}

pub fn encode_string(value: &str, capacity: usize) -> Result<Vec<u8>, String> {
    let mut out = value.as_bytes().to_vec();
    out.push(0);
    if out.len() > capacity {
        return Err(format!(
            "string field does not fit: {} bytes > {} capacity",
            out.len(),
            capacity
        ));
    }
    Ok(out)
}

pub struct Section {
    pub tag: u16,
    pub data: Vec<u8>,
}

/// 紧凑镜像大小：完整镜像去掉 `FPGA_IMAGE`（0x10000 起）后的头部 + 目录 +
/// 身份/校准字段区。布局、CRC 链与完整镜像完全一致，仅 `image_size` 不同，
/// 适合留存身份数据与校准数据的小体积备份（不可直接刷写，恢复时用完整镜像）。
pub const COMPACT_SIZE: usize = 0x0001_0000;

fn assemble(
    model: u32,
    generation: u64,
    sections: &[Section],
    size: usize,
) -> Result<Vec<u8>, String> {
    let mut image = vec![0xffu8; size];
    image[0..8].copy_from_slice(MAGIC);
    image[8..12].copy_from_slice(&VERSION.to_le_bytes());
    image[12..16].copy_from_slice(&(size as u32).to_le_bytes());
    image[16..20].copy_from_slice(&model.to_le_bytes());
    image[24..32].copy_from_slice(&generation.to_le_bytes());
    image[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    let mut written: Vec<(u16, u32, u32, u32)> = Vec::new();
    for section in sections {
        let spec = spec(section.tag)?;
        if section.data.len() > spec.capacity as usize {
            return Err(format!(
                "section {} exceeds capacity: {} > {}",
                spec.name,
                section.data.len(),
                spec.capacity
            ));
        }
        let offset = spec.offset as usize;
        if offset + section.data.len() > size {
            return Err(format!(
                "section {} does not fit a {}-byte image",
                spec.name, size
            ));
        }
        let existing = written.iter().any(|(t, ..)| *t == section.tag);
        if existing {
            return Err(format!("duplicate section {}", spec.name));
        }
        image[offset..offset + section.data.len()].copy_from_slice(&section.data);
        written.push((
            section.tag,
            spec.offset,
            section.data.len() as u32,
            zlib(&section.data),
        ));
    }
    written.sort_by_key(|(t, ..)| *t);
    if written.len() > DIR_COUNT {
        return Err("too many sections for the directory".into());
    }
    for (slot, (tag, offset, size, crc)) in written.iter().enumerate() {
        let base = DIR_OFFSET + slot * ENTRY_SIZE;
        image[base..base + 2].copy_from_slice(&tag.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&1u16.to_le_bytes());
        image[base + 4..base + 8].copy_from_slice(&offset.to_le_bytes());
        image[base + 8..base + 12].copy_from_slice(&size.to_le_bytes());
        image[base + 12..base + 16].copy_from_slice(&crc.to_le_bytes());
    }
    let table_crc = register(&image[DIR_OFFSET..DIR_OFFSET + DIR_COUNT * ENTRY_SIZE], SEED);
    image[TABLE_CRC_OFFSET..TABLE_CRC_OFFSET + 4].copy_from_slice(&table_crc.to_le_bytes());
    let header_crc = header_crc(&image);
    image[HEADER_CRC_OFFSET..HEADER_CRC_OFFSET + 4].copy_from_slice(&header_crc.to_le_bytes());
    image[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    let image_crc = register(&image, SEED);
    image[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].copy_from_slice(&image_crc.to_le_bytes());
    Ok(image)
}

pub fn build(model: u32, generation: u64, sections: &[Section]) -> Result<Vec<u8>, String> {
    assemble(model, generation, sections, IMAGE_SIZE)
}

/// 紧凑镜像（`COMPACT_SIZE`，不含 FPGA_IMAGE；传入的 0x0200 段会被忽略）。
pub fn build_compact(
    model: u32,
    generation: u64,
    sections: &[Section],
) -> Result<Vec<u8>, String> {
    let filtered: Vec<Section> = sections
        .iter()
        .filter(|s| s.tag != 0x0200)
        .map(|s| Section {
            tag: s.tag,
            data: s.data.clone(),
        })
        .collect();
    assemble(model, generation, &filtered, COMPACT_SIZE)
}

/// 按段列表生成段表。完整镜像的校验走 `summary()`；紧凑镜像等无法整镜像
/// 校验的场景用本函数直接从段数据生成展示信息。
pub fn summarize_sections(sections: &[Section]) -> Result<Vec<SummaryEntry>, String> {
    let mut entries: Vec<SummaryEntry> = sections
        .iter()
        .map(|s| {
            let spec = spec(s.tag)?;
            Ok(SummaryEntry {
                name: spec.name,
                offset: spec.offset,
                size: s.data.len() as u32,
                crc: zlib(&s.data),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by_key(|e| e.offset);
    Ok(entries)
}

pub struct SummaryEntry {
    pub name: &'static str,
    pub offset: u32,
    pub size: u32,
    pub crc: u32,
}

pub fn summary(image: &[u8]) -> Result<Vec<SummaryEntry>, String> {
    if image.len() != IMAGE_SIZE || image[0..8] != *MAGIC {
        return Err("not an RTLFACT1 image".into());
    }
    let stored = u32::from_le_bytes(
        image[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].try_into().unwrap(),
    );
    let mut probe = image.to_vec();
    probe[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    if register(&probe, SEED) != stored {
        return Err("image CRC mismatch".into());
    }
    let stored_header_crc = u32::from_le_bytes(
        image[HEADER_CRC_OFFSET..HEADER_CRC_OFFSET + 4].try_into().unwrap(),
    );
    if header_crc(&image) != stored_header_crc {
        return Err("header CRC mismatch".into());
    }
    let mut entries = Vec::new();
    for slot in 0..DIR_COUNT {
        let base = DIR_OFFSET + slot * ENTRY_SIZE;
        let tag = u16::from_le_bytes(image[base..base + 2].try_into().unwrap());
        if tag == 0xffff {
            continue;
        }
        let present = u16::from_le_bytes(image[base + 2..base + 4].try_into().unwrap());
        if present & 1 == 0 {
            continue;
        }
        let offset = u32::from_le_bytes(image[base + 4..base + 8].try_into().unwrap());
        let size = u32::from_le_bytes(image[base + 8..base + 12].try_into().unwrap());
        let crc = u32::from_le_bytes(image[base + 12..base + 16].try_into().unwrap());
        let spec = spec(tag)?;
        if zlib(&image[offset as usize..offset as usize + size as usize]) != crc {
            return Err(format!("section {} CRC mismatch", spec.name));
        }
        entries.push(SummaryEntry {
            name: spec.name,
            offset,
            size,
            crc,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_verify_roundtrip() {
        let image = build(
            MODEL_TYFZA1,
            0,
            &[
                Section { tag: 0x0002, data: vec![1, 2, 3, 4, 5, 6] },
                Section { tag: 0x0010, data: b"TYZDD0046AB9\0".to_vec() },
            ],
        )
        .unwrap();
        assert_eq!(image.len(), IMAGE_SIZE);
        let entries = summary(&image).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "EPON_MAC");
        assert_eq!(entries[0].size, 6);
        assert_eq!(entries[1].name, "PON_SN");
        assert_eq!(&image[0x1020..0x1020 + 13], b"TYZDD0046AB9\0");
        assert_eq!(image[0x1010..0x1010 + 6], [1, 2, 3, 4, 5, 6]);
        assert_eq!(image[0x102D], 0xff);
    }

    #[test]
    fn rejects_oversize() {
        assert!(build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0010, data: vec![0x41; 32] }]
        )
        .is_err());
    }

    #[test]
    fn rejects_unknown_tag() {
        assert!(build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0012, data: vec![0x41; 4] }]
        )
        .is_err());
    }

    #[test]
    fn compact_image_layout_and_crc() {
        let sections = [
            Section { tag: 0x0002, data: vec![1, 2, 3, 4, 5, 6] },
            Section { tag: 0x0010, data: b"TYZDD0046AB9\0".to_vec() },
        ];
        let image = build_compact(MODEL_TYFZA1, 7, &sections).unwrap();
        assert_eq!(image.len(), COMPACT_SIZE);
        assert_eq!(&image[0..8], MAGIC);
        assert_eq!(
            u32::from_le_bytes(image[12..16].try_into().unwrap()),
            COMPACT_SIZE as u32
        );
        // FPGA 段被忽略：传入与否结果一致
        let with_fpga = build_compact(
            MODEL_TYFZA1,
            7,
            &[
                Section { tag: 0x0200, data: vec![0xaa; 64] },
                Section { tag: 0x0002, data: vec![1, 2, 3, 4, 5, 6] },
                Section { tag: 0x0010, data: b"TYZDD0046AB9\0".to_vec() },
            ],
        )
        .unwrap();
        assert_eq!(image, with_fpga);
        // CRC 链与完整镜像同规则：整镜像 / 目录
        let stored = u32::from_le_bytes(
            image[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4]
                .try_into()
                .unwrap(),
        );
        let mut probe = image.clone();
        probe[IMAGE_CRC_OFFSET..IMAGE_CRC_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(register(&probe, SEED), stored);
        assert_eq!(
            register(&image[DIR_OFFSET..DIR_OFFSET + DIR_COUNT * ENTRY_SIZE], SEED),
            u32::from_le_bytes(image[TABLE_CRC_OFFSET..TABLE_CRC_OFFSET + 4].try_into().unwrap())
        );
        assert_eq!(image[0x1010..0x1016], [1, 2, 3, 4, 5, 6]);
        assert_eq!(&image[0x1020..0x1020 + 13], b"TYZDD0046AB9\0");
        // summarize_sections 与 summary 的段表一致（对完整镜像而言）
        let full = build(MODEL_TYFZA1, 0, &sections).unwrap();
        let from_summary = summary(&full).unwrap();
        let from_sections = summarize_sections(&sections).unwrap();
        assert_eq!(from_summary.len(), from_sections.len());
        for (x, y) in from_summary.iter().zip(from_sections.iter()) {
            assert_eq!((x.name, x.offset, x.size, x.crc), (y.name, y.offset, y.size, y.crc));
        }
    }

    #[test]
    fn pon_module_eeprom_field() {
        let s = spec(0x0102).unwrap();
        assert_eq!(s.offset, 0x6000);
        assert_eq!(s.capacity, 0x400);
        let image = build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0102, data: vec![0x5a; 256] }],
        )
        .unwrap();
        assert_eq!(image[0x6000..0x6100], [0x5a; 256]);
        assert!(build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0102, data: vec![0; 0x401] }]
        )
        .is_err());
        // 紧凑镜像同样容纳该段（0x6000 < COMPACT_SIZE）
        let compact = build_compact(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0102, data: vec![0x5a; 256] }],
        )
        .unwrap();
        assert_eq!(compact[0x6000..0x6100], [0x5a; 256]);
    }

    #[test]
    fn wifi_nv_cfg_field() {
        let s = spec(0x0103).unwrap();
        assert_eq!(s.offset, 0x7000);
        assert_eq!(s.capacity, 0x400);
        let mut data = vec![0x72, 0xd6, 0xef, 0x41];
        data.extend_from_slice(b"\x13/** NV_CFG **/\n\x00HW_X=1\0");
        let image = build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0103, data: data.clone() }],
        )
        .unwrap();
        assert_eq!(&image[0x7000..0x7000 + data.len()], &data[..]);
        // 紧凑镜像同样容纳该段（0x7000 < COMPACT_SIZE）
        let compact = build_compact(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0103, data: data.clone() }],
        )
        .unwrap();
        assert_eq!(compact[0x7000], 0x72);
        assert!(build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0103, data: vec![0; 0x401] }]
        )
        .is_err());
    }

    // ---- 与上游 11-13 固定偏移布局的一致性 --------------------------------

    /// 独立参考实现：绕过 build_raw，直接按上游规则往 0xFF 数组里写字节。
    fn upstream_reference(size: usize, sections: &[(usize, &[u8])]) -> Vec<u8> {
        let mut image = vec![0xffu8; size];
        for (offset, data) in sections {
            image[*offset..*offset + data.len()].copy_from_slice(data);
        }
        image
    }

    #[test]
    fn raw_matches_upstream_reference_byte_for_byte() {
        let mac = [1u8, 2, 3, 4, 5, 6];
        let sn = b"TYZDD0046AB9\0";
        let sections = [
            Section { tag: 0x0002, data: mac.to_vec() },
            Section { tag: 0x0010, data: sn.to_vec() },
        ];
        let image = build_raw(0x10000, &sections).unwrap();
        let expected = upstream_reference(
            0x10000,
            &[(0x1010, &mac[..]), (0x1020, &sn[..])],
        );
        assert_eq!(image, expected);
        // 无 magic / 无头 / 无目录 / 无 CRC：前 0x1000 字节与目录区全为 0xFF
        assert!(image[0..0x1000].iter().all(|&b| b == 0xff));
        assert!(image[0x200..0x400].iter().all(|&b| b == 0xff));
        assert!(image[0..0x30].iter().all(|&b| b == 0xff));
        assert_eq!(image[0x1020 + 13], 0xff);
    }

    #[test]
    fn raw_64k_image_size_and_section_table() {
        let sections = [Section { tag: 0x0001, data: vec![0xaa; 10] }];
        let image = build_raw(0x10000, &sections).unwrap();
        assert_eq!(image.len(), 0x10000);
        let entries = summarize_raw(&sections).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "BASE_MAC");
        assert_eq!(entries[0].offset, 0x1000);
        assert_eq!(entries[0].size, 10);
        assert_eq!(entries[0].crc, zlib(&sections[0].data));
    }

    #[test]
    fn raw_rejects_oversize_duplicate_and_unknown_tag() {
        assert!(build_raw(0x2000, &[Section { tag: 0x0010, data: vec![0x41; 32] }]).is_err());
        assert!(build_raw(0x1000, &[Section { tag: 0x0001, data: vec![0x41; 16] }]).is_err());
        assert!(build_raw(
            0x10000,
            &[
                Section { tag: 0x0002, data: vec![1; 6] },
                Section { tag: 0x0002, data: vec![2; 6] },
            ]
        )
        .is_err());
        // 本仓新增字段必须显式拒绝，绝不静默丢弃
        assert!(build_raw(
            0x10000,
            &[Section { tag: 0x0102, data: vec![0x5a; 16] }]
        )
        .is_err());
    }

    #[test]
    fn rtl1_format_is_not_regressed_by_raw_layout() {
        let data = b"TYZDD0046AB9\0".to_vec();
        let image = build(
            MODEL_TYFZA1,
            0,
            &[Section { tag: 0x0010, data: data.clone() }],
        )
        .unwrap();
        assert_eq!(&image[0..8], MAGIC);
        assert_eq!(image.len(), IMAGE_SIZE);
        assert!(summary(&image).is_ok());
        // 同一组字段下两种格式的产物确实不同
        let raw = build_raw(IMAGE_SIZE, &[Section { tag: 0x0010, data }]).unwrap();
        assert_ne!(&image[0..0x30], &raw[0..0x30]);
    }

    #[test]
    fn output_format_parse() {
        assert_eq!(OutputFormat::parse("rtl1").unwrap(), OutputFormat::RtlFact1);
        assert_eq!(OutputFormat::parse("RTLFACT1").unwrap(), OutputFormat::RtlFact1);
        assert_eq!(OutputFormat::parse("raw").unwrap(), OutputFormat::UpstreamRaw);
        assert_eq!(OutputFormat::parse("upstream").unwrap(), OutputFormat::UpstreamRaw);
        assert!(OutputFormat::parse("bogus").is_err());
    }
}
