//! 上游 CLI 的原样移植：与 `rtl9617c-factory` 仓库 main.rs 逐行一致，
//! 仅把模块引用改为外部 lib（`rtl9617c_factory::…`），行为不变。

use std::process::ExitCode;

use rtl9617c_factory::{calib, env, layout, profile, ubi};

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn arg_value(args: &[String], key: &str) -> Result<String, String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .ok_or_else(|| format!("missing required argument {key} <file>"))
}

fn load_env(args: &[String]) -> Result<env::EnvSet, String> {
    let primary = read_file(&arg_value(args, "--env")?)?;
    let secondary = match args.iter().position(|a| a == "--env2") {
        Some(i) => Some(read_file(args.get(i + 1).ok_or("--env2 requires a file")?)?),
        None => None,
    };
    let mut parts = vec![primary];
    if let Some(extra) = secondary {
        parts.push(extra);
    }
    let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
    env::load(&refs)
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(|s| s.as_str()) {
        Some("build") => {
            let model = arg_value(args, "--model")?;
            let output = arg_value(args, "-o")?;
            let compact = args.iter().any(|a| a == "--compact");
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let mut extracted = profile::profile(&model)?(&env, &image)?;
            // 可选：pt1570 mtd3 的 NV_CFG —— 原样存档（0x0103）并组装 Wi-Fi 校准
            // （5G=wlan1=base+2、2G=wlan0=base+1，实机日志证据；DIFF 缺失补 0）
            let mut nvcfg_blob: Option<Vec<u8>> = None;
            if let Some(i) = args.iter().position(|a| a == "--wifi-nvcfg") {
                let path = args.get(i + 1).ok_or("--wifi-nvcfg requires a file")?;
                let data = read_file(path)?;
                let blob = calib::normalize_nv_cfg(&data)?;
                calib::validate_nv_cfg(&blob)?;
                nvcfg_blob = Some(blob);
            }
            if model == "pt1570" {
                if let Some(blob) = &nvcfg_blob {
                    let text = String::from_utf8(blob.clone())
                        .map_err(|_| "NV_CFG is not ASCII text".to_string())?;
                    let base = extracted
                        .identity
                        .base_mac
                        .ok_or("NV_CFG assembly requires BASE_MAC")?;
                    extracted.wifi0 =
                        Some(calib::flash_calibration_nv(&text, &calib::RADIO1_5G, base)?);
                    extracted.wifi1 =
                        Some(calib::flash_calibration_nv(&text, &calib::RADIO0_2G, base)?);
                }
            }
            let mut sections = profile::sections(&extracted)?;
            // 可选：光模块 A2h EEPROM 转储（env 机型 PON 校准不在 flash 分区里）
            if let Some(i) = args.iter().position(|a| a == "--pon-eeprom") {
                let path = args.get(i + 1).ok_or("--pon-eeprom requires a file")?;
                let data = read_file(path)?;
                calib::validate_a2h_dump(&data)?;
                sections.push(layout::Section { tag: 0x0102, data });
            }
            if let Some(blob) = nvcfg_blob {
                sections.push(layout::Section { tag: 0x0103, data: blob });
            }
            let generation = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let model_id = profile::model_id(&model)?;
            let (bin, entries) = if compact {
                // 紧凑镜像（64 KiB，不含 FPGA）无法走 summary() 的 8 MiB 校验，
                // 段表直接从段数据生成
                let bin = layout::build_compact(model_id, generation, &sections)?;
                let entries = layout::summarize_sections(&sections)?;
                (bin, entries)
            } else {
                let bin = layout::build(model_id, generation, &sections)?;
                let entries = layout::summary(&bin)?;
                (bin, entries)
            };
            std::fs::write(&output, &bin)
                .map_err(|e| format!("cannot write {output}: {e}"))?;
            let mut report = format!(
                "factory image: {} ({} bytes{}) -> {}\n",
                model,
                bin.len(),
                if compact { ", compact (no FPGA)" } else { "" },
                output
            );
            for entry in entries {
                report.push_str(&format!(
                    "  0x{:06x}  {:<22} size={:<8} crc=0x{:08x}\n",
                    entry.offset, entry.name, entry.size, entry.crc
                ));
            }
            Ok(report)
        }
        Some("dump") => {
            let model = arg_value(args, "--model")?;
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let extracted = profile::profile(&model)?(&env, &image)?;
            let mut report = format!("model: {} (id {})\n", model, profile::model_id(&model)?);
            report.push_str(&format!(
                "ubi: peb={} leb={} volumes={:?}\n",
                image.peb_size,
                image.leb_size,
                image.volumes.keys().collect::<Vec<_>>()
            ));
            let i = &extracted.identity;
            let mac = |m: &Option<[u8; 6]>| match m {
                Some(a) => format!(
                    "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    a[0], a[1], a[2], a[3], a[4], a[5]
                ),
                None => "-".into(),
            };
            report.push_str(&format!(
                "base_mac={} count={} epon_mac={}\n",
                mac(&i.base_mac),
                i.mac_count,
                mac(&i.epon_mac)
            ));
            for (name, value) in [("pon_sn", &i.pon_sn), ("device_sn", &i.device_sn)] {
                report.push_str(&format!("  {name}: {}\n", value.as_deref().unwrap_or("-")));
            }
            for (name, data) in [
                ("wifi0", &extracted.wifi0),
                ("wifi1", &extracted.wifi1),
                ("bosa", &extracted.bosa),
                ("bosa_olt", &extracted.bosa_olt),
                ("xtemp", &extracted.xtemp),
                ("fpga", &extracted.fpga),
            ] {
                report.push_str(&format!(
                    "  {}: {} bytes\n",
                    name,
                    data.as_ref().map_or(0, |d| d.len())
                ));
            }
            let sections = profile::sections(&extracted)?;
            report.push_str(&format!("sections: {}\n", sections.len()));
            Ok(report)
        }
        _ => Err("usage: rtl9617c-factory <build|dump> --model <tyfza1|pt1500-h10> --env <file> [--env2 <file>] --device <file> [-o <out.bin>]".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
