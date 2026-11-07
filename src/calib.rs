use crate::xml::{field, optional_field};

pub struct Radio {
    pub wlan_index: u32,
}

pub const RADIO0_5G: Radio = Radio { wlan_index: 0 };
pub const RADIO1_2G: Radio = Radio { wlan_index: 1 };

fn byte_field(hs: &str, key: &str) -> Result<u8, String> {
    let value = field(hs, key)?;
    let parsed = value
        .trim()
        .trim_start_matches("0x")
        .parse::<u32>()
        .map_err(|_| format!("invalid numeric field {key}: {value}"))?;
    u8::try_from(parsed).map_err(|_| format!("field {key} out of byte range: {value}"))
}

fn hex_bytes_field(hs: &str, key: &str) -> Result<Vec<u8>, String> {
    let value = field(hs, key)?;
    value
        .split(',')
        .map(|part| u8::from_str_radix(part.trim(), 16).map_err(|_| format!("invalid hex field {key}: {value}")))
        .collect()
}

fn mac_address(base: [u8; 6], offset: u32) -> [u8; 6] {
    let value = u64::from(base[0]) << 40
        | u64::from(base[1]) << 32
        | u64::from(base[2]) << 24
        | u64::from(base[3]) << 16
        | u64::from(base[4]) << 8
        | u64::from(base[5]);
    let shifted = value + u64::from(offset);
    [
        (shifted >> 40) as u8,
        (shifted >> 32) as u8,
        (shifted >> 24) as u8,
        (shifted >> 16) as u8,
        (shifted >> 8) as u8,
        shifted as u8,
    ]
}

pub fn flash_calibration(hs: &str, radio: &Radio, base_mac: [u8; 6]) -> Result<Vec<u8>, String> {
    let mut data = Vec::with_capacity(386);
    data.extend_from_slice(b"TYFC0003");
    for key in [
        "HW_RX_GAINGAP_2G_CCK_AB",
        "HW_RX_GAINGAP_2G_OFDM_AB",
        "HW_RX_GAINGAP_5GL_AB",
        "HW_RX_GAINGAP_5GM_AB",
        "HW_RX_GAINGAP_5GH_AB",
    ] {
        data.push(byte_field(hs, key)?);
    }
    for path in ["A", "B"] {
        data.push(byte_field(hs, &format!("HW_THERMAL_{path}_WLAN{}", radio.wlan_index))?);
    }
    data.push(byte_field(hs, &format!("HW_XCAP_WLAN{}_AX", radio.wlan_index))?);
    for band in ["2G_CCK", "2G_BW40_1S", "5G_BW40_1S"] {
        for path in ["A", "B"] {
            data.extend_from_slice(&hex_bytes_field(hs, &format!("HW_{band}_TSSI_{path}"))?);
        }
    }
    let address = mac_address(base_mac, 2 - radio.wlan_index);
    data.extend_from_slice(&address);
    for kind in ["GAIN", "CW"] {
        for path in ["A", "B"] {
            data.extend_from_slice(&hex_bytes_field(
                hs,
                &format!("HW_2G_CCK_TSSI_{kind}_DIFF_{path}"),
            )?);
            for band in ["2G", "5G"] {
                data.extend_from_slice(&hex_bytes_field(
                    hs,
                    &format!("HW_{band}_TSSI_{kind}_DIFF_{path}"),
                )?);
            }
        }
    }
    let slope = hex_bytes_field(hs, "HW_5G_TSSI_CW_DIFF_A")?;
    let slope = *slope.last().ok_or("empty slope field")?;
    for seed in [0u8, slope] {
        for width in 0u8..4 {
            for _ in 0..14 {
                data.push(if width == 1 { 0 } else { seed });
            }
        }
    }
    Ok(data)
}

pub fn epon_llid_mac(config_xml: &str) -> Option<String> {
    let start = config_xml.find("<chain chainName=\"EPON_LLID_TBL\">")?;
    let remainder = &config_xml[start..];
    let end = remainder.find("</chain>")?;
    let chain = &remainder[..end];
    optional_field(chain, "macAddr").filter(|value| !value.is_empty())
}

/// PON_MODULE_EEPROM 段（tag 0x0102）的容量上限：256B A2h 基页 + 4 个私有页
/// （Table 0x80 / 0x81 / T2 / T3，各 128B）。
pub const A2H_DUMP_MAX: usize = 0x400;

/// 校验 i2cdump 出的光模块 A2h 转储（SFF-8472 外部校准布局）：
/// `0x5F = sum(0x00..0x5E) & 0xFF`（rtkbosa 原话：A2.5Fh = sum[A2.00h-0x5Eh]）。
/// 不足 0x60 字节的部分转储跳过校验。
pub fn validate_a2h_dump(data: &[u8]) -> Result<(), String> {
    if data.len() > A2H_DUMP_MAX {
        return Err(format!(
            "PON EEPROM dump too large: {} > {} bytes",
            data.len(),
            A2H_DUMP_MAX
        ));
    }
    if data.len() >= 0x60 {
        let sum = data[..0x5F].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        if sum != data[0x5F] {
            return Err(format!(
                "A2h checksum mismatch: sum(0x00..0x5E)=0x{sum:02x}, stored CC_EXT=0x{:02x}",
                data[0x5F]
            ));
        }
    }
    Ok(())
}

/// WIFI_NV_CFG 段（tag 0x0103）的容量上限（PT1570 实测 598 字节，留余量）。
pub const NV_CFG_MAX: usize = 0x400;

/// 归一化 NV_CFG 输入：接受**裸 NV_CFG 段**或**整个 mtd3-static_conf 分区镜像**
/// （256 KiB，NV_CFG 在头部，其后为 0x00 填充）。返回裁掉尾部 0x00 填充后的
/// NV_CFG 段（含魔数与标记）。字段为 ASCII 文本（无内嵌 NUL），按最后一个
/// 非 0 字节截断是安全的。
pub fn normalize_nv_cfg(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 21 {
        return Err("NV_CFG dump too small (< 21 bytes)".into());
    }
    if data[0..4] != [0x72, 0xd6, 0xef, 0x41] {
        return Err("NV_CFG magic mismatch (expect 72 D6 EF 41 at offset 0)".into());
    }
    if &data[4..21] != b"\x13/** NV_CFG **/\n\x00" {
        return Err("NV_CFG marker mismatch (expect /** NV_CFG **/)".into());
    }
    let end = data
        .iter()
        .rposition(|&b| b != 0)
        .map_or(21, |i| i + 1)
        .max(21);
    if end > NV_CFG_MAX {
        return Err(format!(
            "NV_CFG section too large: {} > {} bytes",
            end,
            NV_CFG_MAX
        ));
    }
    Ok(data[..end].to_vec())
}

/// 校验已归一化的 NV_CFG 段：魔数 `72 D6 EF 41` + `/** NV_CFG **/`
/// 标记。PT1570 实测布局：魔数(4B) + 0x13 + "/** NV_CFG **/\n" + NUL +
/// `KEY=VALUE` 列表（NUL 分隔）。整段原样存档，不解析不重组。
pub fn validate_nv_cfg(data: &[u8]) -> Result<(), String> {
    if data.len() > NV_CFG_MAX {
        return Err(format!(
            "NV_CFG dump too large: {} > {} bytes",
            data.len(),
            NV_CFG_MAX
        ));
    }
    if data.len() >= 21 {
        if data[0..4] != [0x72, 0xd6, 0xef, 0x41] {
            return Err("NV_CFG magic mismatch (expect 72 D6 EF 41)".into());
        }
        if &data[4..21] != b"\x13/** NV_CFG **/\n\x00" {
            return Err("NV_CFG marker mismatch (expect /** NV_CFG **/)".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_offset_carry() {
        let base = [0xd0, 0x5b, 0xcb, 0x9c, 0x9b, 0xf0];
        assert_eq!(mac_address(base, 2), [0xd0, 0x5b, 0xcb, 0x9c, 0x9b, 0xf2]);
        let max = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
        assert_eq!(mac_address(max, 2), [0x00, 0x00, 0x00, 0x00, 0x01, 0x01]);
    }

    #[test]
    fn hex_lists() {
        let doc = r#"<Value Name="HW_2G_CCK_TSSI_A" Value="fe,fe,09,04,04,04"/>"#;
        assert_eq!(
            hex_bytes_field(doc, "HW_2G_CCK_TSSI_A").unwrap(),
            vec![0xfe, 0xfe, 0x09, 0x04, 0x04, 0x04]
        );
    }

    #[test]
    fn nv_cfg_validation() {
        let mut d = vec![0x72, 0xd6, 0xef, 0x41];
        d.extend_from_slice(b"\x13/** NV_CFG **/\n\x00HW_X=1\0");
        assert!(validate_nv_cfg(&d).is_ok());
        // 不足 21 字节：跳过魔数校验（部分转储）
        assert!(validate_nv_cfg(&d[..15]).is_ok());
        let mut bad_magic = d.clone();
        bad_magic[0] = 0x00;
        assert!(validate_nv_cfg(&bad_magic).is_err());
        let mut bad_marker = d.clone();
        bad_marker[4] = 0x00;
        assert!(validate_nv_cfg(&bad_marker).is_err());
        assert!(validate_nv_cfg(&vec![0u8; 0x401]).is_err());
    }

    #[test]
    fn nv_cfg_normalize_partition_image() {
        // 整个 mtd3 分区镜像：NV_CFG 在头部（598 字节），其后全 0x00 填充
        let mut part = vec![0x72, 0xd6, 0xef, 0x41];
        part.extend_from_slice(b"\x13/** NV_CFG **/\n\x00HW_A=1\0HW_B=2\0");
        let blob_len = part.len();
        part.resize(262144, 0x00);
        let blob = normalize_nv_cfg(&part).unwrap();
        assert_eq!(blob.len(), blob_len);
        assert_eq!(&blob[..21], &b"\x72\xd6\xef\x41\x13/** NV_CFG **/\n\x00"[..]);
        assert!(validate_nv_cfg(&blob).is_ok());
        // 裸段原样通过
        let bare = normalize_nv_cfg(&part[..blob_len]).unwrap();
        assert_eq!(bare, blob);
        // 魔数不对拒绝
        let mut bad = part.clone();
        bad[0] = 0x00;
        assert!(normalize_nv_cfg(&bad).is_err());
        assert!(normalize_nv_cfg(&vec![0u8; 100]).is_err());
    }

    #[test]
    fn a2h_dump_validation() {
        let mut d = vec![0u8; 0x60];
        d[0x5F] = d[..0x5F].iter().fold(0u8, |a, &b| a.wrapping_add(b));
        assert!(validate_a2h_dump(&d).is_ok());
        d[0x5F] = d[0x5F].wrapping_add(1);
        assert!(validate_a2h_dump(&d).is_err());
        assert!(validate_a2h_dump(&vec![0u8; 0x401]).is_err());
    }
}
