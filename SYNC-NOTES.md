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

上游的新格式做成**第二种可选输出格式**，原有 `RTLFACT1` 及其全部增强（4 机型、
紧凑镜像、`WIFI_NV_CFG` 新 tag、`summary()` 校验、图形界面）完整保留。
`raw` 模式严格对齐上游，绝不静默丢弃数据。**r4 起 `raw` 设为默认导出格式**，
`RTLFACT1` 通过 `--format rtl1` / GUI 下拉显式选择。

## 改动文件

| 文件 | 改动 |
|---|---|
| `src/layout.rs` | 新增 `OutputFormat`、`RAW_FIELDS`（与上游 `FIELDS` 逐项一致）、`upstream_name()`、`build_raw()`、`summarize_raw()`；r2 新增 `filter_raw_sections()`；r3 移除 `0x0102` 槽位、精简过滤逻辑并补回归单测；**r4 给 `OutputFormat` 加 `Default`（`UpstreamRaw`）并补单测断言**。原有 `build()` / `build_compact()` / `summary()` 逻辑未改 |
| `src/profile.rs` | 新增 `MODEL_SPECS` 机型表（短名 + 上游长别名 + model id + 两种镜像大小）、`canonical()` / `model_spec()` / `upstream_image_size()`；**r4 把 `pt1570-r10`（`youhua-pt1570-r10`）的 `upstream_image_size` 由 8 MiB 改为 64 KiB**；`profile()` / `model_id()` 改为查表，行为对短名不变 |
| `src/calib.rs` | r3 删除 `A2H_DUMP_MAX` / `validate_a2h_dump()` / A2h 校验单测；保留 NV_CFG 归一化与 TYFC0003 组装 |
| `src/bin/cli.rs` | 新增 `--format <rtl1\|raw>`；`build` 按格式分发；`dump` 支持上游长机型名；raw 下**仅**拒绝 `--compact`；r2 改为过滤存档段；r3 删除 `--pon-eeprom` 参数与 usage 提示；**r4 `--format` 缺省值改用 `OutputFormat::default()`，usage 标注 default: raw** |
| `src/bin/gui/main.rs` | 新增 `BuildMode`、`App::format`、顶栏输出格式下拉框、`format_label()`；`build_image()` 三模式；r2 删除 raw 硬报错、改为过滤 + 展示跳过段；r3 删除 EEPROM 字段/整行 UI/拖拽识别；**r4 初始格式改用 `OutputFormat::default()`，同步注释与文件头说明** |
| `Cargo.toml` / `Cargo.lock` | version `0.2.0 → 0.3.0`；GUI 二进制加 `required-features = ["gui"]` |
| `README.md` / `SYNC-NOTES.md` | 新增「与上游同步」「raw 模式下的 NV_CFG」小节；r3 替换 EEPROM 存档整章为「不写入 factory.bin」说明；**r4 全面改为「默认 raw」并新增「默认输出格式与 R10 规格」小节** |
| `examples/raw_sample.rs` | **新增**：构造固定字段生成 raw 样例镜像，供跨语言比对 |
| `scripts/verify_upstream_compat.py` | **新增**：独立实现上游 `layout.rs::build` 作为 golden，逐字节比对 64 KiB / 8 MiB |
| `tests/nvcfg_raw.rs` | **新增（r2）**：端到端验证 NV_CFG 只写校准、不写原档；r3 移除 0x0102 段 |
| `tests/default_raw.rs` | **新增（r4）**：锁定默认格式、`youhua-pt1570-r10` 别名与 64 KiB 规格（3 个用例） |

## 机型别名映射

| 本仓短名（保留） | 上游长名（别名） | raw 镜像大小 |
|---|---|---|
| `tyfza1` | `tianyi-tyfza1-2p5g` | 8 MiB |
| `pt1500-h10` | `youhua-pt1500-h10-ct-2p5g` | 64 KiB |
| `pt1570` | `youhua-pt1570-h10`（本仓扩展，上游 CLI 不认识） | 64 KiB（按 env 机型推导） |
| `pt1570-r10` | `youhua-pt1570-r10`（本仓扩展，上游 CLI 不认识） | **64 KiB（r4：用户确认的目标规格）** |

## 验证结果

| 项目 | 命令 | 结果 |
|---|---|---|
| 核心库单测（含 r4 默认格式断言） | `cargo test --locked --no-default-features --lib` | 全部通过 |
| 端到端集成测试 | `cargo test --locked --no-default-features --test nvcfg_raw` | 通过 |
| 规格回归集成测试（r4 新增） | `cargo test --locked --no-default-features --test default_raw` | 通过 |
| CLI / inspect / example 编译 | `cargo build --locked --no-default-features --bins --examples` | 通过 |
| 上游格式逐字节一致（64 KiB / 8 MiB） | `python3 scripts/verify_upstream_compat.py` | 双 PASS |
| `RTLFACT1` 未回归 | 单测 + 集成测试 | 通过 |
| GUI 代码（含默认 raw 与格式切换） | `cargo +1.95.0 check --locked --features linux-x11` | 通过 |
| CLI 冒烟 | 不带 `--format` 输出 `format=raw`；显式 `--format rtl1` 仍可用；`raw`+`--compact` 仍拒绝 | 符合预期 |

> 环境说明：沙箱默认 rustc 1.93，而锁定的 egui/eframe 0.36.2 要求 **rustc ≥ 1.95**
> （`epaint` 用到 1.95 才稳定的 `array_windows` / `push_mut`）。已安装 1.95.0 工具链
> 完成 GUI 编译检查；Linux 还需 `--features linux-x11`（原项目既有要求）。
> 编译仅有 1 个原项目遗留的 `unused import` 警告（`src/calib.rs`），不影响功能。

## 用法

```bash
# 默认即为上游 raw 格式：tyfza1 8 MiB，其余机型 64 KiB
rtl9617c-factory build --model tyfza1 ... -o factory.bin
rtl9617c-factory build --model youhua-pt1570-r10 ... -o factory-64k.bin

# 显式切回本仓增强的 RTLFACT1（完整 8 MiB / 紧凑 64 KiB 存档）
rtl9617c-factory build --model tyfza1 --format rtl1 ... -o factory-rtl1.bin
rtl9617c-factory build --model tyfza1 --format rtl1 --compact ... -o factory-compact.bin

# raw + NV_CFG：原档不写入，仅用它组装 Wi-Fi 校准段（PT1570 64 KiB）
rtl9617c-factory build --model pt1570 --wifi-nvcfg mtd3_static_conf.bin \
    --env mtd1_env.bin --device mtd4_ubi_device.bin -o factory-64k.bin
```

GUI：顶栏「输出格式」下拉切换，打开时默认「上游 raw（固定偏移）」。

## 修订 r2（2026-11-14）：NV_CFG 只写校准、不写原档

**问题**：r1 把「导入 NV_CFG」误当成「必须把 NV_CFG 原档写进镜像」，于是 raw 模式下
一发现 `--wifi-nvcfg` / 已导入 NV_CFG 就直接报错。但 NV_CFG 实际承担两件事：

1. 作为**源数据**组装 `WIFI_CAL_RADIO0`（5G，`0x0100`）/ `WIFI_CAL_RADIO1`（2G，
   `0x0101`）——这两段属于上游 `RAW_FIELDS`（`0x2000` / `0x3000`），本就该写入；
2. **原样存档**为 `0x0103`——上游格式没有槽位（r3 起这是唯一的存档段）。

r1 的守卫把 1 和 2 一起拦了，导致 PT1570 在 raw 模式下连 Wi-Fi 校准都写不进去。

**修法**：新增 `layout::filter_raw_sections()`，raw 模式只跳过存档段并返回跳过说明；
其他未知 tag 仍由 `build_raw()` 报错兜底。CLI 与 GUI 都改为「过滤 + 明确列出」，
不再整单失败，也不静默丢数据。

| 段 | `RTLFACT1` | `raw` |
|---|---|---|
| `0x0100` / `0x0101` Wi-Fi 校准（由 NV_CFG 组装） | 写入 | **写入** |
| `0x0103` NV_CFG 原档 | 写入 | 跳过并列出 |
| 其余 10 个固定字段 | 写入 | 写入 |

## 修订 r3（2026-11-14）：移除 PON_MODULE_EEPROM

**依据**：README 早已实测确认——env 机型的 PON 口校准在光模块 A2h EEPROM
（SFF-8472，I2C `0x51`），**不在任何 flash 分区**；整盘 245 MB 备份扫描无
`bosa` / `rtkbosa` / `xtemp` / `calib`。也就是说**备份里根本没有这段数据**，
factory.bin 不需要 `PON_MODULE_EEPROM`（tag `0x0102` / 偏移 `0x6000`）这个槽位。
它是 r1 从 GUI 原仓带过来的「存档增强」，属于多余功能。

**删除范围**（`0x6000` 保持留空，不改变其他字段偏移）：

| 位置 | 删除内容 |
|---|---|
| `src/layout.rs` | `FIELDS` 中的 `0x0102` FieldSpec、`filter_raw_sections()` 中的 `0x0102` 分支、`pon_module_eeprom_field` 单测 |
| `src/calib.rs` | `A2H_DUMP_MAX`、`validate_a2h_dump()`、`a2h_dump_validation` 单测 |
| `src/bin/cli.rs` | `--pon-eeprom` 参数处理与 usage 提示 |
| `src/bin/gui/main.rs` | `pon_eeprom` 字段、`ui_pon_eeprom()` 整行与调用、拖拽 `a2h`/`eeprom` 识别、`build_image()` 中的 push |
| `tests/nvcfg_raw.rs` | `0x0102` 段与相关断言 |
| `README.md` | EEPROM 存档整章与全部功能提及 |

**保留**：「PON 校准在光模块 EEPROM、不在 flash 分区」这条硬件事实说明
（它解释 BOSA / XTEMP 段为何为空），并把原整章压缩为「关于光模块 EEPROM
（不写入 factory.bin）」说明段。另新增单测 `removed_eeprom_tag_is_rejected`，
断言 `spec(0x0102)` 与 `build(0x0102)` 均报错，防止该 tag 被误重新引入。

**兼容性**：RTLFACT1 目录表少一项，其余字段偏移（含 `0x7000` NV_CFG、
`0x10000` FPGA）全部不变；上游 raw 格式本就没有 `0x0102`，字节比对不受影响。
新版工具不再识别该 tag，旧版生成的含 `0x0102` 镜像请用旧版读取或重新生成。

## 修订 r4（2026-11-14）：默认导出 raw、R10 规格 64 KiB

**依据**：用户确认的目标规格三条——① 机型 `youhua-pt1570-r10`；② 其 raw 镜像为
**64 KiB**；③ **默认导出为 raw 镜像**。此前 r1 把 `pt1570-r10` 的 raw 大小按
「XML 机型」推导为 8 MiB，并把 `RTLFACT1` 设为默认，与实际目标不符。

**改动**：

| 项 | 改前 | 改后 |
|---|---|---|
| 默认输出格式 | `RTLFACT1`（CLI 缺省 / GUI 初始） | **`raw`**（`OutputFormat::default()` 单一来源） |
| `youhua-pt1570-r10` 别名 | 已存在（指向 `pt1570-r10`） | 不变 |
| `pt1570-r10` 的 `upstream_image_size` | `0x0080_0000`（8 MiB，推测值） | **`0x0001_0000`（64 KiB，确认规格）** |

**为什么 64 KiB 装得下**：R10 无 `ubi_fpga` 卷，`profile::sections()` 不会产生
`0x0200` FPGA_IMAGE 段；其余身份字段在 `0x1000`–`0x1030`，Wi-Fi 校准在
`0x2000` / `0x3000`，最高到 `0x6000`，全部落在 64 KiB 以内。若备份里意外出现
`ubi_fpga` 卷，`build_raw()` 会因 `0x10000` 越界**明确报错**而非静默截断。

**兼容性**：`RAW_FIELDS` 与 `build_raw()` 逻辑未动，64 KiB / 8 MiB 的 golden 逐字节
比对不受机型表改动影响；`RTLFACT1` 仍完整保留，只是需要显式 `--format rtl1`。
注意 `--compact` 现在必须搭配 `--format rtl1`，否则默认 raw 下会明确报错。

**回归保护**：新增 `tests/default_raw.rs`，锁定别名解析、64 KiB 大小、
`OutputFormat::default() == UpstreamRaw`，并验证 64 KiB 镜像能容纳身份 + 两段校准、
NV_CFG 原档区保持 `0xFF`。

## 注意事项与未做的事

- **两个格式不能混用**：`RTLFACT1` 与 `raw` 的产物互不兼容，刷写前必须确认目标引导程序
  期望哪一种。特别是 64 KiB：`RTLFACT1` 的 64 KiB 是**存档用**、不可刷写；默认
  `raw` 的 64 KiB 是 PT1500-H10 / PT1570 / `youhua-pt1570-r10` 的**正式镜像**。
- `raw` 模式只有上游固定的 10 个字段。`WIFI_NV_CFG`（`0x0103`）**原档不写入**，
  会在 CLI 报告 / GUI 结果区明确列出；由 NV_CFG 组装的 Wi-Fi 校准段照常写入。
  要保存原档请显式切回 `RTLFACT1`。
- **PT1570 / PT1570-R10 上游没有定义**：这两个机型是本仓新增的。`youhua-pt1570-r10`
  的 64 KiB 现为**用户确认的目标规格**（不再是推测值）；`pt1570`(H10) 的 64 KiB
  仍是按 env 机型推导的推测值，刷机前请确认目标引导程序期望的大小。
- **未推送到 GitHub**：本目录是本地副本，无仓库写权限；如需同步到远端请自行提交。
- **未在真实设备上刷写验证**：全部结论来自代码分析与字节级比对。
- 上游 11-13 的 `profile.rs` 重构（长机型名、`MODELS` 表、`Section { name }` 结构）
  没有逐行搬过来——本仓保留 tag 索引以支持新增字段，只把「长名别名 + 按机型大小」
  这两点对外行为同步了过来。
