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

pub fn profile(name: &str) -> Result<fn(&EnvSet, &UbiImage) -> Result<Extracted, String>, String> {
    match name {
        "tyfza1" => Ok(Tyfza1::extract),
        "pt1500-h10" => Ok(Pt1500H10::extract),
        "pt1570" => Ok(Pt1570::extract),
        "pt1570-r10" => Ok(Pt1570R10::extract),
        other => Err(format!("unknown model: {other}")),
    }
}

pub fn model_id(name: &str) -> Result<u32, String> {
    match name {
        "tyfza1" => Ok(MODEL_TYFZA1),
        "pt1500-h10" => Ok(MODEL_PT1500_H10),
        "pt1570" => Ok(MODEL_PT1570),
        "pt1570-r10" => Ok(MODEL_PT1570_R10),
        other => Err(format!("unknown model: {other}")),
    }
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
