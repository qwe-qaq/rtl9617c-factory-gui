# RTL9617C Factory 工具（图形界面 + CLI）

Rust + egui 图形工具，用于**提取 / 生成 / 对照** RTL9617C 平台 GPON/XPON 光猫的
产线身份数据与校准数据。核心提取与组装逻辑逐文件复用上游
[`rtl9617c-factory`](https://github.com/lyq1996/rtl9617c-factory)（逻辑零改动），
界面模式参考同系列 [`h3c-factory`](https://github.com/lyq1996/h3c-factory)
（fiberhome-factory 系）。

支持机型（`--model` / 界面切换）：

| 机型 | 设备 | 产线数据来源 | PON 口校准 | 紧凑镜像 |
|---|---|---|---|---|
| **tyfza1** | 天邑 TYFZA1（Wi-Fi + 下行光 + FPGA） | `ubi_Config` 卷内 XML/校准文件 + `ubi_fpga` 卷 | 有（flash 内，`rtkbosa_k.bin`） | 64 KiB，`RTLFACT1`（无 FPGA） |
| **pt1500-h10** | 友华 PT1500-H10（无 Wi-Fi / 无下行光） | U-Boot env `kp_*` keyparam | **无**（在光模块 EEPROM） | 同上 |
| **pt1570** | 友华 PT1570-H10（Wi-Fi 校准在 mtd3 NV_CFG，不足以组装 TYFC0003） | 同上（另有整机 SN `kp_szSerialNumber`） | **无**（在光模块 EEPROM） | 同上 |
| **pt1570-r10** | 友华 PT1570-R10（新一代固件：env 无 `kp_*`，数据回归 ubi_Config 卷 XML） | `ubi_Config` 卷 `config_hs.xml`/`config.xml`（同 tyfza1，含完整 Wi-Fi 校准字段） | **无**（在光模块 EEPROM） | 同上 |

> ⚠️ **重要声明**
> - 本工具仅供 **研究 / 备份查看 / 自有设备维护** 使用，与 Realtek、Cortina 及任何设备厂商无关联，未包含任何厂商闭源代码。
> - 生成的 factory.bin 由 U-Boot 经 `ubiupdatevol` / `ubi write` 写入 UBI static 卷 `factory`；紧凑镜像（64 KiB）**不可直接刷写**。
> - 修改设备出厂数据可能导致设备无法启动或违反当地法规，请仅在自己拥有的设备上操作。

## 输入文件

整机备份（`dd` 各 mtd）里用到的部分：

| 文件 | 说明 | tyfza1 | pt1500-h10 / pt1570 |
|---|---|---|---|
| `mtd1_env.bin` | U-Boot env 主分区 | 可省（不使用） | **必需** |
| `mtd2_env2.bin` | env 备分区（可选） | 多分区取 flags 更大者 | 同左 |
| `mtd4_ubi_device.bin` | UBI 容器整机镜像 | **必需** | **必需** |
| `mtd3_static_conf.bin` | NV_CFG（仅 pt1570 的 Wi-Fi 字段，见下文） | — | 仅分析用 |

## 数据从哪里来（实测）

### tyfza1：数据全在 mtd4 的 UBI 卷里

`mtd4` 按 EC/VID 头解析（每 PEB 校验 CRC，同 `(vol_id, lnum)` 取 `sqnum` 最新），
卷表（vol_id `0x7FFFEFFF`）定位卷名；`ubi_Config` 是 UBIFS 动态卷，沿
superblock / master（LEB 1、2 取 commit 最新）的 TNC 提交索引提取文件（压缩支持
none / LZO / zlib）：

| 文件 | 内容 | 进入 factory.bin 的字段 |
|---|---|---|
| `config_hs.xml` | 产线 XML（`<Value Name="…"/>`） | `ELAN_MAC_ADDR` → BASE_MAC（count 固定 3）、`GPON_SN` → PON_SN（12 字符）、几十个 `HW_*` → Wi-Fi 校准组装 |
| `config.xml` | 运行配置 | `<chain chainName="EPON_LLID_TBL">` 内 `macAddr` → EPON_MAC（没有则为空） |
| `rtkbosa_k.bin` | 1024 B | BOSA_CALIBRATION（PON 上行口光模块校准） |
| `rtkoltbosa_k.bin` | 588 B | BOSA_OLT_CALIBRATION（下行 OLT 光口，FTTR 下行光） |
| `xtemp.cal` | JSON 文本 | XTEMP_CALIBRATION |
| `ubi_fpga` 卷 | 原始内容（fpgafs squashfs，实测 4,317,184 B） | FPGA_IMAGE（洞补 `0xFF`，整卷直搬） |

### pt1500-h10 / pt1570：数据全在 U-Boot env 里

env 格式：`crc32(4B LE) + flags(1B) + data`，`data` 为 `name=value\0` 列表，CRC
为 zlib 式。多分区均校验，**取 flags 更大者**（PT1570 实测 env2 flags=42 > env1
flags=41）。产线 keyparam 即 `kp_*`：

| keyparam | 进入 factory.bin 的字段 | PT1570 实测样例 |
|---|---|---|
| `kp_ucaBaseMacAddr` | BASE_MAC | `68:7d:00:5a:19:be` |
| `kp_ulNumMacAddrs` | MAC 数量 | `11` |
| `kp_szXponSN` | PON_SN | `YHTC005A19BE` |
| `kp_szSerialNumber` | DEVICE_SN（可选，pt1500 备份常无） | `0E863687D005A19BE` |
| `kp_szProductClass` 等 | **不入 factory.bin**（产品型号/省份/SSID/密码等属上层软件数据） | `PT1570-H10` |

PT1570 的 mtd4 卷表（两份副本逐记录一致）：`ubi_Config / ubi_DTB0/1 / ubi_k0/1 /
ubi_r0/1 / ubi_framework1/2 / ubi_otherConfig / ubi_apps / ubi_BakConfig`，共 12 卷，
**无 `ubi_fpga`**。`ubi_Config` 内只有 `psi`（TR-069 XML 运行时配置，155,275 B，
LZO 压缩）、`scratchpad`、`newkey`/`newkey.cert`、`mnclt_*` 等，**无任何校准文件**。

### pt1570-r10：换代固件，数据回归 config_hs.xml

PT1570-R10（ProductClass `PT1570-R10-`，V3.04.15711U05）换了固件代际：`booti` +
lzma 内核，**env 里没有任何 `kp_*`**，产线数据回归 tyfza1 式布局——`ubi_Config`
卷（UBIFS / LZO）内含 `config_hs.xml`（37,377 B）/ `config.xml`（138,717 B）及
`config_bak` 等 12 个文件。实测身份：MAC `1047e74d673c`、GPON_SN `YHTCE74D673C`、
`EPON_LLID_TBL` 8 条派生链首条 = BASE_MAC。**TYFC0003 组装所需的全部字段（含 9 个
TSSI GAIN/CW DIFF）都在**（config_hs.xml 共 339 个 `HW_*` 字段，thermal/xcap 到
A–D 四路），故 R10 走 tyfza1 同款提取，Wi-Fi 校准可完整组装。

> ⚠️ wlan 频段绑定为**推定**：`HW_WLAN0_FEM_TYPE=238` / `HW_WLAN1_FEM_TYPE=17`
> 与 tyfza1 的 wlan0=5G 组合一致，故沿用 5G=base+2 / 2G=base+1。请在实机上用
> `iw dev` 核对 wlan MAC，若不符反馈实际值即可调整。另 R10 无 `ubi_fpga` 卷、
> 无 BOSA/xtemp 文件；备份多出的 `ubi_dpi` 卷（数据面配置）未纳入提取。

### ⚠️ env 机型的 PON 口校准在 EEPROM，不在分区里

作者实测确认：**1500 系的 PON 口校准存放在光模块 A2h EEPROM（SFF-8472，I2C 从机
`0x51`），不在任何 flash 分区**。整盘 245 MB 扫描 `bosa/rtkbosa/xtemp/calib` 全部
0 命中；`/data` 的 `pmd_*`/`laser.conf` 与 psi 的 `PmdCfg` 也为空。所以：

- 备份里**没有**是正常现象，factory.bin 也**无需**包含它；
- 刷写 factory.bin 不触碰 EEPROM，校准天然安全；
- 要存档/迁移校准 → 在**原厂固件**上 I2C dump（见下文「PON 模块 EEPROM 存档」）。

## factory.bin 规范（RTLFACT1）

总大小 `0x800000`（8 MiB），多字节整数一律小端；未用区域 `0xFF`，ASCII 以 NUL
结尾。

### Header（`0x000`–`0x0FF`）

| 偏移 | 大小 | 字段 | 说明 |
|---|---|---|---|
| `0x000` | 8 | magic | `"RTLFACT1"` |
| `0x008` | 4 | version | 当前 `1` |
| `0x00C` | 4 | image_size | `0x800000` |
| `0x010` | 4 | model | 1=tyfza1，2=pt1500-h10，3=pt1570 |
| `0x014` | 4 | image_crc | 整镜像寄存器式 CRC32，计算时本字段置 0 |
| `0x018` | 8 | generation | 生成时刻 Unix 秒（GUI 与 CLI 产物唯一可能不同的字节） |
| `0x020` | 4 | table_crc | 目录区 CRC |
| `0x024` | 4 | header_crc | 三段链式（见下） |
| `0x028` | `0x1D8` | 保留 | `0xFF` |

### 目录（`0x200`–`0x400`）

32 条 × 16 B，按 tag 升序；空条目保持 `0xFF`（`tag == 0xFFFF` 视为不存在）：

| 偏移 | 大小 | 字段 |
|---|---|---|
| `+0` | 2 | tag（LE u16） |
| `+2` | 2 | flags（bit0 = present） |
| `+4` | 4 | offset（镜像内绝对偏移） |
| `+8` | 4 | size（实际长度，不含 pad） |
| `+12` | 4 | crc32（数据区，zlib 多项式） |

### 字段区

| 偏移 | 容量 | tag | 字段 | tyfza1 | pt1500-h10 / pt1570 |
|---|---|---|---|---|---|
| `0x1000` | `0x10` | `0x0001` | BASE_MAC（6B MAC + 4B count） | `ELAN_MAC_ADDR`，count=3 | `kp_ucaBaseMacAddr` + `kp_ulNumMacAddrs` |
| `0x1010` | `0x06` | `0x0002` | EPON_MAC | `EPON_LLID_TBL.macAddr`（无则空） | = BASE_MAC |
| `0x1020` | `0x10` | `0x0010` | PON_SN | `GPON_SN` | `kp_szXponSN` |
| `0x1030` | `0x20` | `0x0011` | DEVICE_SN | 空（无产线数据源） | `kp_szSerialNumber`（pt1570 有） |
| `0x2000` | `0x1000` | `0x0100` | WIFI_CAL_RADIO0（TYFC0003） | 5G `rtl8852ce` 组装 | 空 |
| `0x3000` | `0x1000` | `0x0101` | WIFI_CAL_RADIO1（TYFC0003） | 2G `rtl8192xbe` 组装 | 空 |
| `0x4000` | `0x800` | `0x0110` | BOSA_CALIBRATION | `rtkbosa_k.bin`（1024 B） | 空（在 EEPROM） |
| `0x4800` | `0x400` | `0x0111` | BOSA_OLT_CALIBRATION | `rtkoltbosa_k.bin`（588 B） | 空（在 EEPROM） |
| `0x5000` | `0x1000` | `0x0112` | XTEMP_CALIBRATION | `xtemp.cal`（JSON） | 空 |
| `0x6000` | `0x400` | `0x0102` | **PON_MODULE_EEPROM**（本仓新增） | 可选（见 EEPROM 章节） | 可选 |
| `0x7000` | `0x400` | `0x0103` | **WIFI_NV_CFG**（本仓新增） | — | 可选（pt1570 mtd3 NV_CFG 原样存档） |
| `0x10000` | `0x7E0000` | `0x0200` | FPGA_IMAGE | `ubi_fpga` 卷原始内容 | 空 |

`0x001040`–`0x001FFF`、`0x007000`–`0x00FFFF` 为预留空隙；新硬件字段优先使用
tag `0x0012`–`0x00FF`（身份区）与 `0x0102`–`0x010F` / `0x0113`–`0x011F`（校准区），
并复用预留空隙的偏移——`0x0102` 即按此规则落在 `0x6000`。

### CRC 规则

统一 CRC32（多项式 `0xEDB88320`，反射），两种用法：

- `crc32(data)`：zlib 式（初值 `0xFFFFFFFF`，终值异或）——目录条目的数据 CRC；
- `register(data, seed)`：寄存器式（**无**终值异或）——三个头部字段：
  - `table_crc = register(dir, 0xFFFFFFFF)`（`0x200`–`0x3FC`）；
  - `header_crc`：`register(0x28..0x7C)` → `register(0x18..0x24)` → `register(0x00..0x14)` 链式，覆盖除 `image_crc` 与自身外全部字段；
  - `image_crc = register(image, 0xFFFFFFFF)`，计算时本字段置 0。

## 紧凑镜像（64 KiB，本仓新增）

8 MiB 完整镜像里 FPGA 独占 `0x10000` 之后约 4.1 MiB，而**全部身份 + 校准字段都在
`0x10000` 之前**。紧凑镜像即 `COMPACT_SIZE = 0x10000`（64 KiB）：

- 布局、目录、CRC 链与完整镜像**完全同规则**，仅 `image_size = 0x00010000`、无
  `FPGA_IMAGE` 段（传入的 `0x0200` 段被忽略，单测锁定）；
- 用途：小体积留存 MAC / SN / Wi-Fi / BOSA / xtemp / PON EEPROM；
- ⚠️ **不可直接刷写**：恢复设备用完整 8 MiB 镜像整体写入 `factory` 卷。

## Wi-Fi 校准组装（TYFC0003，tyfza1）

Wi-Fi 校准段是原厂 `factory.uc` 组装格式的逐字节移植，字段全部来自
`config_hs.xml`：

1. 魔数 `TYFC0003`（8B）；
2. 5 个 RX gaingap 单字节：`HW_RX_GAINGAP_{2G_CCK,2G_OFDM,5GL,5GM,5GH}_AB`（十进制转字节）；
3. 每 path A/B 的 `HW_THERMAL_{A,B}_WLAN{n}`（2B）与 `HW_XCAP_WLAN{n}_AX`（1B）；
4. band × path 的 TSSI hex 数组：`HW_{2G_CCK,2G_BW40_1S,5G_BW40_1S}_TSSI_{A,B}`；
5. 6B MAC：BASE_MAC 整体加 `2 - radio_index`（tyfza1：5G wlan0 为 **+2**，2G wlan1 为 **+1**）；
6. `HW_2G_CCK_TSSI_{GAIN,CW}_DIFF_{A,B}` 与 `HW_{2G,5G}_TSSI_{GAIN,CW}_DIFF_{A,B}` hex 数组；
7. 尾部 112 B：种子 `0` 与 `HW_5G_TSSI_CW_DIFF_A` 最后一字节，各重复 4 个 width × 14 B，width 1 全 0。

> ⚠️ **PT1570 的 Wi-Fi 校准字段在 `mtd3_static_conf.bin`**（`/** NV_CFG **/` +
> `KEY=VALUE\0` 列表，实测 19 个字段：gaingap×5、thermal×2×2、xcap×2、TSSI 表×6），
> 但**缺少第 6/7 步的 TSSI GAIN/CW DIFF 字段**，无法按原厂格式完整组装，故工具
> 不生成（不猜数据）。NV_CFG 可**整段原样存档**进 factory.bin：tag `0x0103`
> `WIFI_NV_CFG`（偏移 `0x7000`，容量 `0x400`）——GUI「Wi-Fi NV_CFG (mtd3)」行
> 载入，CLI `--wifi-nvcfg <file>`，载入时校验魔数 `72 D6 EF 41` + `/** NV_CFG **/`
> 标记；WLAN 驱动直接读 NV 分区（推定），此段为存档用途。另注意 PT1570 的 wlan
> 映射与 tyfza1 **相反**（`HW_WLAN0_FEM_TYPE=17`=2G、`HW_WLAN1_FEM_TYPE=238`=5G），
> mtd4 日志显示其 2G 芯片同为 `rtl8192xbe`、参数从 rootfs `/etc/conf/rtl8192xbe/...` 下载。

## PON 模块 EEPROM（A2h）存档（tag 0x0102，本仓新增）

env 机型的 PON 口校准在光模块内部。硬件事实（PT1500 DTB + 启动日志）：

| 项 | 值 |
|---|---|
| I2C 控制器 | `cortina,ca77xx-i2c`，`0xf4329160`（0x28 字节），100 kHz，GPIO_204/205 |
| 总线 | `/dev/i2c-0`（原厂内核已开 i2c-dev） |
| A0h（Base ID） | 7 位地址 `0x50` |
| **A2h（DDMI + 校准）** | **7 位地址 `0x51`**（启动日志 `A2 use devID:0x51`） |
| 基页 | A2h `0x00`–`0x7F`；页选择字节在 `0x7F`，切页后读 `0x80`–`0xFF` |
| 校验 | `0x5F = sum(0x00..0x5E) & 0xFF`（rtkbosa 原话） |
| 关键系数 | `0x38-0x3B` Rx_PWR 四系数、`0x3C-0x4B` Tx_I/Tx_PWR/T/V 斜率偏移（S16 大端） |
| 厂商私有 | 基页 `0xB1/0xB6/0xBB/…`；Table 0x80：`0xD5`=APD_OFFSET、`0xE1/E2`=APD_RAMP_TGT、`0xE8/E9`=APD_OFFSET-ADC；Table 0x81：`0xB6`=APD LUT enable；T2/T3 各多处 |

dump 方法（**在原厂固件上做**，它自带 i2c-dev）：

```sh
i2cdump -y -f 0 0x51                                   # A2h 基页 256B
i2cset -y -f 0 0x51 0x7F 0x80 && i2cdump -y -f 0 0x51  # 私有页 0x80
i2cset -y -f 0 0x51 0x7F 0x81 && i2cdump -y -f 0 0x51  # 私有页 0x81
i2cset -y -f 0 0x51 0x7F 0x02 && i2cdump -y -f 0 0x51  # T2
i2cset -y -f 0 0x51 0x7F 0x03 && i2cdump -y -f 0 0x51  # T3（0xDB 为表内校验）
i2cset -y -f 0 0x51 0x7F 0x00                          # 回基页
# 也可用原厂 diag：diag i2c … / laser param --dump current / laser dumpdata --show
```

工具把转储作为 `PON_MODULE_EEPROM`（tag `0x0102`，偏移 `0x6000`，容量 `0x400`）
写入完整 / 紧凑镜像，存储布局：`0x000-0x0FF` 基页、`0x100`/`0x180`/`0x200`/`0x280`
起依次为页 `0x80` / `0x81` / T2 / T3（各 128 B，可选）。载入时自动校验 CC_EXT
（≥0x60 字节时），不对则拒绝并给出计算值与存储值。GUI 在「PON 模块 EEPROM (A2h)」
行**载入…**；CLI 用 `--pon-eeprom <dump.bin>`。

> ⚠️ **读得到 ≠ 用得上**：第三方系统要让 PON 上线还需整套闭源驱动栈
> （`ca-ne.ko` / `ca-rtk.ko` / `cli-drv.ko` / `libscfg.ko` + `ca-plat-kernel.ko`，
> vermagic 绑死 `5.10.70 SMP aarch64`）。本功能定位是**存档与迁移**。先 dump 再
> 刷机；刷机过程**不要 `mtd erase ubi_device`**（会连设备身份文件一起丢）。

## 用法

### 图形界面

1. 顶栏选机型（默认天邑 TYFZA1）；
2. 选 / 拖入 `mtd1_env.bin`（env 机型必需）、可选 `mtd2_env2.bin`、
   `mtd4_ubi_device.bin`（必需）——文件就位后自动在后台线程提取（大镜像不卡界面）；
3. 查看身份信息（含派生 MAC 表：5G=base+2、2.4G=base+1，仅 tyfza1）、校准段
   （可单独导出）、UBI 卷列表；「PON 模块 EEPROM (A2h)」行可载入 dump；
4. 「生成 factory.bin…」存 8 MiB，或「导出紧凑镜像 (64 KiB)…」；段表（偏移 /
   大小 / CRC32）直接显示。

### 命令行

```bash
rtl9617c-factory build --model tyfza1 --env mtd1_env.bin --env2 mtd2_env2.bin \
    --device mtd4_ubi_device.bin -o factory.bin
rtl9617c-factory build --model tyfza1 ... --compact -o factory-compact.bin
rtl9617c-factory build --model pt1570 ... --pon-eeprom pon-a2h-dump.bin -o factory.bin
rtl9617c-factory dump  --model tyfza1 --env mtd1_env.bin --device mtd4_ubi_device.bin

# 备份分析：env 变量 / UBI 卷 / UBIFS 文件 / XML 字段 / 校准内容 / 内嵌 MAC 定位
rtl9617c-factory-inspect mtd1_env.bin mtd2_env2.bin mtd4_ubi_device.bin [model]
```

## 与上游 CLI 的一致性

GUI 与 CLI 共用同一份核心代码（`src/` 下 `env` / `ubi` / `ubifs` / `xml` / `calib`
/ `profile` / `layout` / `crc` 八个模块，上游逻辑未改动），两者产出的 factory.bin
除 header `0x18` 处 8 字节 `generation` 时间戳外**逐字节相同**。本仓相对上游新增：
pt1570 机型（model id 3）、紧凑镜像（`--compact` / `build_compact`）、
`PON_MODULE_EEPROM`（tag `0x0102`）、`inspect` 分析工具。

## 构建与运行

```bash
cargo build --release --locked
# Windows 本地构建需要 MSVC Build Tools（提供 link.exe）；
# Linux 需窗口后端：cargo build --release --features linux-x11
```

仓库附带 `Cargo.lock`，锁定 eframe / egui **0.36.2**（本代码所针对的 API）。上游
eframe 0.35→0.36 有破坏性变更（`App::update` → `App::ui`、`Frame::group` →
`ui.group`），升级依赖时请同步核对。`dropped_files` 在 0.36 起改为
`Arc<dyn DroppedFile>` trait 对象，路径用 `path()` 方法读取（直接返回 `&Path`）。

### GitHub Actions

`.github/workflows/build.yml`（h3c-factory 三平台模式）：

- **触发**：任意 push / PR / 手动 workflow_dispatch；
- **矩阵**：Windows x64（zip）、macOS arm64（tar.gz）、Linux x64（装
  `libx11-dev`/`libxkbcommon-dev` 后 `--features linux-x11`）；Windows 作业先跑
  `cargo test --locked --lib`；
- **产物**：每平台压缩包内含 GUI / CLI / inspect 三个可执行文件；
- **发布**：push / PR → `nightly`（prerelease），手动 → `Latest`，同名覆盖更新。

### 界面固定亮色主题（白色）

与 h3c-factory 相同的三件套，缺一不可（均对 eframe/egui 0.36.2 逐行核实）：

1. **重写 `App::clear_color` 返回纯白**——eframe 默认清屏色是硬编码深色，egui 0.36
   传给 `App::ui` 的根 `Ui` 自己不画背景，不改这里整个窗口发黑；
2. **`apply_light_theme` 把 dark / light 两个 style 都写成亮色**——`theme_preference`
   默认 `System`，系统深色时 Ui 会一直按 dark style 取色；顺带把
   `weak_text_color` 钉成 110、三态按钮底色拉开成 238/205/170；
3. **`ui.set_style(...)` 钉住当前帧**——根 `Ui` 的 style 帧开始就被快照，只改 ctx
   首帧会闪深色。

已知限制：eframe 0.36.2 不转发 `ViewportCommand::SetTheme`，窗口标题栏仍跟随系统
主题（仅装饰）。

## 目录结构

```
rtl9617c-factory-gui/
├── Cargo.toml
├── Cargo.lock                 # 锁定 egui/eframe 0.36.2
├── LICENSE                    # GPL-2.0-only
├── .github/workflows/build.yml
└── src/
    ├── lib.rs        # 挂载核心模块
    ├── calib.rs      # Wi-Fi 校准 TYFC0003 组装 + A2h 转储校验（上游 + 本仓）
    ├── crc.rs        # CRC32：zlib 式 + 寄存器式（上游）
    ├── env.rs        # U-Boot env 解析（上游）
    ├── layout.rs     # RTLFACT1 布局 / build / build_compact / summary（上游 + 本仓）
    ├── profile.rs    # 机型提取：Tyfza1 / Pt1500H10 / Pt1570（上游 + 本仓）
    ├── ubi.rs        # UBI 容器解析（上游）
    ├── ubifs.rs      # UBIFS 文件提取（上游）
    ├── xml.rs        # config_hs.xml / config.xml 字段读取（上游）
    └── bin/
        ├── cli.rs          # 上游 CLI（build/dump，--compact/--pon-eeprom 为本仓新增）
        ├── inspect.rs      # 备份分析工具（本仓）
        └── gui/
            ├── main.rs     # egui 图形界面（本仓）
            └── font.rs     # CJK 字体安装（自 h3c-factory 移植）
```

## 许可证

**[GPL-2.0-only](LICENSE)**。

核心模块来自上游 `rtl9617c-factory`（GPL-2.0-only）；`src/bin/gui/font.rs` 移植自
`h3c-factory`（其源自 fiberhome-factory，同为 GPL-2.0-only），故本项目沿用同一
许可证。若需改用其他许可证，请先移除所有源自上游的代码。感谢原作者的工作。
