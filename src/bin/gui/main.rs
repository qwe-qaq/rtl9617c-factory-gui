#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! RTL9617C Factory 图形工具 —— 上游 `rtl9617c-factory` CLI 的 egui 图形版。
//!
//! 界面模式参考 `h3c-factory`（同作者的 fiberhome-factory 系列）：顶栏 + 状态行
//! + 分组面板 + 亮色主题 + 系统 CJK 字体。核心提取/组装逻辑**原封不动**复用上游
//! lib（env / ubi / ubifs / xml / calib / profile / layout），因此 GUI 生成的
//! factory.bin 与 CLI `build` 产物一致（仅 0x18 处的 generation 时间戳不同）。
//!
//! 工作流（对应 CLI 的 `dump` / `build` 两个子命令）：
//!   1. 选机型 → 选 U-Boot env（可选 env2）+ ubi_device 镜像（支持拖入窗口）
//!   2. 后台线程提取身份数据与校准数据（= `dump`）
//!   3. 「生成 factory.bin…」（= `build`），输出 8 MiB RTLFACT1 镜像并显示段表
//!
//! Build: `cargo build --release`
//! Run:   `cargo run --release --bin rtl9617c-factory-gui`

mod font;

use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::mpsc;

use rtl9617c_factory::{calib, env as envmod, layout, profile, ubi};

/// 当前机型（与 CLI `--model` 一一对应）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Model {
    /// 天邑 TYFZA1：Wi-Fi + 下行光 + FPGA，数据在 ubi_Config 卷内。
    Tyfza1,
    /// 友华 PT1500-H10：无 Wi-Fi / 无下行光，数据在 U-Boot env `kp_*`。
    Pt1500H10,
    /// 友华 PT1570-H10：env keyparam 机型（含整机 SN `kp_szSerialNumber`），
    /// 备份卷表中无 ubi_fpga 与校准文件，Wi-Fi/BOSA 段为空。
    Pt1570,
    /// 友华 PT1570-R10：新一代固件（env 无 kp_*），数据回归 ubi_Config 卷的
    /// config_hs.xml / config.xml（同 tyfza1），含完整 Wi-Fi 校准字段；
    /// 无 BOSA/xtemp 文件、无 ubi_fpga 卷。
    Pt1570R10,
}

impl Model {
    fn all() -> [Model; 4] {
        [Model::Tyfza1, Model::Pt1500H10, Model::Pt1570, Model::Pt1570R10]
    }
    fn key(self) -> &'static str {
        match self {
            Model::Tyfza1 => "tyfza1",
            Model::Pt1500H10 => "pt1500-h10",
            Model::Pt1570 => "pt1570",
            Model::Pt1570R10 => "pt1570-r10",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Model::Tyfza1 => "天邑 TYFZA1",
            Model::Pt1500H10 => "友华 PT1500-H10",
            Model::Pt1570 => "友华 PT1570-H10",
            Model::Pt1570R10 => "友华 PT1570-R10",
        }
    }
    fn id(self) -> u32 {
        match self {
            Model::Tyfza1 => layout::MODEL_TYFZA1,
            Model::Pt1500H10 => layout::MODEL_PT1500_H10,
            Model::Pt1570 => layout::MODEL_PT1570,
            Model::Pt1570R10 => layout::MODEL_PT1570_R10,
        }
    }
    /// env 分区对本机型是否必需（tyfza1 的数据全部来自 ubi_device）。
    fn needs_env(self) -> bool {
        matches!(self, Model::Pt1500H10 | Model::Pt1570)
    }
}

/// 校准/镜像数据段在界面上的展示项（tag 与 layout::FIELDS 对应）。
struct SectionRow {
    tag: u16,
    /// factory.bin 里的字段名（与 CLI 段表一致）。
    name: &'static str,
    /// 界面主标题（中文）。
    title: &'static str,
    /// 来源与内容说明。
    note: &'static str,
    /// 「导出」按钮的默认文件名。
    file_name: &'static str,
}

const SECTION_ROWS: &[SectionRow] = &[
    SectionRow { tag: 0x0100, name: "WIFI_CAL_RADIO0", title: "5G Wi-Fi 校准", note: "HW_* 字段组装 TYFC0003 · 内嵌 MAC base+2", file_name: "wifi-cal-radio0.bin" },
    SectionRow { tag: 0x0101, name: "WIFI_CAL_RADIO1", title: "2.4G Wi-Fi 校准", note: "HW_* 字段组装 TYFC0003 · 内嵌 MAC base+1", file_name: "wifi-cal-radio1.bin" },
    SectionRow { tag: 0x0110, name: "BOSA_CALIBRATION", title: "上行 PON 校准", note: "rtkbosa_k.bin · PON 上行口 BOSA 光模块", file_name: "rtkbosa_k.bin" },
    SectionRow { tag: 0x0111, name: "BOSA_OLT_CALIBRATION", title: "下行 PON 校准", note: "rtkoltbosa_k.bin · 下行 OLT 光口 BOSA", file_name: "rtkoltbosa_k.bin" },
    SectionRow { tag: 0x0112, name: "XTEMP_CALIBRATION", title: "XTEMP 校准", note: "xtemp.cal · JSON 文本", file_name: "xtemp.cal" },
    SectionRow { tag: 0x0200, name: "FPGA_IMAGE", title: "FPGA 固件", note: "ubi_fpga 卷原始内容 · fpgafs squashfs", file_name: "ubi_fpga.img" },
];

fn section_data<'a>(e: &'a profile::Extracted, tag: u16) -> Option<&'a Vec<u8>> {
    match tag {
        0x0100 => e.wifi0.as_ref(),
        0x0101 => e.wifi1.as_ref(),
        0x0110 => e.bosa.as_ref(),
        0x0111 => e.bosa_olt.as_ref(),
        0x0112 => e.xtemp.as_ref(),
        0x0200 => e.fpga.as_ref(),
        _ => None,
    }
}

fn mac_to_string(m: &[u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", m[0], m[1], m[2], m[3], m[4], m[5])
}

/// 与 `calib::flash_calibration` 相同的派生规则：MAC 视为大端整数后加 offset。
/// 5G Wi-Fi 校准块内嵌 base+2，2.4G 内嵌 base+1。
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

/// 提取成功的完整结果（对应 CLI `dump` 的输出内容）。
struct Extraction {
    ubi_peb: usize,
    ubi_leb: usize,
    volumes: Vec<String>,
    extracted: profile::Extracted,
}

/// 「生成 factory.bin」的结果：段表 + 保存路径。
struct BuildOut {
    entries: Vec<layout::SummaryEntry>,
    saved_to: Option<PathBuf>,
}

struct App {
    model: Model,
    env_path: Option<PathBuf>,
    env2_path: Option<PathBuf>,
    device_path: Option<PathBuf>,
    /// 用户提供的光模块 A2h EEPROM 转储（factory.bin tag 0x0102，可选）。
    /// env 机型的 PON 口校准在光模块内，只能从原厂固件 i2cdump 获得。
    pon_eeprom: Option<Vec<u8>>,
    /// 用户提供的 mtd3 NV_CFG 转储（factory.bin tag 0x0103，可选）。
    /// pt1570 的 Wi-Fi 校准字段在其中，因缺 DIFF 字段不组装、原样存档。
    wifi_nvcfg: Option<Vec<u8>>,
    /// 提取结果（None = 尚未提取）。
    result: Option<Result<Extraction, String>>,
    build_out: Option<Result<BuildOut, String>>,
    status: String,
    /// 后台提取任务的结果通道；Some 即任务在跑。
    rx: Option<mpsc::Receiver<Result<Extraction, String>>>,
    busy: bool,
}

impl App {
    fn new() -> App {
        App {
            model: Model::Tyfza1,
            env_path: None,
            env2_path: None,
            device_path: None,
            pon_eeprom: None,
            wifi_nvcfg: None,
            result: None,
            build_out: None,
            status: "选择机型与数据来源（env / ubi_device），或直接把备份文件拖进窗口"
                .to_string(),
            rx: None,
            busy: false,
        }
    }

    // ---- 数据加载 -----------------------------------------------------------

    /// 在后台线程读取文件并执行提取（等价于 CLI 的 dump 流程）。
    /// 大镜像（mtd4 约 250 MB）的读取与解析都不占用 UI 线程。
    fn start_extract(&mut self, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        let Some(device) = self.device_path.clone() else {
            self.status = "请先选择 ubi_device 镜像（mtd4_ubi_device.bin）".into();
            return;
        };
        let env = self.env_path.clone();
        let env2 = self.env2_path.clone();
        let nv = self.wifi_nvcfg.clone();
        let model = self.model;
        if model.needs_env() && env.is_none() {
            self.status = "该机型需要 U-Boot env 分区（mtd1_env.bin）".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.busy = true;
        self.rx = Some(rx);
        self.result = None;
        self.build_out = None;
        self.status =
            format!("正在解析（env / UBI 容器 / UBIFS），机型 {} …", model.label());
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let out = run_extract(model, env, env2, device, nv);
            let _ = tx.send(out);
            ctx2.request_repaint();
        });
    }

    /// 收取后台提取结果。
    fn poll_job(&mut self, ctx: &egui::Context) {
        if !self.busy {
            return;
        }
        let Some(rx) = &self.rx else {
            self.busy = false;
            return;
        };
        match rx.try_recv() {
            Ok(res) => {
                self.busy = false;
                self.rx = None;
                match &res {
                    Ok(e) => {
                        let n =
                            profile::sections(&e.extracted).map(|s| s.len()).unwrap_or(0);
                        self.status = format!(
                            "提取完成：{} 个 UBI 卷，{} 段将写入 factory.bin",
                            e.volumes.len(),
                            n
                        );
                    }
                    Err(err) => {
                        self.status = format!("提取失败：{err}");
                    }
                }
                self.result = Some(res);
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.status = "内部错误：提取线程意外退出".into();
            }
        }
    }

    /// 生成镜像（等价于 CLI 的 build 流程），弹出保存对话框。
    /// `compact=true` 时输出 64 KiB 紧凑镜像（布局/CRC 与完整镜像一致，不含 FPGA）。
    fn build_image(&mut self, compact: bool) {
        let Some(Ok(ext)) = self.result.as_ref() else {
            return;
        };
        let res = (|| -> Result<BuildOut, String> {
            let mut sections = profile::sections(&ext.extracted)?;
            if let Some(data) = &self.pon_eeprom {
                sections.push(layout::Section {
                    tag: 0x0102,
                    data: data.clone(),
                });
            }
            if let Some(data) = &self.wifi_nvcfg {
                sections.push(layout::Section {
                    tag: 0x0103,
                    data: data.clone(),
                });
            }
            let generation = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let (bin, entries) = if compact {
                let bin = layout::build_compact(self.model.id(), generation, &sections)?;
                let entries = layout::summarize_sections(&sections)?;
                (bin, entries)
            } else {
                let bin = layout::build(self.model.id(), generation, &sections)?;
                let entries = layout::summary(&bin)?;
                (bin, entries)
            };
            // 保存对话框被取消时不算错误：段表仍然展示，只是不落盘。
            let name = if compact {
                format!("factory-{}-compact.bin", self.model.key())
            } else {
                format!("factory-{}.bin", self.model.key())
            };
            let saved_to = FileDialog::new().set_file_name(name).save_file();
            if let Some(target) = &saved_to {
                std::fs::write(target, &bin).map_err(|e| format!("写入失败: {e}"))?;
            }
            Ok(BuildOut { entries, saved_to })
        })();
        let kind = if compact { "64 KiB 紧凑镜像（不含 FPGA）" } else { "8 MiB factory.bin" };
        self.build_out = Some(match res {
            Ok(out) => {
                self.status = match &out.saved_to {
                    Some(p) => format!("已生成 {} -> {}", kind, p.display()),
                    None => format!("已生成 {}（未保存，重新点生成可保存）", kind),
                };
                Ok(out)
            }
            Err(e) => {
                self.status = format!("生成失败：{e}");
                Err(e)
            }
        });
    }

    // ---- 界面 ---------------------------------------------------------------

    fn ui_topbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("RTL9617C Factory");
            ui.separator();
            for m in Model::all() {
                if ui.selectable_label(self.model == m, m.label()).clicked() && self.model != m {
                    self.model = m;
                    self.build_out = None;
                    self.status = format!("已切换机型：{}", m.label());
                    if self.device_path.is_some() {
                        let ctx = ui.ctx().clone();
                        self.start_extract(&ctx);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ready = self.result.as_ref().map_or(false, |r| r.is_ok()) && !self.busy;
                if ui
                    .add_enabled(ready, egui::Button::new("生成 factory.bin…"))
                    .clicked()
                {
                    self.build_image(false);
                }
                if ui
                    .add_enabled(ready, egui::Button::new("导出紧凑镜像 (64 KiB)…"))
                    .clicked()
                {
                    self.build_image(true);
                }
                if self.busy {
                    ui.spinner();
                }
            });
        });
    }

    fn ui_sources(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.strong("数据来源");
        ui.label(
            egui::RichText::new("支持把 mtd1_env.bin / mtd2_env2.bin / mtd4_ubi_device.bin 直接拖进窗口")
                .weak()
                .small(),
        );
        ui.label(
            egui::RichText::new(match self.model {
                Model::Tyfza1 => {
                    "tyfza1：身份与校准数据都在 ubi_device 的 ubi_Config / ubi_fpga 卷里，env 不参与"
                }
                Model::Pt1500H10 => "pt1500-h10：身份与 SN 来自 env 的 kp_* 产线变量",
                Model::Pt1570 => {
                    "pt1570：身份与 SN 来自 env 的 kp_* 产线变量；PON 校准在 EEPROM（不在分区），Wi-Fi 校准字段在 mtd3 NV_CFG"
                }
                Model::Pt1570R10 => {
                    "pt1570-r10：身份与 Wi-Fi 校准都在 ubi_Config 卷（config_hs.xml），仅 mtd4 必需；PON 校准在 EEPROM"
                }
            })
            .weak()
            .small(),
        );
        let mut changed = false;
        changed |= source_row(
            ui,
            "U-Boot env",
            &mut self.env_path,
            if self.model.needs_env() {
                "必需（pt1500-h10）"
            } else {
                "可省（tyfza1 不使用）"
            },
            false,
        );
        changed |= source_row(
            ui,
            "env2 备分区",
            &mut self.env2_path,
            "可选；多分区取 flags 更大者",
            true,
        );
        changed |= source_row(
            ui,
            "ubi_device 容器",
            &mut self.device_path,
            "必需（身份与校准数据来源）",
            true,
        );
        if changed {
            let ctx = ui.ctx().clone();
            self.start_extract(&ctx);
        }
    }

    /// 光模块 A2h EEPROM 转储行（用户自行从原厂固件 dump，可选载入）。
    fn ui_pon_eeprom(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong("PON 模块 EEPROM (A2h)");
                ui.weak("env 机型 PON 口校准所在 · 原厂固件 i2cdump 0x51 · 校验 0x5F");
                match &self.pon_eeprom {
                    Some(d) => ui.weak(format!("已载入 {} 字节", d.len())),
                    None => ui.weak("未提供"),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.pon_eeprom.is_some() && ui.button("清除").clicked() {
                        self.pon_eeprom = None;
                        self.status = "已清除 PON EEPROM 转储".into();
                    }
                    if let Some(d) = &self.pon_eeprom {
                        if ui.button("导出…").clicked() {
                            if let Some(p) = FileDialog::new()
                                .set_file_name("pon-a2h-dump.bin")
                                .save_file()
                            {
                                match std::fs::write(&p, d.as_slice()) {
                                    Ok(_) => {
                                        self.status =
                                            format!("已导出 {} 字节 -> {}", d.len(), p.display())
                                    }
                                    Err(e) => self.status = format!("导出失败: {e}"),
                                }
                            }
                        }
                    }
                    if ui.button("导入…").clicked() {
                        if let Some(p) = FileDialog::new()
                            .add_filter("EEPROM 转储", &["bin", "dump"])
                            .pick_file()
                        {
                            match std::fs::read(&p) {
                                Ok(data) => match calib::validate_a2h_dump(&data) {
                                    Ok(()) => {
                                        self.status = format!(
                                            "已导入 PON EEPROM 转储: {} 字节（校验和通过）",
                                            data.len()
                                        );
                                        self.pon_eeprom = Some(data);
                                    }
                                    Err(e) => self.status = format!("导入被拒: {e}"),
                                },
                                Err(e) => self.status = format!("读取失败: {e}"),
                            }
                        }
                    }
                });
            });
        });
    }

    /// mtd3 NV_CFG 转储行（pt1570 的 Wi-Fi 校准字段，原样存档，可选）。
    fn ui_wifi_nvcfg(&mut self, ui: &mut egui::Ui) {
        ui.add_space(2.0);
        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.strong("Wi-Fi NV_CFG（导入 mtd3-static_conf.bin）");
                ui.weak("pt1570 Wi-Fi 校准字段 · DIFF 按产线惯例补 0 组装 · 并存原档");
                let status = match &self.wifi_nvcfg {
                    Some(d) => {
                        let n = String::from_utf8_lossy(d)
                            .split('\0')
                            .filter(|s| s.contains('='))
                            .count();
                        let preview = String::from_utf8_lossy(&d[..d.len().min(600)]).to_string();
                        ui.weak(format!("已载入 {} 字节 · {} 个字段", d.len(), n))
                            .on_hover_text(preview)
                    }
                    None => ui.weak("未提供"),
                };
                drop(status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.wifi_nvcfg.is_some() && ui.button("清除").clicked() {
                        self.wifi_nvcfg = None;
                        self.status = "已清除 Wi-Fi NV_CFG 转储".into();
                    }
                    if let Some(d) = &self.wifi_nvcfg {
                        if ui.button("导出…").clicked() {
                            if let Some(p) = FileDialog::new()
                                .set_file_name("wifi-nvcfg.bin")
                                .save_file()
                            {
                                match std::fs::write(&p, d.as_slice()) {
                                    Ok(_) => {
                                        self.status =
                                            format!("已导出 {} 字节 -> {}", d.len(), p.display())
                                    }
                                    Err(e) => self.status = format!("导出失败: {e}"),
                                }
                            }
                        }
                    }
                    if ui.button("导入…").clicked() {
                        if let Some(p) = FileDialog::new()
                            .add_filter("NV_CFG 转储 / mtd3 分区镜像", &["bin", "conf"])
                            .pick_file()
                        {
                            match std::fs::read(&p) {
                                Ok(data) => match calib::normalize_nv_cfg(&data) {
                                    Ok(blob) => {
                                        let n = String::from_utf8_lossy(&blob)
                                            .split('\0')
                                            .filter(|s| s.contains('='))
                                            .count();
                                        let note = if data.len() != blob.len() {
                                            format!("（已从 {} 字节分区镜像裁剪）", data.len())
                                        } else {
                                            String::new()
                                        };
                                        self.status = format!(
                                            "已导入 Wi-Fi NV_CFG: {} 字节 · {} 个字段{}",
                                            blob.len(),
                                            n,
                                            note
                                        );
                                        self.wifi_nvcfg = Some(blob);
                                    }
                                    Err(e) => self.status = format!("导入被拒: {e}"),
                                },
                                Err(e) => self.status = format!("读取失败: {e}"),
                            }
                        }
                    }
                });
            });
        });
    }

    fn ui_identity(&mut self, ui: &mut egui::Ui) {
        let Some(Ok(ext)) = self.result.as_ref() else {
            return;
        };
        ui.add_space(6.0);
        ui.strong("身份信息（产线烧录数据）");
        let i = &ext.extracted.identity;
        // 有 Wi-Fi 校准的机型才展示派生 MAC（与校准块内嵌值同规则）。
        let has_wifi_cal = matches!(
            self.model,
            Model::Tyfza1 | Model::Pt1570 | Model::Pt1570R10
        );
        egui::Grid::new("identity")
            .num_columns(2)
            .spacing([12.0, 5.0])
            .show(ui, |ui| {
                ui.weak("BASE_MAC（产线主 MAC）");
                ui.monospace(
                    i.base_mac
                        .as_ref()
                        .map_or("-".into(), |m| mac_to_string(m)),
                );
                ui.end_row();
                ui.weak("MAC 数量");
                ui.monospace(format!("{}", i.mac_count));
                ui.end_row();
                ui.weak("EPON_MAC（PON 口）");
                ui.monospace(
                    i.epon_mac
                        .as_ref()
                        .map_or("-".into(), |m| mac_to_string(m)),
                );
                ui.end_row();
                // 派生 MAC：与 Wi-Fi 校准块内嵌值一致（5G=base+2，2.4G=base+1）。
                if has_wifi_cal {
                    if let Some(base) = i.base_mac {
                        ui.weak("5G Wi-Fi MAC（base+2）");
                        ui.monospace(mac_to_string(&derived_mac(base, 2)));
                        ui.end_row();
                        ui.weak("2.4G Wi-Fi MAC（base+1）");
                        ui.monospace(mac_to_string(&derived_mac(base, 1)));
                        ui.end_row();
                    }
                }
                ui.weak("PON_SN（GPON SN）");
                ui.monospace(i.pon_sn.clone().unwrap_or_else(|| "-".into()));
                ui.end_row();
                ui.weak("DEVICE_SN（整机 SN）");
                ui.monospace(i.device_sn.clone().unwrap_or_else(|| {
                    if has_wifi_cal {
                        "-（无产线数据源）".into()
                    } else {
                        "-".into()
                    }
                }));
                ui.end_row();
            });
    }

    fn ui_sections(&mut self, ui: &mut egui::Ui) {
        let Some(Ok(ext)) = self.result.as_ref() else {
            return;
        };
        ui.add_space(6.0);
        ui.strong("校准与固件段（可单独导出）");
        ui.add_space(2.0);
        for row in SECTION_ROWS {
            let data = section_data(&ext.extracted, row.tag);
            ui.group(|ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.strong(row.title);
                    ui.weak(format!("{} · {}", row.name, row.note));
                    // Wi-Fi 校准行直接显示内嵌 MAC 实际值（5G=base+2、2.4G=base+1）
                    if row.tag == 0x0100 || row.tag == 0x0101 {
                        if let Some(base) = ext.extracted.identity.base_mac {
                            let off = if row.tag == 0x0100 { 2u64 } else { 1u64 };
                            ui.weak(format!("内嵌 {}", mac_to_string(&derived_mac(base, off))));
                        }
                    }
                    match data {
                        Some(d) => {
                            // xtemp 是 JSON 文本，悬停可预览开头若干字节。
                            let s = format!("{} 字节", d.len());
                            if row.tag == 0x0112 {
                                let preview =
                                    String::from_utf8_lossy(&d[..d.len().min(400)]).to_string();
                                ui.weak(s).on_hover_text(preview);
                            } else {
                                ui.weak(s);
                            }
                        }
                        None => {
                            ui.weak("空（此机型无）");
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(d) = data {
                            if ui.button("导出…").clicked() {
                                if let Some(p) = FileDialog::new()
                                    .set_file_name(row.file_name)
                                    .save_file()
                                {
                                    match std::fs::write(&p, d.as_slice()) {
                                        Ok(_) => {
                                            self.status = format!(
                                                "已导出 {} 字节 -> {}",
                                                d.len(),
                                                p.display()
                                            )
                                        }
                                        Err(e) => self.status = format!("导出失败: {e}"),
                                    }
                                }
                            }
                        }
                    });
                });
            });
            ui.add_space(2.0);
        }
    }

    fn ui_ubi_info(&mut self, ui: &mut egui::Ui) {
        let Some(Ok(ext)) = self.result.as_ref() else {
            return;
        };
        ui.add_space(6.0);
        ui.strong("UBI 容器信息");
        ui.monospace(format!(
            "peb={} leb={} 卷: {}",
            ext.ubi_peb,
            ext.ubi_leb,
            ext.volumes.join(", ")
        ));
    }

    fn ui_build_result(&mut self, ui: &mut egui::Ui) {
        let Some(out) = self.build_out.as_ref() else {
            return;
        };
        ui.add_space(6.0);
        ui.strong("生成结果（8 MiB RTLFACT1）");
        match out {
            Ok(b) => {
                if let Some(p) = &b.saved_to {
                    ui.monospace(p.display().to_string());
                }
                egui::Grid::new("build_summary")
                    .num_columns(4)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.weak("偏移");
                        ui.weak("字段");
                        ui.weak("大小");
                        ui.weak("CRC32");
                        ui.end_row();
                        for e in &b.entries {
                            ui.monospace(format!("0x{:06x}", e.offset));
                            ui.monospace(e.name);
                            ui.monospace(format!("{} 字节", e.size));
                            ui.monospace(format!("0x{:08x}", e.crc));
                            ui.end_row();
                        }
                    });
            }
            Err(e) => {
                ui.colored_label(egui::Color32::from_rgb(170, 40, 40), e);
            }
        }
    }

    /// 处理拖入的文件：按文件名分类到 env / env2 / ubi_device 槽位。
    fn handle_drops(&mut self, ctx: &egui::Context) {
        // egui 0.36 起 dropped_files 是 `Arc<dyn DroppedFile>` trait 对象，
        // `path()` 直接返回 `&Path`（无路径时为空）；过滤空路径后转拥有值。
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| {
                    let p = f.path();
                    (!p.as_os_str().is_empty()).then(|| p.to_path_buf())
                })
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let mut reextract = false;
        for path in dropped {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if name.contains("env2") {
                self.env2_path = Some(path);
                reextract = true;
            } else if name.contains("env") {
                self.env_path = Some(path);
                reextract = true;
            } else if name.contains("static_conf") || name.contains("nvcfg") {
                // pt1570 的 mtd3-static_conf.bin：Wi-Fi NV_CFG 存档槽
                // （接受裸段或整个 256 KiB 分区镜像，自动裁剪）
                match std::fs::read(&path) {
                    Ok(data) => match calib::normalize_nv_cfg(&data) {
                        Ok(blob) => {
                            let n = String::from_utf8_lossy(&blob)
                                .split('\0')
                                .filter(|s| s.contains('='))
                                .count();
                            let note = if data.len() != blob.len() {
                                format!("（已从 {} 字节分区镜像裁剪）", data.len())
                            } else {
                                String::new()
                            };
                            self.status = format!(
                                "已导入 Wi-Fi NV_CFG（拖拽）: {} 字节 · {} 个字段{}",
                                blob.len(),
                                n,
                                note
                            );
                            self.wifi_nvcfg = Some(blob);
                        }
                        Err(e) => self.status = format!("拖拽被拒（NV_CFG 校验失败）: {e}"),
                    },
                    Err(e) => self.status = format!("读取失败: {e}"),
                }
            } else if name.contains("a2h") || name.contains("eeprom") {
                // 光模块 A2h 转储：PON EEPROM 存档槽
                match std::fs::read(&path) {
                    Ok(data) => match calib::validate_a2h_dump(&data) {
                        Ok(()) => {
                            self.pon_eeprom = Some(data);
                            self.status = "已导入 PON EEPROM 转储（拖拽）· 校验和通过".into();
                        }
                        Err(e) => self.status = format!("拖拽被拒（A2h 校验失败）: {e}"),
                    },
                    Err(e) => self.status = format!("读取失败: {e}"),
                }
            } else {
                self.device_path = Some(path);
                reextract = true;
            }
        }
        if reextract {
            let ctx2 = ctx.clone();
            self.start_extract(&ctx2);
        }
    }
}

/// 一行数据来源：标题 + 提示 + 右侧「打开… / 清除」按钮 + 下方路径。
/// 返回 true 表示路径发生了变化（需要重新提取）。
fn source_row(
    ui: &mut egui::Ui,
    title: &str,
    path: &mut Option<PathBuf>,
    hint: &str,
    clearable: bool,
) -> bool {
    let mut changed = false;
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.strong(title);
            ui.weak(hint);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if clearable && path.is_some() && ui.button("清除").clicked() {
                    *path = None;
                    changed = true;
                }
                if ui.button("打开…").clicked() {
                    if let Some(p) = FileDialog::new()
                        .add_filter("镜像 / 备份文件", &["bin", "img"])
                        .pick_file()
                    {
                        *path = Some(p);
                        changed = true;
                    }
                }
            });
        });
        let text = path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<未选择>".into());
        ui.label(egui::RichText::new(text).weak().monospace());
    });
    ui.add_space(2.0);
    changed
}

/// 后台线程执行的完整提取流程（与 CLI dump 逻辑一致）。
/// `nv` 为 pt1570 的 mtd3 NV_CFG 转储（可选）——组装 Wi-Fi 校准（0x0100/0x0101）。
fn run_extract(
    model: Model,
    env: Option<PathBuf>,
    env2: Option<PathBuf>,
    device: PathBuf,
    nv: Option<Vec<u8>>,
) -> Result<Extraction, String> {
    let read = |p: &PathBuf| -> Result<Vec<u8>, String> {
        std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display()))
    };
    let env_bytes = env.as_ref().map(read).transpose()?;
    let env2_bytes = env2.as_ref().map(read).transpose()?;
    let device_bytes = read(&device)?;

    // env + env2 一起交给 env::load（内部按 flags 取更有效的一份）。
    let envset = if env_bytes.is_some() || env2_bytes.is_some() {
        let mut parts: Vec<Vec<u8>> = Vec::new();
        if let Some(b) = env_bytes {
            parts.push(b);
        }
        if let Some(b) = env2_bytes {
            parts.push(b);
        }
        let refs: Vec<&[u8]> = parts.iter().map(|p| p.as_slice()).collect();
        envmod::load(&refs)?
    } else {
        // tyfza1 / pt1570-r10 不读 env；给一个空 EnvSet 即可（字段全 pub）。
        envmod::EnvSet {
            variables: Default::default(),
        }
    };

    let image = ubi::parse(&device_bytes)?;
    let mut extracted = profile::profile(model.key())?(&envset, &image)?;
    // pt1570：用导入的 NV_CFG 组装 Wi-Fi 校准。5G=wlan1=base+2、2G=wlan0=base+1
    // （实机日志 hwaddr 证据），DIFF 字段缺失按产线惯例（实机全 0）补齐。
    if model == Model::Pt1570 {
        if let Some(nv) = &nv {
            // 魔数含非 ASCII 字节（72 D6 EF 41），字段查找只碰 ASCII 的
            // KEY=VALUE 区，lossy 解码不影响结果
            let text = String::from_utf8_lossy(nv).to_string();
            let base = extracted
                .identity
                .base_mac
                .ok_or("NV_CFG 组装需要 BASE_MAC")?;
            extracted.wifi0 =
                Some(calib::flash_calibration_nv(&text, &calib::RADIO1_5G, base)?);
            extracted.wifi1 =
                Some(calib::flash_calibration_nv(&text, &calib::RADIO0_2G, base)?);
        }
    }
    Ok(Extraction {
        ubi_peb: image.peb_size,
        ubi_leb: image.leb_size,
        volumes: image.volumes.keys().cloned().collect(),
        extracted,
    })
}

// ---- eframe 0.36 接线（亮色主题三件套与 h3c-factory 相同） ------------------

/// 构造本应用的亮色 [`egui::Style`]（只构造一次，之后每次只是 Arc 克隆）。
fn light_style() -> std::sync::Arc<egui::Style> {
    static STYLE: std::sync::OnceLock<std::sync::Arc<egui::Style>> = std::sync::OnceLock::new();
    STYLE
        .get_or_init(|| {
            let mut v = egui::Visuals::light();
            v.weak_text_color = Some(egui::Color32::from_gray(110));
            v.widgets.inactive.bg_fill = egui::Color32::from_gray(238);
            v.widgets.inactive.weak_bg_fill = egui::Color32::from_gray(238);
            v.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, egui::Color32::from_gray(190));
            v.widgets.hovered.bg_fill = egui::Color32::from_gray(205);
            v.widgets.hovered.weak_bg_fill = egui::Color32::from_gray(205);
            v.widgets.active.bg_fill = egui::Color32::from_gray(170);
            v.widgets.active.weak_bg_fill = egui::Color32::from_gray(170);
            v.panel_fill = egui::Color32::WHITE;
            v.window_fill = egui::Color32::WHITE;
            v.window_stroke.color = egui::Color32::from_gray(150);
            let mut style = egui::Style::default();
            style.visuals = v;
            std::sync::Arc::new(style)
        })
        .clone()
}

/// 把亮色主题强制写到 `ctx` 上，返回该 style 供调用方钉到当前帧的根 `Ui`。
fn apply_light_theme(ctx: &egui::Context) -> std::sync::Arc<egui::Style> {
    let style = light_style();
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_theme(egui::Theme::Light);
    style
}

impl eframe::App for App {
    /// 纯白清屏。eframe 默认清屏色是硬编码深色，而 egui 0.36 传给 `App::ui`
    /// 的根 `Ui` 自己不画背景，不覆盖这里整个窗口会发黑。
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Color32::WHITE.to_normalized_gamma_f32()
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // 每帧重申亮色主题：ctx 侧（两套 style 都写亮）+ 当前帧根 Ui 侧。
        ui.set_style(apply_light_theme(ui.ctx()));

        self.poll_job(ui.ctx());
        self.handle_drops(ui.ctx());

        self.ui_topbar(ui);
        ui.label(egui::RichText::new(&self.status).weak());
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.ui_sources(ui);
                self.ui_pon_eeprom(ui);
                self.ui_wifi_nvcfg(ui);
                self.ui_identity(ui);
                self.ui_sections(ui);
                self.ui_ubi_info(ui);
                self.ui_build_result(ui);
            });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([780.0, 860.0])
            .with_min_inner_size([700.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RTL9617C Factory 工具",
        options,
        Box::new(|cc| {
            font::install(&cc.egui_ctx);
            apply_light_theme(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}
