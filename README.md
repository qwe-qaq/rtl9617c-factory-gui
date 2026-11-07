# RTL9617C Factory 图形工具

RTL9617C 平台 Factory 镜像生成工具的 **Rust + egui 图形版**，逻辑与上游
[rtl9617c-factory](https://github.com/lyq1996/rtl9617c-factory) CLI 完全一致
（核心模块逐文件复用），界面模式参考同系列的 `h3c-factory`（fiberhome-factory
系）。

> ⚠️ 仅供 **研究 / 备份恢复 / 自有设备维护** 使用，与 Realtek / 设备厂商无任何
> 关联。修改设备出厂数据可能导致设备无法启动，请仅在自己拥有的设备上操作。

## 功能

- **机型**：`tyfza1`（天邑 TYFZA1）/ `pt1500-h10`（友华 PT1500-H10）
- **数据来源**：U-Boot env 分区（`mtd1_env.bin`，可选 `mtd2_env2.bin`）+
  `ubi_device` 容器镜像（`mtd4_ubi_device.bin`）；支持把文件**拖进窗口**自动分类
- **提取（= CLI `dump`）**：后台线程解析 env / UBI 容器 / UBIFS，界面显示
  BASE_MAC / EPON_MAC / PON_SN / DEVICE_SN / MAC 数量、各校准段大小、UBI 卷列表
- **生成（= CLI `build`）**：输出 8 MiB `RTLFACT1` factory.bin，并显示各段
  偏移 / 大小 / CRC32 段表；各校准段（Wi-Fi TYFC0003、BOSA、xtemp、FPGA）
  可单独导出留档

生成的 bin 由 U-Boot 写入 UBI static 卷 `factory`（`ubiupdatevol` 或
`ubi write`），镜像格式规范见上游 README。

## 构建

需要 Rust 工具链（rustup）；Windows 上还需要 **MSVC Build Tools**（提供
`link.exe`，没装会报 `linker 'link.exe' not found`）：

```bash
cargo build --release
```

产物在 `target/release/`：

| 文件 | 说明 |
|---|---|
| `rtl9617c-factory-gui.exe` | 图形界面（release 下无控制台窗口） |
| `rtl9617c-factory.exe` | 原上游 CLI（build / dump） |

Linux 构建需窗口后端：`cargo build --release --features linux-x11`。

## GitHub Actions

`.github/workflows/build.yml`（同 h3c-factory 的三平台模式）：

- **触发**：任意 push / PR / 手动 workflow_dispatch
- **矩阵**：Windows x64（zip）、macOS arm64（tar.gz）、Linux x64
  （装 `libx11-dev`/`libxkbcommon-dev` 后用 `--features linux-x11` 构建）
- **产物**：每个平台的压缩包内同时含 CLI 与 GUI 两个可执行文件
- **发布**：构建完自动发 Release——push/PR 触发的进 `nightly`（prerelease），
  手动触发的进 `Latest`；同名发布会被 `overwrite_files` 覆盖更新

Windows 本地没装 MSVC Build Tools 时，直接用 Actions 产物即可。

## 用法

1. 选择机型（默认天邑 TYFZA1）；
2. 选 / 拖入 `mtd1_env.bin`（pt1500-h10 必需，tyfza1 可省）、可选 `mtd2_env2.bin`、
   `mtd4_ubi_device.bin`（必需）——文件载入后自动开始提取；
3. 查看提取出的身份信息与校准段；
4. 点「生成 factory.bin…」保存 8 MiB 镜像，段表直接显示在窗口里。

CLI 用法与上游一致：

```bash
rtl9617c-factory build --model tyfza1 --env mtd1_env.bin --env2 mtd2_env2.bin \
    --device mtd4_ubi_device.bin -o factory.bin
rtl9617c-factory dump  --model tyfza1 --env mtd1_env.bin --device mtd4_ubi_device.bin
```

## 与 CLI 的一致性

GUI 与 CLI 共用同一份核心代码（`src/` 下 `env` / `ubi` / `ubifs` / `xml` /
`calib` / `profile` / `layout`，未改动任何逻辑），两者产出的 factory.bin 除
header `0x18` 处 8 字节 generation 时间戳（生成时刻的 Unix 秒）外逐字节相同。

## 目录结构

```
rtl9617c-factory-gui/
├── Cargo.toml
└── src/
    ├── lib.rs          # 挂载核心模块
    ├── bin/
    │   ├── cli.rs      # 上游 CLI（原样移植，模块引用改为 lib）
    │   └── gui/
    │       ├── main.rs # egui 图形界面
    │       └── font.rs # 系统 CJK 字体安装（自 h3c-factory 移植）
    ├── calib.rs        # Wi-Fi 校准 TYFC0003 组装（上游）
    ├── crc.rs          # CRC32（上游）
    ├── env.rs          # U-Boot env 解析（上游）
    ├── layout.rs       # RTLFACT1 镜像布局（上游）
    ├── profile.rs      # 机型提取逻辑（上游）
    ├── ubi.rs          # UBI 容器解析（上游）
    ├── ubifs.rs        # UBIFS 文件提取（上游）
    └── xml.rs          # config_hs.xml / config.xml 字段读取（上游）
```

## 许可证

**GPL-2.0-only**。上游 `rtl9617c-factory` 为 GPL-2.0-only；`src/bin/gui/font.rs`
移植自 `h3c-factory`（其源自 fiberhome-factory，同为 GPL-2.0-only），故本项目
沿用同一许可证。
