# RTL9617C Factory 图形工具

RTL9617C 平台 GPON 光猫 **Factory 镜像的查看 / 提取 / 生成** 工具，一套三个程序：

| 程序 | 形态 | 用途 |
|---|---|---|
| `rtl9617c-factory-gui` | Rust + egui 图形界面 | 选文件（支持拖入）→ 提取字段 → 生成 factory.bin，所见即所得 |
| `rtl9617c-factory` | 命令行 | 与上游 [rtl9617c-factory](https://github.com/lyq1996/rtl9617c-factory) 完全一致的 `build` / `dump` |
| `rtl9617c-factory-inspect` | 命令行 | 备份分析：打印 MAC / SN / 校准数据在备份里的**实际值与出处** |

核心提取与组装代码（`env` / `ubi` / `ubifs` / `xml` / `calib` / `profile` / `layout` /
`crc` 八个模块）**逐文件复用上游，逻辑零改动**；图形界面参考同系列的
[h3c-factory](https://github.com/lyq1996/h3c-factory)（fiberhome-factory 系）：
亮色主题、系统 CJK 字体、分组面板。

> ⚠️ **重要声明**
> - 本工具仅供 **研究 / 备份查看 / 自有设备维护** 使用，与 Realtek 及任何设备厂商无关联，不含厂商闭源代码。
> - 修改设备出厂数据可能导致设备无法启动或违反当地法规，请仅在自己拥有的设备上操作。

## 支持机型

| `--model` | 设备 | 产线数据来源 |
|---|---|---|
| `tyfza1` | 天邑 TYFZA1（Wi-Fi + 下行光 + FPGA） | `ubi_Config` 卷内 `config_hs.xml` / `config.xml` / `rtkbosa_k.bin` / `rtkoltbosa_k.bin` / `xtemp.cal`，`ubi_fpga` 卷 |
| `pt1500-h10` | 友华 PT1500-H10（无 Wi-Fi / 无下行光） | U-Boot env `kp_*` keyparam 变量 |
| `pt1570` | 友华 PT1570-H10（有 Wi-Fi 硬件，但备份中无校准数据：卷表无 `ubi_fpga`，`ubi_Config` 内无 config_hs.xml / BOSA / xtemp） | U-Boot env `kp_*` keyparam 变量（含整机 SN `kp_szSerialNumber`） |

需要的输入文件（整机备份里都有）：

| 文件 | 说明 | tyfza1 | pt1500-h10 / pt1570 |
|---|---|---|---|
| `mtd1_env.bin` | U-Boot env 主分区 | 可省（不使用） | **必需** |
| `mtd2_env2.bin` | env 备分区（可选） | 多分区时取 flags 更大者 | 同左 |
| `mtd4_ubi_device.bin` | UBI 容器整机镜像 | **必需** | **必需** |

## 数据从哪里来

`factory.bin` 每个字段在备份中的出处（tyfza1）：

| factory.bin 字段 | 出处 | 说明 |
|---|---|---|
| `BASE_MAC` | `ubi_Config` → `config_hs.xml` → `ELAN_MAC_ADDR` | 产线主 MAC，`count` 固定 3 |
| `EPON_MAC` | `ubi_Config` → `config.xml` → `<chain chainName="EPON_LLID_TBL">` 内 `macAddr` | 没有则为空 |
| `PON_SN` | `ubi_Config` → `config_hs.xml` → `GPON_SN`（12 字符） | |
| `DEVICE_SN` | —（tyfza1 无产线数据源） | pt1500-h10 来自 env `kp_szSerialNumber` |
| `WIFI_CAL_RADIO0/1` | `config_hs.xml` 中几十个 `HW_*` 字段 + BASE_MAC 派生 MAC **逐字节组装**成原厂 `TYFC0003` 格式，各 4 KiB | 不是现成文件；5G 块内嵌 MAC = base+2，2G = base+1 |
| `BOSA_CALIBRATION` | `ubi_Config` → `rtkbosa_k.bin`（1024 B） | 下行光模块校准 |
| `BOSA_OLT_CALIBRATION` | `ubi_Config` → `rtkoltbosa_k.bin`（588 B） | |
| `XTEMP_CALIBRATION` | `ubi_Config` → `xtemp.cal`（JSON 文本） | GUI 悬停该行可预览 |
| `FPGA_IMAGE` | UBI 卷 `ubi_fpga` 原始内容（fpgafs squashfs） | 整卷直搬，洞补 `0xFF` |

> `mtd1/mtd2` env 对 tyfza1 不承载产线身份数据（只是 U-Boot 变量）；env 机型
> （pt1500-h10 / pt1570）从 env 的 `kp_ucaBaseMacAddr` / `kp_ulNumMacAddrs` /
> `kp_szXponSN` 取数，pt1570 另有整机 SN `kp_szSerialNumber`（PT1570 实测样例：
> MAC `68:7d:00:5a:19:be`、MAC 数量 11、XPON SN `YHTC005A19BE`、整机 SN
> `0E863687D005A19BE`）。想看备份里的原始数据，用
> `rtl9617c-factory-inspect mtd1_env.bin mtd2_env2.bin mtd4_ubi_device.bin [model]`。

## Factory bin 规范（RTLFACT1）

- 总大小 `0x800000`（8 MiB），小端；未用区域 `0xFF`，ASCII 以 NUL 结尾。
- **Header**（`0x000`）：magic `"RTLFACT1"`、`version=1`、`image_size`、`model`
  （1=tyfza1，2=pt1500-h10）、`image_crc`、`generation`（Unix 秒）、`table_crc`、`header_crc`。
- **目录**（`0x200`–`0x400`）：32 条 × 16 B（tag / flags / offset / size / crc32），按 tag 升序。
- **字段区**（固定偏移）：`0x1000` 身份（BASE_MAC / EPON_MAC / PON_SN / DEVICE_SN）、
  `0x2000`–`0x5000` 校准（Wi-Fi ×2 / BOSA ×2 / xtemp）、`0x10000` 起 FPGA。
- **CRC**：统一 CRC32（`0xEDB88320` 反射）。目录数据用 zlib 式（带终值异或）；
  `table_crc` / `header_crc` / `image_crc` 用寄存器式（无终值异或），其中
  `header_crc` 对 header 三段链式计算，`image_crc` 计算时本字段置 0。

完整规范见上游 README；实现见 `src/layout.rs`（含单元测试）。

## 紧凑镜像（64 KiB，不含 FPGA）

8 MiB 完整镜像里，**所有身份 + 校准字段都在 `0x10000` 之前**，FPGA 独占后面
约 4.1 MiB。据此提供紧凑镜像导出（`0x10000` = 64 KiB）：

- 布局、目录、CRC 链与完整镜像**完全同规则**，仅 `image_size = 0x00010000`、
  无 `FPGA_IMAGE` 段（传入的 0x0200 段会被忽略，有单测锁定）；
- 适合小体积留存 MAC / SN / Wi-Fi TYFC0003 / BOSA / xtemp，便于对照与二次开发；
- ⚠️ **不可直接刷写**：恢复设备请用完整 8 MiB 镜像整体写入 UBI static 卷
  `factory`（`ubiupdatevol` 或 U-Boot `ubi write`）。

```bash
rtl9617c-factory build --model tyfza1 --env mtd1_env.bin --env2 mtd2_env2.bin \
    --device mtd4_ubi_device.bin --compact -o factory-compact.bin
```

GUI 对应顶栏「导出紧凑镜像 (64 KiB)…」按钮。

## 用法

### 图形界面

1. 选机型（默认天邑 TYFZA1）；
2. 选 / 拖入 `mtd1_env.bin`（pt1500-h10 必需）、可选 `mtd2_env2.bin`、
   `mtd4_ubi_device.bin` —— 文件就位后自动在后台线程提取（大镜像不卡界面）；
3. 查看身份信息（BASE_MAC / MAC 数量 / EPON_MAC / PON_SN / DEVICE_SN）、
   各校准段大小（可单独导出留档）、UBI 卷列表；
4. 「生成 factory.bin…」存 8 MiB 完整镜像，或「导出紧凑镜像 (64 KiB)…」；
   段表（偏移 / 大小 / CRC32）直接显示在窗口里。

### 命令行

```bash
# 生成 8 MiB factory.bin（打印各段摘要）
rtl9617c-factory build --model tyfza1 --env mtd1_env.bin --env2 mtd2_env2.bin \
    --device mtd4_ubi_device.bin -o factory.bin

# 生成 64 KiB 紧凑镜像（不含 FPGA）
rtl9617c-factory build --model tyfza1 ... --compact -o factory-compact.bin

# 打印提取到的字段
rtl9617c-factory dump --model tyfza1 --env mtd1_env.bin --device mtd4_ubi_device.bin

# 备份分析：env 变量 / UBI 卷 / UBIFS 文件 / XML 产线字段 / 校准内容 / 内嵌 MAC 定位
rtl9617c-factory-inspect mtd1_env.bin mtd2_env2.bin mtd4_ubi_device.bin
```

## 与上游 CLI 的一致性

GUI 与 CLI 共用同一份核心代码（`src/` 下八个模块未改动任何逻辑），两者产出的
factory.bin 除 header `0x18` 处 8 字节 `generation` 时间戳（生成时刻）外
**逐字节相同**。

## 构建

```bash
cargo build --release --locked
```

产物在 `target/release/`（见顶部三件套表格）。Linux 需窗口后端：
`cargo build --release --features linux-x11`。

Windows 本地构建需要 **MSVC Build Tools**（提供 `link.exe`；只装 rustup 会报
`linker 'link.exe' not found`）。

### GitHub Actions（推荐）

`.github/workflows/build.yml`（h3c-factory 三平台模式）：

- **触发**：任意 push / PR / 手动 workflow_dispatch；
- **矩阵**：Windows x64（zip）、macOS arm64（tar.gz）、Linux x64
  （装 `libx11-dev` / `libxkbcommon-dev` 后 `--features linux-x11` 构建）；
- Windows 作业先跑 `cargo test --locked --lib`（布局 / env / UBI 单元测试）；
- **产物**：每平台压缩包内含三件套全部可执行文件；
- **发布**：push / PR 触发的进 `nightly`（prerelease），手动触发的进 `Latest`，
  同名发布自动覆盖更新。

## 目录结构

```
rtl9617c-factory-gui/
├── Cargo.toml
├── .github/workflows/build.yml   # 三平台构建 + nightly/Latest 发布
└── src/
    ├── lib.rs          # 挂载核心模块
    ├── bin/
    │   ├── cli.rs      # 上游 CLI（build / dump，--compact 为本仓新增）
    │   ├── inspect.rs  # 备份分析工具
    │   └── gui/
    │       ├── main.rs # egui 图形界面
    │       └── font.rs # 系统 CJK 字体安装（自 h3c-factory 移植）
    ├── calib.rs        # Wi-Fi 校准 TYFC0003 组装（上游）
    ├── crc.rs          # CRC32：zlib 式 + 寄存器式（上游）
    ├── env.rs          # U-Boot env 解析（上游）
    ├── layout.rs       # RTLFACT1 布局 / build / summary / build_compact（上游 + 紧凑镜像）
    ├── profile.rs      # 机型字段提取（上游）
    ├── ubi.rs          # UBI 容器解析（上游）
    ├── ubifs.rs        # UBIFS 文件提取（上游）
    └── xml.rs          # config_hs.xml / config.xml 字段读取（上游）
```

## 许可证

**[GPL-2.0-only](LICENSE)**。

上游 `rtl9617c-factory` 为 GPL-2.0-only；`src/bin/gui/font.rs` 移植自
`h3c-factory`（其源自 fiberhome-factory，同为 GPL-2.0-only），故本项目沿用
同一许可证。感谢原作者的工作。
