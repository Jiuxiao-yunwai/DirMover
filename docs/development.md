# DirMover 开发与发布规范

## 分支模型

项目使用两个主要分支：

- `main`：只保存已经正式发布的稳定版本，不直接开发。
- `develop`：日常集成分支，所有功能、修复和性能优化都在这里完成。

开发完成前，不把 `develop` 合并到 `main`。临时分支如确有需要，应从 `develop` 创建并合回 `develop`。

## 版本模型

未正式发布的版本使用：

```text
<目标正式版本>-dev.<阶段序号>
```

阶段序号只在生成新测试包或完成明显开发阶段时增加，不随每次提交增加。例如：

```text
2.2.0-dev.1  开始开发
2.2.0-dev.2  完成核心流程
2.2.0-dev.3  完成恢复与最终测试
2.2.0        正式发布
```

当前周期：

```text
main     2.2.2
develop  2.2.2
目标     2.2.2
```

若 `2.2.0` 发布后继续开发新功能，下一个开发版本为 `2.3.0-dev.1`；若只做补丁修复，则使用 `2.2.1-dev.1`。

## 版本文件

修改版本号时，以下文件必须保持一致：

```text
package.json
package-lock.json
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
```

`src-tauri/Cargo.lock` 是生成的 Rust 锁文件，Rust 包版本变化时也必须同步更新。

## 日常开发流程

```bash
git switch develop
git pull --ff-only
# 修改、测试、提交
```

Codex 或其他自动化工具开始修改前必须确认当前分支。未经用户明确要求，不得自动合并 `main`、创建标签、推送分支或发布 GitHub Release。

## 合并前质量门槛

```powershell
npm run typecheck
npm run lint
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo build --release --manifest-path src-tauri/Cargo.toml
```

所有检查通过后，先在 `develop` 将版本从 `2.2.0-dev.N` 改为正式版本，再按明确的发布请求执行：

```bash
git switch main
git pull --ff-only
git merge --ff-only develop
git push origin main
git tag -a v2.2.0 -m "DirMover 2.2.0"
git push origin v2.2.0
```

正式发布完成后回到 `develop`，开始下一个开发版本。创建标签、推送和 GitHub Release 都需要用户明确授权。
