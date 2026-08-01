# DirMover

DirMover 是一个轻量、高性能的 Windows 目录空间分析与安全迁移工具，基于 **Tauri 2 + Rust + Vue 3 + TypeScript** 构建。Rust 负责 NTFS 扫描、文件迁移、目录联接和本地记录，Vue 负责响应式桌面界面。

## 下载与安装

前往 [Releases](https://github.com/Jiuxiao-yunwai/DirMover/releases/latest) 下载最新版本。当前仅提供 Windows x64 版本，建议使用 Windows 10 或 Windows 11。

同一版本提供两种安装包，应用功能完全相同，请选择其中一种安装，不要同时安装：

| 安装包 | 默认安装位置 | 权限 | 适用场景 |
| --- | --- | --- | --- |
| `DirMover_*_x64-setup.exe` | `%LOCALAPPDATA%\DirMover` | 无需管理员权限 | 推荐普通用户使用 |
| `DirMover_*_x64_zh-CN.msi` | `C:\Program Files\DirMover` | 需要管理员权限 | 企业部署、组策略或标准 MSI 管理 |

安装步骤：

1. 从 Releases 下载所需安装包。
2. 退出正在运行的旧版 DirMover。
3. 双击安装包并按照中文向导完成安装。
4. 后续升级建议继续使用同一种安装包；如果需要在 EXE 与 MSI 之间切换，请先卸载原版本。

安装器会在系统缺少 Microsoft Edge WebView2 Runtime 时联网下载安装。Windows 10 和 Windows 11 通常已自带该组件。

## 功能

- 预览并扫描指定位置的一级目录，显示逻辑大小、磁盘占用和文件数量。
- 管理员模式优先顺序读取 NTFS MFT；不可用时自动回退到并行文件遍历。
- 把目录安全迁移到另一固定 NTFS 分区，并在原位置创建目录联接。
- 复制后按文件数量和字节数校验；失败时自动回滚并保留可恢复记录。
- 检查迁移状态、恢复中断操作，并把已迁移数据还原到原磁盘。
- 检测直接子项中的符号链接与目录联接，不递归进入链接目标。
- 默认托管根优先选择系统盘同一物理硬盘上分区编号最大的可用 NTFS 分区；无法识别时回退到 D 盘，没有 D 盘则回退到 C 盘。
- 左侧菜单可折叠；集中帮助页提供各模块操作说明。
- 空间扫描为可选操作，未扫描也能规划迁移；扫描期间可继续使用其他模块。
- 自动、浅色、深色三种主题；窗口大小和位置自动恢复。
- 扫描和迁移使用后台任务、进度通道和取消信号，界面保持响应。

## 安全边界

- 只允许迁移目录；禁止磁盘根目录、整个用户目录、AppData、Program Files、ProgramData 和 Windows 目录。
- 源与目标必须位于不同的固定 NTFS 分区，目标不能已经存在。
- 源目录包含链接、存在不可读项目或目标空间不足时拒绝迁移。
- 不调用 `cmd.exe` 创建链接；目录联接由 Rust 原生库完成。
- 迁移使用明确的准备、复制、校验、切换、联接、清理阶段。提交点之后会优先恢复可用的源路径。
- 不把管理员权限写入应用清单；仅在用户发起 MFT 扫描时按需重启提权。

## 数据兼容

数据继续保存在隐藏目录 `%USERPROFILE%\.dirmover`：

- `settings.json`
- `moves.json`
- `links.json`

Rust 存储层保留旧版 PascalCase JSON 结构；前端 IPC 使用独立的 camelCase DTO，因此已有记录可直接读取，界面模型也保持类型一致。写入采用临时文件、刷新落盘和原子替换。

## 使用提醒

迁移目录前请退出正在使用该目录的应用，并确认重要数据已有备份。不要迁移驱动程序、反作弊组件、系统服务或明确不支持目录联接的软件。第三方程序是否支持目录联接取决于其自身实现。

## 本地开发

需要 Node.js 20+、Rust 1.85+、Microsoft C++ Build Tools 与 WebView2。

分支、开发版本号和正式发布流程见 [开发与发布规范](docs/development.md)。日常修改在 `develop` 完成，`main` 仅保存正式发布版本。

```powershell
npm install
npm run tauri -- dev
```

质量检查：

```powershell
npm run typecheck
npm run lint
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 构建发布包

```powershell
npm run package
```

构建完成后，NSIS EXE 与 MSI 安装包会统一汇总到 `artifacts\releases\<版本号>\`。Windows 安装界面固定使用简体中文。

## 许可证

本项目基于 [MIT License](LICENSE) 开源。

更多设计说明见 [架构文档](docs/architecture.md)。
