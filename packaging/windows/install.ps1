# fdxmp 安装脚本（免 NSIS 的替代方案：复制到用户目录 + 创建快捷方式 + 生成卸载脚本）。
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -InstallDir "F:\fdxmp" -NoDesktopShortcut
#
# 默认安装到 %LOCALAPPDATA%\Programs\fdxmp（无需管理员权限）。
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\fdxmp"),
    [switch]$NoShortcuts,
    [switch]$NoDesktopShortcut
)

$ErrorActionPreference = "Stop"
$source = $PSScriptRoot
$exeName = "fdxmp-win-ui.exe"
if (-not (Test-Path (Join-Path $source $exeName))) {
    throw "安装包内缺少 $exeName（请确认解压完整）"
}

Write-Host "安装 fdxmp 到：$InstallDir"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Get-ChildItem $source -File | Where-Object { $_.Extension -in ".exe", ".dll", ".txt" } |
    ForEach-Object { Copy-Item $_.FullName $InstallDir -Force }

if (-not $NoShortcuts) {
    $shell = New-Object -ComObject WScript.Shell
    $startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
    New-Item -ItemType Directory -Force -Path $startMenu | Out-Null
    $shortcut = $shell.CreateShortcut((Join-Path $startMenu "fdxmp.lnk"))
    $shortcut.TargetPath = Join-Path $InstallDir $exeName
    $shortcut.WorkingDirectory = $InstallDir
    $shortcut.Description = "fdxmp 图像处理"
    $shortcut.Save()
    if (-not $NoDesktopShortcut) {
        $desktop = [Environment]::GetFolderPath("Desktop")
        $desktopShortcut = $shell.CreateShortcut((Join-Path $desktop "fdxmp.lnk"))
        $desktopShortcut.TargetPath = Join-Path $InstallDir $exeName
        $desktopShortcut.WorkingDirectory = $InstallDir
        $desktopShortcut.Description = "fdxmp 图像处理"
        $desktopShortcut.Save()
    }
}

$uninstallTemplate = @'
$ErrorActionPreference = "SilentlyContinue"
$installDir = "__INSTALL_DIR__"
$startMenu = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\fdxmp.lnk"
$desktop = Join-Path ([Environment]::GetFolderPath("Desktop")) "fdxmp.lnk"
Remove-Item $startMenu -Force
Remove-Item $desktop -Force
Remove-Item $installDir -Recurse -Force
Write-Host "fdxmp 已卸载"
'@
$uninstallTemplate.Replace("__INSTALL_DIR__", $InstallDir) |
    Set-Content (Join-Path $InstallDir "uninstall.ps1") -Encoding UTF8

Write-Host "完成：$InstallDir\$exeName"
Write-Host "卸载：powershell -ExecutionPolicy Bypass -File `"$InstallDir\uninstall.ps1`""
