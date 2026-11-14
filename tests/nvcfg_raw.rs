//! 端到端验证用户诉求：PT1570 的 NV_CFG 只用来组装 Wi-Fi 校准段，
//! 上游 raw（64 KiB）镜像里写入校准段、不写入 NV_CFG 原档。
//!
//! 覆盖真实调用链：normalize_nv_cfg → flash_calibration_nv → filter_raw_sections
//! → build_raw / summarize_raw，并确认 RTLFACT1 路径未回归。

use rtl9617c_factory::{calib, layout};

/// 构造一个合法的 mtd3 NV_CFG 段（魔数 + 标记 + KEY=VALUE 列表）。
fn nv_cfg_blob() -> Vec<u8> {
    let text = concat!(
        "HW_RX_GAINGAP_2G_CCK_AB=0x60\0",
        "HW_RX_GAINGAP_2G_OFDM_AB=0x60\0",
        "HW_RX_GAINGAP_5GL_AB=0xFE\0",
        "HW_RX_GAINGAP_5GM_AB=0xF0\0",
        "HW_RX_GAINGAP_5GH_AB=0x00\0",
        "HW_WLAN0_FEM_TYPE=17\0HW_WLAN1_FEM_TYPE=238\0",
        "HW_THERMAL_A_WLAN1=32\0HW_THERMAL_B_WLAN1=32\0HW_XCAP_WLAN1_AX=100\0",
        "HW_5G_BW40_1S_TSSI_A=FD FE FE FC 00 02 02 FD 00 04 06 03 03 07\0",
        "HW_5G_BW40_1S_TSSI_B=00 00 FF FD 01 02 00 FA 00 04 05 02 01 05\0",
        "HW_THERMAL_A_WLAN0=29\0HW_THERMAL_B_WLAN0=29\0HW_XCAP_WLAN0_AX=68\0",
        "HW_2G_CCK_TSSI_A=08 08 07 00 00 00\0HW_2G_CCK_TSSI_B=FB FB FA F7 F7 F7\0",
        "HW_2G_BW40_1S_TSSI_A=06 06 05 02 02\0HW_2G_BW40_1S_TSSI_B=F9 F9 F9 F8 F8\0",
    );
    let mut v = vec![0x72, 0xd6, 0xef, 0x41];
    v.extend_from_slice(b"\x13/** NV_CFG **/\n\x00");
    v.extend_from_slice(text.as_bytes());
    v
}

#[test]
fn nvcfg_raw_writes_calibration_only() {
    // 1) 导入 NV_CFG 并据此组装两段 Wi-Fi 校准（与 GUI/CLI 的 pt1570 流程一致）
    let blob = nv_cfg_blob();
    let nv = calib::normalize_nv_cfg(&blob).unwrap();
    calib::validate_nv_cfg(&nv).unwrap();
    let text = String::from_utf8_lossy(&nv).to_string();
    let base = [0x68, 0x7d, 0x00, 0x5a, 0x19, 0xbe];
    let wifi0 = calib::flash_calibration_nv(&text, &calib::RADIO1_5G, base).unwrap();
    let wifi1 = calib::flash_calibration_nv(&text, &calib::RADIO0_2G, base).unwrap();
    assert_eq!(&wifi0[..8], b"TYFC0003");
    assert_eq!(wifi0.len(), wifi1.len());

    // 2) 模拟生成段表：标准字段 + 校准段 + NV_CFG「原样存档」段
    let mut base_mac = base.to_vec();
    base_mac.extend_from_slice(&11u32.to_le_bytes());
    let sections = vec![
        layout::Section { tag: 0x0001, data: base_mac },
        layout::Section { tag: 0x0100, data: wifi0.clone() },
        layout::Section { tag: 0x0101, data: wifi1.clone() },
        layout::Section { tag: 0x0103, data: nv.clone() },
    ];

    // 3) raw 模式过滤：只跳过 NV_CFG 原档，校准段必须留下
    let (kept, skipped) = layout::filter_raw_sections(&sections);
    assert_eq!(kept.len(), 3);
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].contains("WIFI_NV_CFG"));

    let image = layout::build_raw(0x0001_0000, &kept).unwrap();
    assert_eq!(image.len(), 64 * 1024);
    assert_eq!(&image[0x2000..0x2000 + wifi0.len()], &wifi0[..]);
    assert_eq!(&image[0x3000..0x3000 + wifi1.len()], &wifi1[..]);
    assert!(
        image[0x6000..0x8000].iter().all(|&b| b == 0xff),
        "0x6000（原 EEPROM 槽）/ 0x7000（NV_CFG 槽）必须保持空白 0xFF"
    );

    let entries = layout::summarize_raw(&kept).unwrap();
    assert!(entries.iter().any(|e| e.name == "WIFI_CAL_RADIO0"));
    assert!(entries.iter().any(|e| e.name == "WIFI_CAL_RADIO1"));
    assert!(!entries.iter().any(|e| e.name.contains("NV_CFG")));

    // 4) RTLFACT1 路径未回归：原档仍完整写入目录表
    let rtl1 = layout::build(layout::MODEL_PT1570, 0, &sections).unwrap();
    let rtl1_entries = layout::summary(&rtl1).unwrap();
    assert!(rtl1_entries.iter().any(|e| e.name == "WIFI_NV_CFG"));
    assert!(rtl1_entries.iter().any(|e| e.name == "WIFI_CAL_RADIO0"));
}
