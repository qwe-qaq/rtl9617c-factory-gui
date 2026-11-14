use crate::calib::{epon_llid_mac, flash_calibration, RADIO0_5G, RADIO1_2G};
use crate::env::{parse_mac, EnvSet};
use crate::layout::{
    encode_string, Section, IMAGE_SIZE, MODEL_PT1500_H10, MODEL_PT1570, MODEL_PT1570_R10,
    MODEL_TYFZA1,
};
use crate::ubi::{Image as UbiImage, Volume};
use crate::ubifs;
use crate::xml::{field, optional_field};
use std::collections::BTreeMap;

pub struct Identity {
    pub base_mac: Option<[u8; 6]>,
    pub mac_count: u32,
    pub epon_mac: Option<[u8; 6]>,
    pub pon_sn: Option<String>,
    pub device_sn: Option<String>,
}

pub struct Extracted {
    pub identity: Identity,
    pub wifi0: Option<Vec<u8>>,
    pub wifi1: Option<Vec<u8>>,
    pub bosa: Option<Vec<u8>>,
    pub bosa_olt: Option<Vec<u8>>,
    pub xtemp: Option<Vec<u8>>,
    pub fpga: Option<Vec<u8>>,
}

pub trait DeviceProfile {
    fn extract(env: &EnvSet, device: &UbiImage) -> Result<Extracted, String>;
}

fn flat_lebs(volume: &Volume) -> Vec<u8> {
    volume.lebs.concat()
}

pub struct Tyfza1;

impl DeviceProfile for Tyfza1 {
    fn extract(_env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
        let config = device.volume("ubi_Config")?;
        let lebs: BTreeMap<u32, Vec<u8>> = config
            .lebs
            .iter()
            .enumerate()
            .map(|(i, data)| (i as u32, data.clone()))
            .collect();
        let files = ubifs::extract(&lebs)?;
        let hs = read_text(&files, "config_hs.xml")?;
        let cs = read_text(&files, "config.xml")?;
        let base_mac = parse_mac(&field(&hs, "ELAN_MAC_ADDR")?)?;
        let pon_sn = optional_field(&hs, "GPON_SN").filter(|v| !v.is_empty());
        let epon_mac = match epon_llid_mac(&cs) {
            Some(value) => Some(parse_mac(&value)?),
            None => None,
        };
        let fpga = match device.volumes.get("ubi_fpga") {
            Some(volume) => Some(flat_lebs(volume)),
            None => None,
        };
        Ok(Extracted {
            identity: Identity {
                base_mac: Some(base_mac),
                mac_count: 3,
                epon_mac,
                pon_sn,
                device_sn: None,
            },
            wifi0: Some(flash_calibration(&hs, &RADIO0_5G, base_mac)?),
            wifi1: Some(flash_calibration(&hs, &RADIO1_2G, base_mac)?),
            bosa: files.get("rtkbosa_k.bin").cloned(),
            bosa_olt: files.get("rtkoltbosa_k.bin").cloned(),
            xtemp: files.get("xtemp.cal").cloned(),
            fpga,
        })
    }
}

pub struct Pt1500H10;

impl DeviceProfile for Pt1500H10 {
    fn extract(env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
        device.volume("ubi_Config")?;
        let required = ["kp_ucaBaseMacAddr", "kp_ulNumMacAddrs", "kp_szXponSN"];
        for key in required {
            if env.get(key).is_none() {
                return Err(format!("keyparam {key} is missing from the environment"));
            }
        }
        let base_mac = parse_mac(env.get("kp_ucaBaseMacAddr").unwrap())?;
        let mac_count = env
            .get("kp_ulNumMacAddrs")
            .unwrap()
            .trim()
            .parse::<u32>()
            .map_err(|e| format!("invalid kp_ulNumMacAddrs: {e}"))?;
        Ok(Extracted {
            identity: Identity {
                base_mac: Some(base_mac),
                mac_count,
                epon_mac: Some(base_mac),
                pon_sn: env.get("kp_szXponSN").map(|v| v.to_string()),
                device_sn: env.get("kp_szSerialNumber").map(|v| v.to_string()),
            },
            wifi0: None,
            wifi1: None,
            bosa: None,
            bosa_olt: None,
            xtemp: None,
            fpga: None,
        })
    }
}

/// 友华 PT1570-H10：与 PT1500-H10 同为 env keyparam 机型（实测备份卷表中只有
/// ubi_Config / ubi_otherConfig / ubi_BakConfig / 内核 rootfs 等卷，无 ubi_fpga，
/// 也没有 config_hs.xml / BOSA / xtemp 校准文件），身份字段来源一致，且整机 SN
/// 来自 `kp_szSerialNumber`。单独建型号便于日后补充该校准或新字段。
pub struct Pt1570;

impl DeviceProfile for Pt1570 {
    fn extract(env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
        Pt1500H10::extract(env, device)
    }
}

/// 友华 PT1570-R10：新一代固件（booti + lzma 内核，env 无任何 `kp_*`），产线
/// 数据回归 `config_hs.xml` / `config.xml`（ubi_Config 卷，UBIFS / LZO）。实测
/// 字段集与 tyfza1 完全一致——含 TYFC0003 组装所需的全部 TSSI GAIN/CW DIFF
/// 字段；无 `rtkbosa_k.bin` / `xtemp.cal`（PON 校准在光模块 EEPROM）、无
/// `ubi_fpga` 卷；`EPON_LLID_TBL` 首条 macAddr = BASE_MAC（实测 8 条派生链）。
/// 提取逻辑与 [`Tyfza1`] 完全相同；wlan0=5G=base+2 沿用天邑规则（推定，
/// FEM_TYPE 组合 238/17 与 tyfza1 一致，待实机核对）。
pub struct Pt1570R10;

impl DeviceProfile for Pt1570R10 {
    fn extract(env: &EnvSet, device: &UbiImage) -> Result<Extracted, String> {
        Tyfza1::extract(env, device)
    }
}

fn read_text(files: &ubifs::Files, name: &str) -> Result<String, String> {
    let data = files
        .get(name)
        .ok_or_else(|| format!("{name} is missing from ubi_Config"))?;
    String::from_utf8(data.clone()).map_err(|_| format!("{name} is not valid UTF-8"))
}

/// 机型表：本仓短名与上游 2026-11-13 之后的长机型名一一对应。
/// 上游只定义了前两个机型；后两个是本仓扩展，别名按上游命名规则给出，
/// `upstream_image_size` 按同类机型推导（env 机型 64 KiB、XML 机型 8 MiB）。
pub struct ModelSpec {
    pub key: &'static str,
    pub upstream_name: &'static str,
    pub model_id: u32,
    pub rtl1_image_size: usize,
    pub upstream_image_size: usize,
    pub extract: fn(&EnvSet, &UbiImage) -> Result<Extracted, String>,
}

pub const MODEL_SPECS: &[ModelSpec] = &[
    ModelSpec {
        key: "tyfza1",
        upstream_name: "tianyi-tyfza1-2p5g",
        model_id: MODEL_TYFZA1,
        rtl1_image_size: IMAGE_SIZE,
        upstream_image_size: 0x0080_0000,
        extract: Tyfza1::extract,
    },
    ModelSpec {
        key: "pt1500-h10",
        upstream_name: "youhua-pt1500-h10-ct-2p5g",
        model_id: MODEL_PT1500_H10,
        rtl1_image_size: IMAGE_SIZE,
        upstream_image_size: 0x0001_0000,
        extract: Pt1500H10::extract,
    },
    ModelSpec {
        key: "pt1570",
        upstream_name: "youhua-pt1570-h10",
        model_id: MODEL_PT1570,
        rtl1_image_size: IMAGE_SIZE,
        upstream_image_size: 0x0001_0000,
        extract: Pt1570::extract,
    },
    ModelSpec {
        key: "pt1570-r10",
        upstream_name: "youhua-pt1570-r10",
        model_id: MODEL_PT1570_R10,
        rtl1_image_size: IMAGE_SIZE,
        upstream_image_size: 0x0080_0000,
        extract: Pt1570R10::extract,
    },
];

pub fn model_spec(name: &str) -> Result<&'static ModelSpec, String> {
    MODEL_SPECS
        .iter()
        .find(|m| m.key == name || m.upstream_name == name)
        .ok_or_else(|| format!("unknown model: {name}"))
}

/// 接受本仓短名或上游长名，统一返回本仓短名（未知机型原样报错）。
pub fn canonical(name: &str) -> Result<&'static str, String> {
    model_spec(name).map(|m| m.key)
}

pub fn profile(name: &str) -> Result<fn(&EnvSet, &UbiImage) -> Result<Extracted, String>, String> {
    model_spec(name).map(|m| m.extract)
}

pub fn model_id(name: &str) -> Result<u32, String> {
    model_spec(name).map(|m| m.model_id)
}

/// 上游 11-13 之后的镜像大小（按机型：8 MiB / 64 KiB）。
pub fn upstream_image_size(name: &str) -> Result<usize, String> {
    model_spec(name).map(|m| m.upstream_image_size)
}

/// 本仓 RTLFACT1 格式的镜像大小（统一 8 MiB）。
pub fn rtl1_image_size(name: &str) -> Result<usize, String> {
    model_spec(name).map(|m| m.rtl1_image_size)
}

pub fn sections(extracted: &Extracted) -> Result<Vec<Section>, String> {
    let mut out = Vec::new();
    let i = &extracted.identity;
    if let Some(mac) = i.base_mac {
        let mut data = mac.to_vec();
        data.extend_from_slice(&i.mac_count.to_le_bytes());
        out.push(Section { tag: 0x0001, data });
    }
    if let Some(mac) = i.epon_mac {
        out.push(Section { tag: 0x0002, data: mac.to_vec() });
    }
    for (tag, value) in [(0x0010, &i.pon_sn), (0x0011, &i.device_sn)] {
        if let Some(text) = value {
            let spec = crate::layout::spec(tag)?;
            out.push(Section {
                tag,
                data: encode_string(text, spec.capacity as usize)?,
            });
        }
    }
    for (tag, data) in [
        (0x0100, &extracted.wifi0),
        (0x0101, &extracted.wifi1),
        (0x0110, &extracted.bosa),
        (0x0111, &extracted.bosa_olt),
        (0x0112, &extracted.xtemp),
    ] {
        if let Some(payload) = data {
            out.push(Section {
                tag,
                data: payload.clone(),
            });
        }
    }
    if let Some(fpga) = &extracted.fpga {
        if fpga.len() > IMAGE_SIZE {
            return Err("FPGA image exceeds the factory image size".into());
        }
        out.push(Section { tag: 0x0200, data: fpga.clone() });
    }
    Ok(out)
}
