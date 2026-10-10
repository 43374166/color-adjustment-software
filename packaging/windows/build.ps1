# fdxmp Windows 打包：构建 win_ui → 复制 exe → 运行时 DLL → 安装包（NSIS / IExpress）→ 7z 分发包。
#
# 用法：
#   powershell -File packaging/windows/build.ps1
#   powershell -File packaging/windows/build.ps1 -SkipInstaller -SkipBuild
param(
    [switch]$SkipInstaller,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$dist = Join-Path $root "dist/windows"
$version = "0.2.1"
$cargoToml = Get-Content (Join-Path $root "Cargo.toml") -Raw
if ($cargoToml -match '(?m)^version\s*=\s*"([^"]+)"') { $version = $Matches[1] }

# 优先使用仓库内 .tmp 的 7-Zip（无系统安装），否则退回 PATH 上的 7za。
$sevenZip = Join-Path $root ".tmp/tools/7zip/x64/7za.exe"
if (-not (Test-Path $sevenZip)) { $sevenZip = Join-Path $root ".tmp/tools/7zip/7za.exe" }
if (-not (Test-Path $sevenZip)) {
    $found = Get-Command 7za -ErrorAction SilentlyContinue
    if ($found) { $sevenZip = $found.Source } else { $sevenZip = $null }
}

Push-Location $root
try {
    if (-not $SkipBuild) {
        # GPU 调色（wgpu）+ RAW（rsraw/LibRaw）+ HEIF（libheif）+ AI 蒙版（ONNX Runtime）
        cargo build --release -p fdxmp-win-ui --features gpu,raw,heif,ai
    }

    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item (Join-Path $root "target/release/fdxmp-win-ui.exe") $dist -Force

    # LibRaw / libheif 是 C/C++，按 exe 导入表递归收集 MinGW 运行时 DLL
    & (Join-Path $PSScriptRoot "deploy-runtime.ps1") -Exe (Join-Path $dist "fdxmp-win-ui.exe")

    # 动态加载的 ONNX Runtime（不在导入表里，需显式复制）
    $onnxRuntime = Join-Path $root ".tmp/tools/onnxruntime/onnxruntime.dll"
    if (Test-Path $onnxRuntime) {
        Copy-Item $onnxRuntime $dist -Force
        Write-Host "已复制 onnxruntime.dll（AI 蒙版运行时）"
    }
    else {
        Write-Host "未找到 .tmp/tools/onnxruntime/onnxruntime.dll：AI 蒙版将不可用（可用 packaging/windows/fetch-onnxruntime.ps1 获取）" -ForegroundColor Yellow
    }

    # 安装脚本写入 UTF-8 BOM，保证 Windows PowerShell 5.1 下中文不乱码。
    $installSource = Get-Content (Join-Path $PSScriptRoot "install.ps1") -Raw
    [System.IO.File]::WriteAllText(
        (Join-Path $dist "install.ps1"),
        $installSource,
        (New-Object System.Text.UTF8Encoding($true))
    )
    Copy-Item (Join-Path $PSScriptRoot "README-dist.txt") (Join-Path $dist "README.txt") -Force

    if (-not $SkipInstaller) {
        # 优先使用本地构建好的 NSIS（.tmp/tools/nsis），其次 PATH 上的 makensis，
        # 都没有时退回 IExpress 自解压安装包。
        $localNsis = Join-Path $root ".tmp/tools/nsis/Bin/makensis.exe"
        $makensis = $null
        if (Test-Path $localNsis) {
            $makensis = $localNsis
        }
        else {
            $found = Get-Command makensis -ErrorAction SilentlyContinue
            if ($found) { $makensis = $found.Source }
        }

        if ($makensis) {
            Push-Location (Join-Path $root "packaging/windows")
            try {
                & $makensis /V2 "fdxmp.nsi"
            }
            finally {
                Pop-Location
            }
        }
        else {
            Write-Host "未检测到 makensis（NSIS），改用 IExpress 自解压安装包。" -ForegroundColor Yellow
            & (Join-Path $PSScriptRoot "make-iexpress-setup.ps1") -SourceDir $dist -Target (Join-Path $root "dist/fdxmp-setup-sfx.exe")
        }
    }

    if ($sevenZip) {
        $archive = Join-Path $root "dist/fdxmp-$version-win64.7z"
        if (Test-Path $archive) { Remove-Item $archive -Force }
        # AI 模型（约 313 MB）不进分发包，由应用内下载
        & $sevenZip a -t7z -mx=9 $archive (Join-Path $dist "*") "-xr!models" | Out-Null
        Write-Host ("7z 分发包：{0}（{1:N1} MB）" -f $archive, ((Get-Item $archive).Length / 1MB))
    }
    else {
        Write-Host "未找到 7-Zip（.tmp/tools/7zip 或 PATH），跳过 7z 打包。" -ForegroundColor Yellow
    }

    Write-Host "Windows 打包完成：$dist" -ForegroundColor Green
}
finally {
    Pop-Location
}
