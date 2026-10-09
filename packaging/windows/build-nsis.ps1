# 从源码构建 NSIS（makensis + stubs）——本机没有 NSIS 安装包、GitHub/SourceForge 不可达时的替代路径。
#
# 依赖（全部落在 F:\FD-Room\.tmp，不写入 C/D 盘）：
#   - Gitee 上的 NSIS 源码镜像（https://gitee.com/mirrors/NSIS.git）
#   - 便携版 Python（华为云镜像）+ scons（阿里云 PyPI 镜像）
#   - MinGW-w64 工具链（dev-env.ps1 提供 gcc/g++/windres/objdump）
#   - 3 处 GCC 16 兼容补丁（offsetof 常量表达式 / bool 自增）
#
# 产出：.tmp\tools\nsis\{makensis.exe, Stubs\, Include\, libwinpthread-1.dll, zlib1.dll}
#
# 用法：powershell -ExecutionPolicy Bypass -File packaging/windows/build-nsis.ps1
param(
    [switch]$ForceRebuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tools = Join-Path $root ".tmp/tools"
$nsisOut = Join-Path $tools "nsis"
$src = Join-Path $tools "nsis-src"
$py = Join-Path $tools "python312"
$zlib = Join-Path $tools "zlib-w32"

if ((Test-Path (Join-Path $nsisOut "makensis.exe")) -and -not $ForceRebuild) {
    Write-Host "NSIS 已就绪：$nsisOut\makensis.exe（-ForceRebuild 可重建）" -ForegroundColor Green
    exit 0
}

# 1) 便携 Python + scons（镜像：华为云 Python / 阿里云 PyPI）
if (-not (Test-Path (Join-Path $py "python.exe"))) {
    Write-Host "下载便携 Python 3.12（华为云镜像）…"
    $zip = Join-Path $tools "python-3.12.10-embed-amd64.zip"
    Invoke-WebRequest -Uri "https://mirrors.huaweicloud.com/python/3.12.10/python-3.12.10-embed-amd64.zip" -OutFile $zip -TimeoutSec 600
    Expand-Archive -LiteralPath $zip -DestinationPath $py -Force
    $pth = Join-Path $py "python312._pth"
    $content = (Get-Content $pth -Raw) -replace "#\s*import site", "import site"
    if ($content -notmatch "Lib\\site-packages") { $content += "`r`nLib\site-packages" }
    Set-Content -LiteralPath $pth -Value $content -Encoding ASCII
    New-Item -ItemType Directory -Force -Path (Join-Path $py "Lib/site-packages") | Out-Null
}
if (-not (Test-Path (Join-Path $py "Lib/site-packages/SCons"))) {
    Write-Host "下载 scons（阿里云 PyPI 镜像）…"
    $ua = @{ "User-Agent" = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0 Safari/537.36" }
    $indexUrl = "https://mirrors.aliyun.com/pypi/simple/scons/"
    $page = Invoke-WebRequest -Uri $indexUrl -UseBasicParsing -TimeoutSec 60 -Headers $ua
    $href = ([regex]::Matches($page.Content, 'href="([^"]+scons-[0-9.]+-py3-none-any\.whl[^"]*)"') |
        ForEach-Object { $_.Groups[1].Value } | Select-Object -Last 1)
    if (-not $href) { throw "未找到 scons wheel" }
    $absolute = [System.Uri]::new([System.Uri]$indexUrl, $href).AbsoluteUri
    $wheel = Join-Path $py "Lib/site-packages/scons.whl"
    Invoke-WebRequest -Uri $absolute -OutFile $wheel -TimeoutSec 600 -Headers $ua
    Expand-Archive -LiteralPath $wheel -DestinationPath (Join-Path $py "Lib/site-packages") -Force
    Set-Content -LiteralPath (Join-Path $py "scons.py") -Encoding ASCII -Value @"
import sys
from SCons.Script import main
sys.exit(main())
"@
}

# 2) NSIS 源码（Gitee 镜像）
if (-not (Test-Path (Join-Path $src "SConstruct"))) {
    Write-Host "克隆 NSIS 源码（Gitee 镜像）…"
    git clone --depth 1 https://gitee.com/mirrors/NSIS.git $src
}

# 3) GCC 16 兼容补丁（幂等）
$buildCpp = Join-Path $src "Source/build.cpp"
$makenssi = Join-Path $src "Source/makenssi.cpp"
$build = Get-Content $buildCpp -Raw
if ($build -match "FIELD_OFFSET\(postbuild_cmd, cmd\[") {
    $build = $build.Replace(
        "postbuild_cmd *p = (postbuild_cmd*) (new BYTE[FIELD_OFFSET(postbuild_cmd, cmd[_tcsclen(cmdstr)+!0])]);",
        "size_t cmd_chars = _tcsclen(cmdstr) + 1;`r`n  postbuild_cmd *p = (postbuild_cmd*) (new BYTE[FIELD_OFFSET(postbuild_cmd, cmd) + cmd_chars * sizeof(TCHAR)]);"
    )
}
if ($build -match "FIELD_OFFSET\(PROMPT_FILEPATH_DATA, Path\[") {
    $build = $build.Replace(
        "TinyGrowBuf inputbuf((IGrowBuf::size_type) (cb = FIELD_OFFSET(PROMPT_FILEPATH_DATA, Path[pathcap])));",
        "cb = FIELD_OFFSET(PROMPT_FILEPATH_DATA, Path) + pathcap * sizeof(TCHAR);`r`n  TinyGrowBuf inputbuf((IGrowBuf::size_type) cb);"
    )
}
Set-Content -LiteralPath $buildCpp -Value $build -Encoding ASCII
$ms = Get-Content $makenssi -Raw
if ($ms -match "\+\+initialparsefail;") {
    $ms = $ms.Replace("++initialparsefail;", "initialparsefail = true;")
    Set-Content -LiteralPath $makenssi -Value $ms -Encoding ASCII
}

# 4) zlib（MinGW 自带）整理成 NSIS 期望的目录
$mingw = Join-Path $env:USERPROFILE ".cache/fdxmp-tools/mingw64"
New-Item -ItemType Directory -Force -Path (Join-Path $zlib "include"), (Join-Path $zlib "lib") | Out-Null
Copy-Item (Join-Path $mingw "include/zlib.h"), (Join-Path $mingw "include/zconf.h") (Join-Path $zlib "include") -Force
Copy-Item (Join-Path $mingw "lib/libz.a"), (Join-Path $mingw "lib/libz.dll.a") (Join-Path $zlib "lib") -Force
Copy-Item (Join-Path $mingw "bin/zlib1.dll") $zlib -Force -ErrorAction SilentlyContinue

# 5) 构建 makensis + stubs
. (Join-Path $PSScriptRoot "dev-env.ps1") *> $null
$env:ZLIB_W32 = $zlib
$env:NSIS_SCONS_GNU_ENVPATHHACK = "1"
$sconsArgs = @(
    (Join-Path $py "scons.py"),
    "TOOLSET=mingw,gcc,g++,gnulink,ar,gas",
    "TARGET_ARCH=amd64",
    "NSIS_SCONS_GNU_ENVPATHHACK=1",
    "SKIPPLUGINS=all", "SKIPUTILS=all", "SKIPMISC=all", "SKIPDOC=all", "SKIPTESTS=all",
    "-j8"
)
Push-Location $src
try {
    & (Join-Path $py "python.exe") @sconsArgs
} finally { Pop-Location }

# 6) 组装可用的 NSIS 工具目录（含运行时 DLL）
New-Item -ItemType Directory -Force -Path (Join-Path $nsisOut "Stubs"), (Join-Path $nsisOut "Bin") | Out-Null
# makensis 以「exe 所在目录的上一级」作为 NSISDIR，因此 exe 放在 Bin 子目录。
Copy-Item (Join-Path $src "build/urelease/makensis/makensis.exe") (Join-Path $nsisOut "Bin") -Force
foreach ($name in @("zlib", "zlib_solid", "bzip2", "bzip2_solid", "lzma", "lzma_solid")) {
    $stubDir = Join-Path $src "build/urelease/stub_$name-amd64-unicode"
    $exe = Get-ChildItem $stubDir -Filter "stub_*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($exe) { Copy-Item $exe.FullName (Join-Path $nsisOut "Stubs/$name-amd64-unicode") -Force }
}
Copy-Item (Join-Path $src "Source/exehead/uninst.ico") (Join-Path $nsisOut "Stubs/uninst") -Force
Copy-Item (Join-Path $src "Include") (Join-Path $nsisOut "Include") -Recurse -Force
Copy-Item (Join-Path $mingw "bin/libwinpthread-1.dll") (Join-Path $nsisOut "Bin") -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $mingw "bin/zlib1.dll") (Join-Path $nsisOut "Bin") -Force -ErrorAction SilentlyContinue

$env:NSISDIR = $nsisOut
$version = & (Join-Path $nsisOut "Bin/makensis.exe") /VERSION
Write-Host "NSIS 构建完成：$version（$nsisOut）" -ForegroundColor Green
