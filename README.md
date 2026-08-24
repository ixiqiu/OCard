# OCard

跨平台 DIT 素材备份管理工具（Tauri 2 · Rust 核心 + React Web UI），复刻 KOCARD 核心功能的内部 DIT 素材备份管理工具，落实《摄影前后期技术规格和数据管理流程规范 OB/GF 001—2026（第2版）》。

> 依据 PRD v2 草案（2026-08-24）：跨 Windows/macOS/Linux，技术栈 Tauri 2。

## 功能总览（对应 PRD §5）

| 模块 | 说明 |
|---|---|
| 设备登记（§5.1） | 相机（型号/机位/使用者代称 → 自动编码如 `DJIRonin4D_B_ZS`）、存储卡（一卡一机），登记表存 NAS 项目 `.ocard/devices.json` |
| 项目管理（§5.2） | 新建项目向导：`YYYYMMDD_项目名`，工况 A（6 目录）/ 工况 B（待分类 + 自定义分类 + 精选/待修已修 + 其他），列表页显示状态 |
| 拷卡引擎（§5.3） | 可移动卷自动检测（三平台轮询）、双确认、多目的地并行写、xxHash3-64 流式校验、回读比对、manifest 落盘、断点续传、逐文件容错 |
| 分类工作台（§5.4） | 网格预览 + 缩略图缓存、键盘流（数字键分类 / P 精选复制 / O 其他 / D 两段式删除）、连拍自动折叠、回收站可恢复 |
| 本地 AI 选片（§5.5） | 清晰度（拉普拉斯方差）+ 曝光直方图纯算法评分、时间邻近连拍聚类、质量排序荐优；人脸/闭眼经 `AiBackend` 抽象（`full-ai` feature 启用 ONNX Runtime，默认诚实降级） |
| 转码引擎（§5.6） | ffmpeg sidecar：NVENC/QSV/AMF/VideoToolbox/VAAPI 自动探测、回落 x264/x265；代理转码 + 归档 HEVC 10-bit 三档 |
| 交付打包（§5.7） | 按半天自动分包、不压缩、交付清单 + 待上传列表（人工上传百度网盘） |
| 成片命名校验（§5.8） | `时间日期_片名_分辨率_用途_版本` 正则校验，识别预览版（720p）与成品 |
| 审计日志（§5.9） | 追加式 JSON Lines 日志，按机器 ID 分文件，读取时合并多工作站记录 |

## 架构（PRD §6）

```
OCard (Tauri 2)
├── UI 层（Web，React + 虚拟化网格）：项目列表 / 拷卡 / 分类工作台 / 转码 / 交付 / 设置登记
├── Rust 核心（tauri commands + 后台任务）
│   ├── copy_engine    拷贝+校验+续传（单读多写，流式）
│   ├── hash           xxHash3-64 流式计算
│   ├── project_store  项目模板/命名规则/状态（规范的代码化）
│   ├── media_indexer  缩略图/EXIF/拍摄时间提取（缓存于 .ocard/）
│   ├── culling        ONNX Runtime 聚类/评分/闭眼（后台队列，默认 feature 关闭）
│   ├── transcode      ffmpeg sidecar 队列（按平台选硬件编码器）
│   ├── packaging      半天分包+清单
│   └── audit_log      追加式操作日志
└── 存储：一切项目级状态放 NAS 项目夹内 .ocard/（JSON + 缩略图缓存）
```

多工作站协同：无服务端、无文件锁，每台工作站写自己的追加日志（机器 ID 命名），读取时合并（PRD §6.3）。

## 开发

```bash
# 前端依赖
npm install

# 本地开发（需 Rust toolchain + 系统依赖）
npm run tauri dev

# 图标（从 src-tauri/app-icon.png 生成全套）
cd src-tauri && npx tauri icon app-icon.png

# 测试与构建
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

> 由于本仓库在空间受限环境开发，**所有构建/测试由 GitHub Actions 完成**（`.github/workflows/ci.yml`，三平台矩阵），本地不执行 cargo/npm 构建。

## 测试（PRD §8）

- 单元：命名规则、建夹模板、manifest、半天分包、拷贝/校验/续传、哈希、审计合并（Rust 侧，三平台 CI 全跑）
- 集成：用临时目录模拟存储卡，含故意损坏文件验证校验拦截、断点续传
- 平台矩阵：Windows / macOS / Linux 三平台各构建并跑测试
- 打包：Windows msi + nsis、macOS dmg、Linux AppImage + deb

## 里程碑（PRD §7）

- M1 拷卡底座：项目管理/建夹、设备登记、拷卡引擎（HASH/多目的地/断点续传）、审计日志 ✅ 已实现
- M2 分类交付：缩略图索引、分类工作台（键盘流）、回收站、交付打包+清单 ✅ 已实现
- M3 智能与转码：AI 选片（聚类/评分/荐优，闭眼检测为可选 feature）、转码引擎（代理+归档）、成片命名校验 ✅ 已实现

## 分发

- Windows：msi / nsis
- macOS：dmg（签名可后置）
- Linux：AppImage / deb
- ffmpeg 与 ONNX 模型随包分发（ONNX 模型需放置于可执行文件旁 `models/face_det.onnx`）
