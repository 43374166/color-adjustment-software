# 用 Windows 自带 IExpress 生成自解压安装包（NSIS 不可用时的替代方案）。
#
# 用法：powershell -File packaging/windows/make-iexpress-setup.ps1 -SourceDir dist\windows -Target dist\fdxmp-setup.exe
#
# 生成的 setup.exe 解压后自动运行 install.ps1（复制到 %LOCALAPPDATA%\Programs\fdxmp 并创建快捷方式）。
# 支持运行时参数：/Q 静默、/T:<目录> 指定解压目录、/C 只解压不执行。
param(
    [Parameter(Mandatory = $true)][string]$SourceDir,
    [Parameter(Mandatory = $true)][string]$Target
)

$ErrorActionPreference = "Stop"
$source = (Resolve-Path $SourceDir).Path
$targetPath = [System.IO.Path]::GetFullPath($Target)
$sedPath = Join-Path ([System.IO.Path]::GetTempPath()) ("fdxmp-setup-" + [Guid]::NewGuid().ToString("N") + ".sed")
if (Test-Path $targetPath) { Remove-Item -LiteralPath $targetPath -Force }

$files = Get-ChildItem $source -File |
    Where-Object { $_.Extension -in ".exe", ".dll", ".ps1", ".txt" } |
    Sort-Object Name
if ($files.Count -eq 0) { throw "源目录没有可打包的文件：$source" }
if (-not ($files.Name -contains "fdxmp-win-ui.exe")) { throw "源目录缺少 fdxmp-win-ui.exe" }
if (-not ($files.Name -contains "install.ps1")) { throw "源目录缺少 install.ps1" }

$sb = New-Object System.Text.StringBuilder
[void]$sb.AppendLine("[Version]")
[void]$sb.AppendLine("Class=IEXPRESS")
[void]$sb.AppendLine("SEDVersion=3")
[void]$sb.AppendLine("[Options]")
[void]$sb.AppendLine("PackagePurpose=InstallApp")
[void]$sb.AppendLine("ShowInstallProgramWindow=1")
[void]$sb.AppendLine("HideExtractAnimation=1")
[void]$sb.AppendLine("UseLongFileName=1")
[void]$sb.AppendLine("InsideCompressed=0")
[void]$sb.AppendLine("CAB_FixedSize=0")
[void]$sb.AppendLine("CAB_ResvCodeSigning=0")
[void]$sb.AppendLine("RebootMode=N")
[void]$sb.AppendLine("InstallPrompt=fdxmp 安装：将复制到 %LOCALAPPDATA%\Programs\fdxmp 并创建快捷方式")
[void]$sb.AppendLine("DisplayLicense=")
[void]$sb.AppendLine("FinishMessage=fdxmp 安装完成")
[void]$sb.AppendLine("TargetName=$targetPath")
[void]$sb.AppendLine("FriendlyName=fdxmp 安装程序")
[void]$sb.AppendLine("AppLaunched=powershell.exe -NoProfile -ExecutionPolicy Bypass -File install.ps1")
[void]$sb.AppendLine("PostInstallCmd=<None>")
[void]$sb.AppendLine("AdminQuietInstCmd=")
[void]$sb.AppendLine("UserQuietInstCmd=")
[void]$sb.AppendLine("SourceFiles=SourceFiles")
[void]$sb.AppendLine("[SourceFiles]")
[void]$sb.AppendLine("SourceFiles0=$source\")
[void]$sb.AppendLine("[SourceFiles0]")
$index = 0
foreach ($file in $files) {
    [void]$sb.AppendLine("%FILE$index%=$($file.Name)")
    $index++
}
[void]$sb.AppendLine("[Strings]")
$index = 0
foreach ($file in $files) {
    [void]$sb.AppendLine("FILE$index=`"$($file.Name)`"")
    $index++
}
Set-Content -LiteralPath $sedPath -Value $sb.ToString() -Encoding ASCII

$iexpress = Join-Path $env:SystemRoot "System32\iexpress.exe"
if (-not (Test-Path $iexpress)) { throw "找不到 IExpress：$iexpress" }
& $iexpress /N /Q $sedPath

# IExpress 是 GUI 程序，可能在后台异步完成，这里等待产物出现（最多 120 秒）。
$waitStarted = Get-Date
while (-not (Test-Path $targetPath) -and ((Get-Date) - $waitStarted).TotalSeconds -lt 120) {
    Start-Sleep -Milliseconds 500
}
if (-not (Test-Path $targetPath)) { throw "IExpress 未生成安装包：$targetPath" }
Start-Sleep -Seconds 2 # 等待写入完成后取大小
Write-Host ("自解压安装包：{0}（{1:N1} MB）" -f $targetPath, ((Get-Item $targetPath).Length / 1MB))
