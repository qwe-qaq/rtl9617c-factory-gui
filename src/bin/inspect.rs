//! 备份分析工具：打印 MAC / SN / 校准数据在备份中的**实际值与出处**——
//! U-Boot env 变量、UBI 卷列表、ubi_Config 内 UBIFS 文件、config_hs.xml /
//! config.xml 产线字段、校准文件内容，以及 tyfza1 提取结果与 Wi-Fi 校准块
//! 内嵌 MAC 的定位。
//!
//! 用法：rtl9617c-factory-inspect <mtd1_env.bin> <mtd2_env2.bin> <mtd4_ubi_device.bin> [model]
//! 其中 model 缺省为 tyfza1，可选 tyfza1 / pt1500-h10 / pt1570 / pt1570-r10。
use rtl9617c_factory::{calib, env, profile, ubi, ubifs, xml};
use std::collections::BTreeMap;

fn mac(m: &[u8]) -> String {
    format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", m[0], m[1], m[2], m[3], m[4], m[5])
}

fn hex16(data: &[u8]) -> String {
    data.iter().take(16).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 与 calib.rs 相同的派生规则：MAC 视为大端整数后加 offset。
fn derived_mac(base: [u8; 6], offset: u64) -> [u8; 6] {
    let mut v = 0u64;
    for b in base {
        v = (v << 8) | u64::from(b);
    }
    v += offset;
    [
        ((v >> 40) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    ]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let e1 = std::fs::read(&args[0]).expect("env");
    let e2 = std::fs::read(&args[1]).expect("env2");
    let dev = std::fs::read(&args[2]).expect("device");

    println!("================ U-Boot env ================");
    for (name, data) in [("mtd1_env.bin", &e1), ("mtd2_env2.bin", &e2)] {
        match env::load(&[data.as_slice()]) {
            Ok(set) => println!("[{}] 有效，{} 个变量", name, set.variables.len()),
            Err(e) => println!("[{}] 无效：{}", name, e),
        }
    }
    let set = env::load(&[&e1, &e2]).expect("env load");
    println!("合并采用（flags 大者优先）共 {} 个变量：", set.variables.len());
    for (k, v) in &set.variables {
        println!("  {}={}", k, v);
    }

    println!("================ UBI 容器 (mtd4_ubi_device.bin) ================");
    let image = ubi::parse(&dev).expect("ubi");
    println!("peb={} leb={}", image.peb_size, image.leb_size);
    for (name, vol) in &image.volumes {
        println!(
            "  卷 {:<14} {:>3} LEB x {} = {:>10} 字节",
            name,
            vol.lebs.len(),
            image.leb_size,
            vol.lebs.len() * image.leb_size
        );
    }

    println!("================ ubi_Config 卷内 UBIFS 文件 ================");
    let config = image.volume("ubi_Config").expect("ubi_Config");
    let lebs: BTreeMap<u32, Vec<u8>> = config
        .lebs
        .iter()
        .enumerate()
        .map(|(i, d)| (i as u32, d.clone()))
        .collect();
    let files = ubifs::extract(&lebs).expect("ubifs");
    for (name, data) in &files {
        println!("  {:<24} {:>8} 字节", name, data.len());
    }

    let hs = files
        .get("config_hs.xml")
        .and_then(|d| String::from_utf8(d.clone()).ok());
    let cs = files
        .get("config.xml")
        .and_then(|d| String::from_utf8(d.clone()).ok());

    if let Some(hs) = &hs {
        println!("================ config_hs.xml 身份字段 ================");
        for key in ["ELAN_MAC_ADDR", "GPON_SN"] {
            match xml::optional_field(hs, key) {
                Some(v) => println!("  {:<16} = {:?}", key, v),
                None => println!("  {:<16} = <无>", key),
            }
        }

        println!("================ config_hs.xml Wi-Fi 校准字段（组装进 TYFC0003）================");
        for line in hs.lines().filter(|l| l.contains("Name=\"HW_")) {
            println!("  {}", line.trim());
        }
    } else {
        println!("================ config_hs.xml：本备份 ubi_Config 中无此文件（env 机型）================");
    }

    if let Some(cs) = &cs {
        println!("================ config.xml EPON ================");
        println!("  EPON_LLID_TBL.macAddr = {:?}", calib::epon_llid_mac(cs));
        if let Some(p) = cs.find("EPON_LLID_TBL") {
            let seg = &cs[p..(p + 600).min(cs.len())];
            for line in seg.lines().filter(|l| l.contains("macAddr")) {
                println!("  原文: {}", line.trim());
                break;
            }
        }
    }

    println!("================ 校准 / 固件文件内容 ================");
    if let Some(b) = files.get("rtkbosa_k.bin") {
        println!("  rtkbosa_k.bin     {} 字节  头: {}", b.len(), hex16(b));
    }
    if let Some(b) = files.get("rtkoltbosa_k.bin") {
        println!("  rtkoltbosa_k.bin  {} 字节  头: {}", b.len(), hex16(b));
    }
    if let Some(b) = files.get("xtemp.cal") {
        let text = String::from_utf8_lossy(&b[..b.len().min(300)]);
        println!("  xtemp.cal         {} 字节  内容: {}", b.len(), text.trim_end());
    }
    if let Some(vol) = image.volumes.get("ubi_fpga") {
        let flat: Vec<u8> = vol.lebs.concat();
        println!(
            "  ubi_fpga 卷       {} 字节  头: {} ({:?})",
            flat.len(),
            hex16(&flat),
            String::from_utf8_lossy(&flat[..4.min(flat.len())])
        );
    }

    let model = args.get(3).map(|s| s.as_str()).unwrap_or("tyfza1");
    println!("================ {model} 提取结果（等价 CLI dump）================");
    let extracted = profile::profile(model)
        .expect("unknown model")(&set, &image)
        .expect("extract");
    let i = &extracted.identity;
    println!(
        "  BASE_MAC  = {}  (写入 factory.bin 后 MAC 数量 {})",
        i.base_mac.map(|m| mac(&m)).unwrap_or("-".into()),
        i.mac_count
    );
    println!("  EPON_MAC  = {}", i.epon_mac.map(|m| mac(&m)).unwrap_or("-".into()));
    println!("  PON_SN    = {}", i.pon_sn.as_deref().unwrap_or("-"));
    if let Some(sn) = &i.device_sn {
        println!("  DEVICE_SN = {sn}");
    } else if model == "tyfza1" || model == "pt1570-r10" {
        println!("  DEVICE_SN = -（无产线数据源）");
    } else {
        println!("  DEVICE_SN = -");
    }
    if model == "tyfza1" || model == "pt1570-r10" {
        let base = i.base_mac.expect("base mac");
        for (label, blob, off) in [
            ("wifi0 (5G, TYFC0003)", &extracted.wifi0, 2u64),
            ("wifi1 (2G, TYFC0003)", &extracted.wifi1, 1u64),
        ] {
            match blob {
                Some(d) => {
                    let em = derived_mac(base, off);
                    let at = find_sub(d, &em);
                    println!("  {}  {} 字节  头: {}", label, d.len(), hex16(d));
                    println!(
                        "    内嵌 MAC {} (base{}) 位于 blob 偏移 {}",
                        mac(&em),
                        if off == 2 { "+2" } else { "+1" },
                        at.map(|p| format!("0x{p:04X}"))
                            .unwrap_or_else(|| "<未找到>".into())
                    );
                }
                None => println!("  {}  <空>", label),
            }
        }
    } else {
        println!("  Wi-Fi / PON 校准：env 机型无校准数据段（与 CLI dump 一致为空）");
    }
}
