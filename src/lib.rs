//! RTL9617C factory 镜像核心库。
//!
//! 模块与上游 `rtl9617c-factory` CLI 完全一致（逐文件拷贝，未改逻辑），
//! 这里只是把它们挂成 lib，供 `bin/cli.rs` 与 `bin/gui.rs` 共用：
//! - `env`   U-Boot env 分区（crc32 + flags + name=value 列表，多分区取 flags 大者）
//! - `ubi`   ubi_device 容器（EC/VID 头解析、同 (vol_id,lnum) 取 sqnum 最新、卷表定位）
//! - `ubifs` UBIFS 文件提取（superblock/master/TNC，none/LZO/zlib 压缩）
//! - `xml`   config_hs.xml / config.xml 字段读取
//! - `calib` Wi-Fi 校准（TYFC0003 组装）与 EPON LLID MAC
//! - `profile` 机型（tyfza1 / pt1500-h10）字段提取
//! - `layout` RTLFACT1 镜像布局（header + 目录 + 字段区，CRC 链）
//! - `crc`   zlib 式 CRC32 与寄存器式 CRC32

pub mod calib;
pub mod crc;
pub mod env;
pub mod layout;
pub mod profile;
pub mod ubi;
pub mod ubifs;
pub mod xml;
