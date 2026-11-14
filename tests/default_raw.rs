//! 锁定用户确认的规格（r4）：
//! - `youhua-pt1570-r10` 别名解析到 `pt1570-r10`，其 raw 镜像大小为 64 KiB；
//! - 未指定 `--format` 时默认导出上游 raw 格式；
//! - 64 KiB raw 镜像可容纳该机型的身份与 Wi-Fi 校准字段。

use rtl9617c_factory::{layout, profile};

const R10_64K: usize = 0x0001_0000;

#[test]
fn r10_upstream_alias_and_raw_size() {
    assert_eq!(profile::canonical("youhua-pt1570-r10").unwrap(), "pt1570-r10");
    assert_eq!(profile::canonical("pt1570-r10").unwrap(), "pt1570-r10");
    assert_eq!(
        profile::upstream_image_size("youhua-pt1570-r10").unwrap(),
        R10_64K
    );
    assert_eq!(
        profile::upstream_image_size("pt1570-r10").unwrap(),
        R10_64K
    );
    assert_eq!(
        profile::model_id("youhua-pt1570-r10").unwrap(),
        layout::MODEL_PT1570_R10
    );
    // 其他机型规格不变
    assert_eq!(profile::upstream_image_size("tyfza1").unwrap(), 0x0080_0000);
    assert_eq!(
        profile::upstream_image_size("youhua-pt1500-h10-ct-2p5g").unwrap(),
        R10_64K
    );
}

#[test]
fn default_output_format_is_raw() {
    assert_eq!(
        layout::OutputFormat::default(),
        layout::OutputFormat::UpstreamRaw
    );
    assert_eq!(layout::OutputFormat::default().as_str(), "raw");
    // 显式切换仍能拿到 RTLFACT1
    assert_eq!(
        layout::OutputFormat::parse("rtl1").unwrap(),
        layout::OutputFormat::RtlFact1
    );
}

#[test]
fn r10_raw_image_fits_in_64k() {
    let mut base_mac = vec![0x68, 0x7d, 0x00, 0x5a, 0x19, 0xbe];
    base_mac.extend_from_slice(&11u32.to_le_bytes());
    let sections = vec![
        layout::Section { tag: 0x0001, data: base_mac },
        layout::Section { tag: 0x0100, data: vec![0x5a; 284] },
        layout::Section { tag: 0x0101, data: vec![0x5b; 284] },
        // NV_CFG 原档：raw 模式下应被跳过，不写入镜像
        layout::Section { tag: 0x0103, data: vec![0xcc; 598] },
    ];
    let (kept, skipped) = layout::filter_raw_sections(&sections);
    assert_eq!(kept.len(), 3);
    assert_eq!(skipped.len(), 1);

    let size = profile::upstream_image_size("youhua-pt1570-r10").unwrap();
    let bin = layout::build_raw(size, &kept).unwrap();
    assert_eq!(bin.len(), R10_64K);
    assert!(
        bin[0..0x1000].iter().all(|&b| b == 0xff),
        "前 0x1000 字节应全为 0xFF（无头部）"
    );
    assert_eq!(&bin[0x2000..0x2000 + 284], &[0x5au8; 284][..]);
    assert_eq!(&bin[0x3000..0x3000 + 284], &[0x5bu8; 284][..]);
    assert!(
        bin[0x7000..0x8000].iter().all(|&b| b == 0xff),
        "0x7000（NV_CFG 原档槽）必须保持空白 0xFF"
    );
}
