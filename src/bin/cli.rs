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
            let model_input = arg_value(args, "--model")?;
            // 同时接受本仓短名与上游 11-13 之后的长机型名。
            let model = profile::canonical(&model_input)?;
            let output = arg_value(args, "-o")?;
            let format = match args.iter().position(|a| a == "--format") {
                Some(i) => layout::OutputFormat::parse(
                    args.get(i + 1).ok_or("--format requires <rtl1|raw>")?,
                )?,
                // 默认导出上游 raw 格式（与 OutputFormat::default() 保持一致）。
                None => layout::OutputFormat::default(),
            };
            let compact = args.iter().any(|a| a == "--compact");
            // raw 格式严格对齐上游：固定 10 个字段、镜像大小由机型决定，不支持
            // 紧凑镜像。--wifi-nvcfg 仍可传入：NV_CFG 用于组装
            // Wi-Fi 校准段(0x0100/0x0101，属于上游字段表)，只是原档不写入镜像。
            if format == layout::OutputFormat::UpstreamRaw && compact {
                return Err("--compact 仅适用于 --format rtl1；raw 格式的镜像大小由机型决定".into());
            }
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let mut extracted = profile::profile(model)?(&env, &image)?;
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
                    // 魔数含非 ASCII 字节；字段查找只碰 ASCII 的 KEY=VALUE 区
                    let text = String::from_utf8_lossy(blob).to_string();
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
            if let Some(blob) = nvcfg_blob {
                sections.push(layout::Section { tag: 0x0103, data: blob });
            }
            // raw 模式只剔除「原样存档」段（0x0103）。NV_CFG 组装出的
            // Wi-Fi 校准段(0x0100/0x0101)是上游固定字段，必须保留并写入镜像。
            let skipped_sections = if format == layout::OutputFormat::UpstreamRaw {
                let (kept, skipped) = layout::filter_raw_sections(&sections);
                sections = kept;
                skipped
            } else {
                Vec::new()
            };
            let generation = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let model_id = profile::model_id(model)?;
            let (bin, entries) = match (format, compact) {
                // 上游 11-13 之后的纯固定偏移格式：大小按机型、无头无校验。
                (layout::OutputFormat::UpstreamRaw, _) => {
                    let size = profile::upstream_image_size(model)?;
                    let bin = layout::build_raw(size, &sections)?;
                    let entries = layout::summarize_raw(&sections)?;
                    (bin, entries)
                }
                // 本仓紧凑镜像（64 KiB，不含 FPGA）无法走 summary() 的 8 MiB 校验，
                // 段表直接从段数据生成。
                (layout::OutputFormat::RtlFact1, true) => {
                    let bin = layout::build_compact(model_id, generation, &sections)?;
                    let entries = layout::summarize_sections(&sections)?;
                    (bin, entries)
                }
                (layout::OutputFormat::RtlFact1, false) => {
                    let bin = layout::build(model_id, generation, &sections)?;
                    let entries = layout::summary(&bin)?;
                    (bin, entries)
                }
            };
            std::fs::write(&output, &bin)
                .map_err(|e| format!("cannot write {output}: {e}"))?;
            let mut report = format!(
                "factory image: {} ({} bytes, format={}{}) -> {}\n",
                model_input,
                bin.len(),
                format.as_str(),
                if compact { ", compact (no FPGA)" } else { "" },
                output
            );
            for entry in entries {
                report.push_str(&format!(
                    "  0x{:06x}  {:<22} size={:<8} crc=0x{:08x}\n",
                    entry.offset, entry.name, entry.size, entry.crc
                ));
            }
            if !skipped_sections.is_empty() {
                report.push_str(
                    "note: archive-only sections dropped (upstream layout has no slot; \
                     NV_CFG-derived Wi-Fi calibration was written as usual):\n",
                );
                for s in &skipped_sections {
                    report.push_str(&format!("  - {s}\n"));
                }
            }
            Ok(report)
        }
        Some("dump") => {
            let model_input = arg_value(args, "--model")?;
            let model = profile::canonical(&model_input)?;
            let env = load_env(args)?;
            let device = read_file(&arg_value(args, "--device")?)?;
            let image = ubi::parse(&device)?;
            let extracted = profile::profile(model)?(&env, &image)?;
            let mut report = format!("model: {} (id {})\n", model_input, profile::model_id(model)?);
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
        _ => Err("usage: rtl9617c-factory <build|dump> --model <tyfza1|pt1500-h10|pt1570|pt1570-r10|tianyi-tyfza1-2p5g|youhua-pt1500-h10-ct-2p5g|youhua-pt1570-h10|youhua-pt1570-r10> --env <file> [--env2 <file>] --device <file> [-o <out.bin>] [--format <rtl1|raw>] [--compact] [--wifi-nvcfg <file>] (default: raw)".into()),
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
