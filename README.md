# DirMover

轻量的 Windows 目录迁移工具：使用 WPF Fluent 现代界面，直接读取 NTFS 主文件表（MFT）统计目录大小，再把选中的子文件夹迁移到其他 NTFS 磁盘，并在原位置创建目录联接（junction）。

## 安全设计

- 只允许迁移子文件夹，不允许迁移磁盘根目录、整个 AppData、Program Files、ProgramData 或用户目录。
- 目标必须在另一块 NTFS 磁盘，且不能已经存在。
- 含软链接/目录联接、无法读取文件或空间不足时拒绝迁移。
- 先复制并核对文件数量和总字节数，再切换原路径；中途失败会尽量自动回滚。
- 迁移记录原子写入 `%USERPROFILE%\.dirmover\moves.json`，每次启动重新核对链接。
- “链接检测”只检查所选目录的直接子项，不递归进入普通文件夹或链接目标。
- 支持把有效迁移还原到原磁盘。
- 空间分析会像 WizTree 一样顺序读取 NTFS MFT，并显示逻辑大小和磁盘占用；非 NTFS 或 MFT 扩展记录无法解析时自动回退到普通文件遍历。
- 启动时不自动扫描；只有用户点击扫描后才执行，并在需要 MFT 权限时请求管理员权限。
- 启动或切换顶部目录时会立即列出该位置的一级文件夹，但不遍历内容、不计算大小；已有的本次运行扫描缓存会直接恢复。
- 应用数据统一保存在隐藏目录 `%USERPROFILE%\.dirmover`。
- 界面使用 .NET 10 内置 WPF Fluent 主题和虚拟化表格，无第三方 UI 依赖；扫描与迁移继续在后台执行。
- 程序启动时立即检查并创建隐藏配置目录 `%USERPROFILE%\.dirmover`；同一次运行中按扫描路径复用内存缓存，也可点击“重新扫描”强制刷新。
- 默认把迁移内容统一托管到盘符最靠后的固定 NTFS 分区下的 `DirMoverData`，并按 `AppData\Local`、`AppData\LocalLow`、`AppData\Roaming`、`Program Files` 等来源自动分类。
- 全局托管位置保存在 `%USERPROFILE%\.dirmover\settings.json`；Local、LocalLow、Roaming、Program Files 等分类均可在设置页单独指定位置，每次迁移也可以临时切换为自定义位置。
- 如果只检测到系统盘，默认托管会停用并提示用户；迁移目标仍必须位于另一个 NTFS 分区。
- 扫描进度会按 MFT 读取、目录索引和大小统计阶段显示；兼容扫描按完成的一级文件夹计数。
- 扫描发现的直接软链接和目录联接写入 `%USERPROFILE%\.dirmover\links.json`。
- 空间表格只显示文件夹、磁盘占用、文件数和状态；目录链接目标在鼠标悬停时显示。表格支持像素滚动、虚拟化、右键在资源管理器中打开以及复制选中内容。

迁移 Program Files 中的应用前，请退出相关应用和服务。少数带驱动、反作弊、系统服务或自校验机制的软件即使使用目录联接也可能不支持迁移。

## 构建

```powershell
dotnet build
dotnet run -- --self-test
dotnet publish -c Release -r win-x64 --self-contained false -p:PublishSingleFile=true -o artifacts
```

发布后的 `artifacts\DirMover.exe` 是单文件程序，需要机器上有 .NET 10 Desktop Runtime。
