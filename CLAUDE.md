# Keyboard Lock OSD

## 项目概述

轻量级 Windows 键盘锁定状态 OSD 显示工具，基于 Tauri v2 + React 构建。

## 版本管理

本项目版本号存在于三个位置，发布时必须同步更新：

- `package.json` → `"version": "x.y.z"`
- `src-tauri/Cargo.toml` → `version = "x.y.z"`
- `src-tauri/tauri.conf.json` → `"version": "x.y.z"`

**重要**：tauri-action 构建时使用 `tauri.conf.json` 的版本号，如果不同步会导致：
- 用户无法收到更新通知（updater 认为最新版本未变）
- 安装包文件名错误

## 发布流程

### 正式版本发布

使用发布脚本自动同步版本号：

```powershell
.\scripts\release.ps1 0.1.7
```

脚本会自动：
1. 更新三个版本文件（package.json、Cargo.toml、tauri.conf.json）
2. 本地构建验证（pnpm run build + cargo check）
3. 提交更改
4. 打 tag
5. 推送到远程仓库
6. CI 自动构建并发布到 GitHub Releases

> 详细的发布规则、签名密钥管理和 CI 配置见 `AGENTS.md`。

## 技术栈

- **前端**：React 19 + TypeScript + Vite
- **后端**：Rust + Tauri v2
- **包管理**：pnpm
- **CI/CD**：GitHub Actions

## 常用命令

```bash
# 开发
pnpm tauri dev

# 构建
pnpm tauri build

# 前端开发服务器
pnpm dev
```
