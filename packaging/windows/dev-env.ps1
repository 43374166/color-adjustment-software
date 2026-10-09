# fdxmp Windows 开发环境（本机无 MSVC 工具链时的替代方案）
#
# 用法（在当前 PowerShell 会话中生效）：
#   . .\packaging\windows\dev-env.ps1
#   cargo clippy --workspace --all-targets -- -D warnings
#
# 原理：
#   1. 使用 Rust 自带的 GNU 宿主工具链（rust-mingw 提供链接器）；
#   2. 使用本地缓存的 MinGW-w64 工具链提供 gcc / dlltool / as；
#   3. 通过 RUSTUP_TOOLCHAIN 让后续裸 `cargo` 命令走该工具链。
#
# 首次准备（仅需一次）：
#   rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal
#   # MinGW-w64 工具链解压到 $env:USERPROFILE\.cache\fdxmp-tools\mingw64（见 docs/BUILD.md）

$toolchain = "stable-x86_64-pc-windows-gnu"
$mingwBin = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64\bin"

$cargoPath = rustup which cargo --toolchain $toolchain 2>$null
if (-not $cargoPath) {
    Write-Warning "缺少工具链 $toolchain，请执行：rustup toolchain install $toolchain --profile minimal"
    return
}

$tcRoot = Split-Path (Split-Path $cargoPath -Parent) -Parent
$selfContained = Join-Path $tcRoot "lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained"
if (-not (Test-Path $selfContained)) {
    Write-Warning "缺少 rust-mingw 自包含目录：$selfContained"
}
if (-not (Test-Path $mingwBin)) {
    Write-Warning "缺少 MinGW-w64 工具链：$mingwBin（安装方式见 docs/BUILD.md）"
}

# 顺序很重要：真实 MinGW-w64 的 gcc 必须排在 rust-mingw 自包含目录之前，
# 否则 rustc 会用到只带链接器、没有 crt 库的自包含 gcc。
$prepend = @()
if (Test-Path $mingwBin) {
    $prepend += $mingwBin
}
if (Test-Path $selfContained) {
    $prepend += $selfContained
}
if ($prepend.Count -gt 0) {
    $env:PATH = (($prepend -join ';') + ';' + $env:PATH)
}
$env:RUSTUP_TOOLCHAIN = $toolchain

# C: 盘空间紧张（系统盘只剩几 GB），构建/测试的临时文件统一放到 F: 的工作区。
$tempDir = Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) ".tmp"
if (-not (Test-Path $tempDir)) {
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
}
$env:TEMP = $tempDir
$env:TMP = $tempDir

# rsraw（RAW 解码）构建环境：bindgen 需要 libclang + MinGW 头文件，
# 且必须显式给出 MinGW 目标三元组，否则 stdlib.h 解析失败。
$libclang = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\pylibs\clang\native"
$mingwInclude = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64\include"
$gccInclude = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64\lib\gcc\x86_64-w64-mingw32\16.2.0\include"
if (Test-Path (Join-Path $libclang "libclang.dll")) {
    $env:LIBCLANG_PATH = $libclang
}
if ((Test-Path $mingwInclude) -and (Test-Path $gccInclude)) {
    $env:CPATH = "$mingwInclude;$gccInclude"
    $env:BINDGEN_EXTRA_CLANG_ARGS = "--target=x86_64-w64-windows-gnu"
}

# HEIF（libheif-rs）构建环境：pkg-config + 本地 libheif 1.23
$pkgConfigDir = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64\lib\pkgconfig"
if (Test-Path $pkgConfigDir) {
    $env:PKG_CONFIG_PATH = $pkgConfigDir
}

Write-Host "fdxmp 开发环境已就绪" -ForegroundColor Green
Write-Host "  工具链：$toolchain（$(rustup run $toolchain rustc --version)）"
Write-Host "  MinGW ：$mingwBin"
Write-Host "  后续可直接使用：cargo build / cargo test / cargo clippy / cargo run -p fdxmp-win-ui"
