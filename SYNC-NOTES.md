# 上游同步改动清单（2026-11-14）

## 基线

| | GUI 副本 | 上游 |
|---|---|---|
| 仓库 | `qwe-qaq/rtl9617c-factory-gui` | `lyq1996/rtl9617c-factory` |
| 版本 | `main @ 18ff38c1`（2026-11-07） | `main @ 7a2455c8`（2026-11-13，`layout: output fields at fixed offsets`） |

上游 11-13 重构**删除了 `RTLFACT1` magic、头部、32 条目录表和三重 CRC**，改为纯
`0xFF` + 固定偏移；机型改长名、镜像大小改按机型（8 MiB / 64 KiB）。GUI 副本基于上游
10-28 初版，两者产物原本互不兼容。

## 策略：双格式可切换，不覆盖

上游的新格式做成**第二种可选输出格式**（`--format raw` / GUI 顶栏切换），原有
`RTLFACT1` 及其全部增强（4 机型、紧凑镜像、EEPROM / NV_CFG 两个新 tag、`summary()`
校验、图形界面）完整保留。`raw` 模式严格对齐上游，绝不静默丢弃数据。

## 改动文件

| 文件 | 改动 | diff 行数 |
|---|---|---|
| `src/layout.rs` | 新增 `OutputFormat`、`RAW_FIELDS`（与上游 `FIELDS` 逐项一致）、`upstream_name()` tag 映射、`build_raw()`、`summarize_raw()`；新增 5 个单测。原有 `build()` / `build_compact()` / `summary()` 逻辑未改 | 247 |
| `src/profile.rs` | 新增 `MODEL_SPECS` 机型表（短名 + 上游长别名 + model id + 两种镜像大小）、`canonical()` / `model_spec()` / `upstream_image_size()`；`profile()` / `model_id()` 改为查表，行为对短名不变 | 98 |
| `src/bin/cli.rs` | 新增 `--format <rtl1|raw>`；`build` 按格式分发；`dump` 支持上游长机型名；raw 模式下拒绝 `--compact` / `--pon-eeprom` / `--wifi-nvcfg`；usage 更新 | 111 |
| `src/bin/gui/main.rs` | 新增 `BuildMode`、`App::format`、顶栏输出格式下拉框、`format_label()`；`build_image()` 改为三模式；raw 模式下生成按钮与文件名切换 | 212 |
| `Cargo.toml` | version `0.2.0 → 0.3.0`；GUI 二进制加 `required-features = ["gui"]`，使无窗口依赖环境可用 `--no-default-features` | 23 |
| `Cargo.lock` | 根包版本同步为 0.3.0 | 11 |
| `README.md` | 新增「与上游同步」章节、双格式对照表、raw 用法示例、构建/验证命令、目录结构更新；澄清「与上游 CLI 一致性」章节的含义 | 170 |
| `examples/raw_sample.rs` | **新增**：构造固定字段生成 raw 样例镜像，供跨语言比对 | 51 行 |
| `scripts/verify_upstream_compat.py` | **新增**：独立实现上游 `layout.rs::build` 作为 golden，逐字节比对 64 KiB / 8 MiB 两种产物 | 112 行 |

## 机型别名映射

| 本仓短名（保留） | 上游长名（新增别名） | raw 镜像大小 |
|---|---|---|
| `tyfza1` | `tianyi-tyfza1-2p5g` | 8 MiB |
| `pt1500-h10` | `youhua-pt1500-h10-ct-2p5g` | 64 KiB |
| `pt1570` | `youhua-pt1570-h10`（本仓扩展，上游 CLI 不认识） | 64 KiB（按 env 机型推导） |
| `pt1570-r10` | `youhua-pt1570-r10`（本仓扩展，上游 CLI 不认识） | 8 MiB（按 XML 机型推导） |

## 验证结果

| 项目 | 命令 | 结果 |
|---|---|---|
| 核心库单测（21 个，含 5 个新增） | `cargo test --locked --no-default-features --lib` | 全部通过 |
| CLI / inspect / example 编译 | `cargo build --locked --no-default-features --bins --examples` | 通过 |
| 上游格式逐字节一致（64 KiB） | `python3 scripts/verify_upstream_compat.py` | PASS |
| 上游格式逐字节一致（8 MiB） | 同上 | PASS |
| `RTLFACT1` 未回归 | 单测 `rtl1_format_is_not_regressed_by_raw_layout` | 通过 |
| GUI 代码（含格式切换） | `cargo +1.95.0 check --locked --features linux-x11` | 通过（`CARGO_EXIT=0`） |
| CLI 冒烟（6 项） | 未知格式/长机型名/未知机型/raw+compact/raw+EEPROM/usage | 全部符合预期 |

> 环境说明：沙箱默认 rustc 1.93，而锁定的 egui/eframe 0.36.2 要求 **rustc ≥ 1.95**
> （`epaint` 用到 1.95 才稳定的 `array_windows` / `push_mut`）。已安装 1.95.0 工具链
> 完成 GUI 编译检查；Linux 还需 `--features linux-x11`（原项目既有要求）。
> 编译仅有 1 个原项目遗留的 `unused import` 警告（`src/calib.rs`），不影响功能。

## 用法

```bash
# 默认：本仓增强的 RTLFACT1（8 MiB / 紧凑 64 KiB 存档）
rtl9617c-factory build --model tyfza1 ... -o factory.bin

# 上游当前版格式：纯固定偏移、无校验、大小按机型
rtl9617c-factory build --model tianyi-tyfza1-2p5g --format raw ... -o factory-upstream.bin
rtl9617c-factory build --model youhua-pt1500-h10-ct-2p5g --format raw ... -o factory-64k.bin
```

GUI：顶栏「输出格式」下拉切换；`raw` 下按钮变为「生成上游格式 factory.bin…」。

## 注意事项与未做的事

- **两个格式不能混用**：`RTLFACT1` 与 `raw` 的产物互不兼容，刷写前必须确认目标引导程序
  期望哪一种。特别是 64 KiB：本仓 `RTLFACT1` 的 64 KiB 是**存档用**、不可刷写；上游
  `raw` 的 64 KiB 是 PT1500-H10 的**正式可刷写**镜像。
- `raw` 模式只支持上游固定的 10 个字段；要使用 `PON_MODULE_EEPROM`（`0x0102`）或
  `WIFI_NV_CFG`（`0x0103`），必须切回 `RTLFACT1`，否则 CLI / GUI 明确报错。
- **未推送到 GitHub**：本目录是本地副本，无仓库写权限；如需同步到远端请自行提交。
- **未在真实设备上刷写验证**：全部结论来自代码分析与字节级比对。
- 上游 11-13 的 `profile.rs` 重构（长机型名、`MODELS` 表、`Section { name }` 结构）
  没有逐行搬过来——本仓保留 tag 索引以支持新增字段，只把「长名别名 + 按机型大小」
  这两点对外行为同步了过来。
