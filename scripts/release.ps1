# 版本发布脚本
# 用法: .\scripts\release.ps1 <version>
# 示例: .\scripts\release.ps1 0.1.7

param(
    [Parameter(Mandatory=$true)]
    [string]$Version
)

$ErrorActionPreference = "Stop"

# 检查当前分支是否是 main
$currentBranch = git rev-parse --abbrev-ref HEAD
if ($currentBranch -ne "main") {
    Write-Host "错误: 当前在 $currentBranch 分支，请切换到 main 分支后重试" -ForegroundColor Red
    exit 1
}

# 检查是否在项目根目录
if (-not (Test-Path "package.json") -or -not (Test-Path "src-tauri/Cargo.toml") -or -not (Test-Path "src-tauri/tauri.conf.json")) {
    Write-Host "错误: 请在项目根目录运行此脚本" -ForegroundColor Red
    exit 1
}

# 验证版本号格式 (semver)
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Host "错误: 版本号格式不正确，应为 x.y.z (如 0.1.7)" -ForegroundColor Red
    exit 1
}

$Tag = "v$Version"

Write-Host "准备发布版本: $Version" -ForegroundColor Cyan

# 检查工作目录是否干净
$status = git status --porcelain
if ($status) {
    Write-Host "错误: 工作目录不干净，请先提交或暂存更改" -ForegroundColor Red
    exit 1
}

# 检查本地 main 是否落后于远程
Write-Host "检查远程仓库状态..." -ForegroundColor Yellow
git fetch origin
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: 无法连接远程仓库" -ForegroundColor Red
    exit 1
}
$behind = git rev-list --count "HEAD..origin/main"
if ([int]$behind -gt 0) {
    Write-Host "错误: 本地 main 分支落后远程 $behind 个提交，请先执行 git pull" -ForegroundColor Red
    exit 1
}

# 检查 tag 是否已存在
$tagExists = git tag -l $Tag
if ($tagExists) {
    Write-Host "错误: tag $Tag 已存在" -ForegroundColor Red
    exit 1
}

Write-Host "更新版本号..." -ForegroundColor Yellow

# 读取当前版本号
$packageJson = Get-Content package.json -Raw | ConvertFrom-Json
$oldVersion = $packageJson.version
if ($oldVersion -eq $Version) {
    Write-Host "错误: 新版本号与当前版本号相同 ($Version)" -ForegroundColor Red
    exit 1
}
if ([version]$Version -le [version]$oldVersion) {
    Write-Host "错误: 新版本号 ($Version) 必须高于当前版本号 ($oldVersion)" -ForegroundColor Red
    exit 1
}
Write-Host "当前版本: $oldVersion -> 新版本: $Version"

# 更新 package.json (只替换第一个匹配)
$packageContent = Get-Content package.json -Raw
$packageContent = [regex]::Replace($packageContent, '"version":\s*"[^"]*"', "`"version`": `"$Version`"", 1)
[System.IO.File]::WriteAllText((Resolve-Path "package.json"), $packageContent, [System.Text.UTF8Encoding]::new($false))

# 更新 Cargo.toml (只替换 [package] 段的 version)
$cargoLines = Get-Content src-tauri/Cargo.toml
$inPackage = $false
$cargoLines = $cargoLines | ForEach-Object {
    if ($_ -match '^\[package\]') { $inPackage = $true }
    elseif ($_ -match '^\[') { $inPackage = $false }
    if ($inPackage -and $_ -match '^version\s*=\s*"[^"]*"') {
        "version = `"$Version`""
    } else {
        $_
    }
}
[System.IO.File]::WriteAllLines((Resolve-Path "src-tauri/Cargo.toml"), $cargoLines, [System.Text.UTF8Encoding]::new($false))

# 更新 tauri.conf.json (只替换顶层 version 字段)
$tauriContent = Get-Content src-tauri/tauri.conf.json -Raw
$tauriContent = [regex]::Replace($tauriContent, '(?m)^\s*"version":\s*"[^"]*"', "  `"version`": `"$Version`"", 1)
[System.IO.File]::WriteAllText((Resolve-Path "src-tauri/tauri.conf.json"), $tauriContent, [System.Text.UTF8Encoding]::new($false))

Write-Host "版本号已更新:" -ForegroundColor Green
Write-Host "  - package.json"
Write-Host "  - src-tauri/Cargo.toml"
Write-Host "  - src-tauri/tauri.conf.json"

# 验证更新结果
Write-Host ""
Write-Host "验证版本号:" -ForegroundColor Yellow
$packageVersion = (Get-Content package.json -Raw | ConvertFrom-Json).version
$cargoVersion = Select-String -Path src-tauri/Cargo.toml -Pattern '^version\s*=\s*"([^"]*)"' | ForEach-Object { $_.Matches[0].Groups[1].Value }
$tauriVersion = (Get-Content src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version

Write-Host "  package.json: $packageVersion"
Write-Host "  Cargo.toml: $cargoVersion"
Write-Host "  tauri.conf.json: $tauriVersion"

if ($packageVersion -ne $Version -or $cargoVersion -ne $Version -or $tauriVersion -ne $Version) {
    Write-Host "错误: 版本号更新验证失败" -ForegroundColor Red
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 1
}
Write-Host "版本号验证通过!" -ForegroundColor Green

# 显示更改
Write-Host ""
git diff

# 确认提交
$confirm = Read-Host "确认提交并发布? (y/N)"
if ($confirm -ne 'y' -and $confirm -ne 'Y') {
    Write-Host "已取消" -ForegroundColor Yellow
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 0
}

# 本地构建验证
Write-Host ""
Write-Host "验证本地构建..." -ForegroundColor Yellow

Write-Host "  运行 pnpm run build..." -ForegroundColor Gray
pnpm run build
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: 前端构建失败，请修复后重试" -ForegroundColor Red
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 1
}

Write-Host "  运行 cargo check..." -ForegroundColor Gray
cargo check --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: Rust 编译检查失败，请修复后重试" -ForegroundColor Red
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 1
}

Write-Host "构建验证通过!" -ForegroundColor Green

# 提交
git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: git add 失败" -ForegroundColor Red
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 1
}

git commit -m "chore(release): 发布 $Tag"
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: 提交失败" -ForegroundColor Red
    git checkout HEAD -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
    exit 1
}

# 打 tag
git tag $Tag
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: 创建 tag 失败" -ForegroundColor Red
    exit 1
}

# 推送
git push origin main
if ($LASTEXITCODE -ne 0) {
    Write-Host "错误: 推送 main 分支失败，请手动执行:" -ForegroundColor Red
    Write-Host "  git push origin main && git push origin $Tag"
    exit 1
}

git push origin $Tag
if ($LASTEXITCODE -ne 0) {
    Write-Host "tag 推送失败，正在重试..." -ForegroundColor Yellow
    git push origin $Tag
    if ($LASTEXITCODE -ne 0) {
        Write-Host "错误: 推送 tag 失败 (main 分支已推送成功)" -ForegroundColor Red
        Write-Host "请手动执行: git push origin $Tag"
        exit 1
    }
}

Write-Host ""
Write-Host "发布成功！" -ForegroundColor Green
Write-Host "Tag: $Tag"
Write-Host "CI 将自动构建并发布到 GitHub Releases"
