# 把 MinGW/C++ 运行时 DLL 复制到可执行文件旁边，让 exe 可以双击直接运行。
#
# 用法：
#   powershell -File packaging/windows/deploy-runtime.ps1 -Exe target\debug\fdxmp-win-ui.exe
#   powershell -File packaging/windows/deploy-runtime.ps1 -Exe target\release\fdxmp-win-ui.exe
#
# 原理：用 objdump 读取 exe 的导入表，递归收集依赖，从本地 MinGW 工具链目录
# （默认 D:\dev\msys64\mingw64\bin，可用 -SourceDir 或 $env:FDXMP_TOOLS 覆盖）复制缺失的 DLL。

param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$SourceDir = ""
)

$ErrorActionPreference = "Stop"

if (-not $SourceDir) {
    # 与 dev-env.ps1 保持一致的解析顺序：$env:FDXMP_TOOLS → D:\dev\msys64\mingw64 → 旧缓存目录
    $candidates = @()
    if ($env:FDXMP_TOOLS) { $candidates += $env:FDXMP_TOOLS }
    $candidates += @("D:\dev\msys64\mingw64", (Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64"))
    $mingwRoot = $candidates | Where-Object { Test-Path (Join-Path $_ "bin\objdump.exe") } | Select-Object -First 1
    if ($mingwRoot) {
        $SourceDir = Join-Path $mingwRoot "bin"
    }
    else {
        $onPath = Get-Command objdump -ErrorAction SilentlyContinue
        if ($onPath) { $SourceDir = Split-Path $onPath.Source -Parent }
        else { $SourceDir = Join-Path $env:USERPROFILE ".cache\fdxmp-tools\mingw64\bin" }
    }
}
$exePath = (Resolve-Path $Exe).Path
$targetDir = Split-Path $exePath -Parent
$objdump = Join-Path $SourceDir "objdump.exe"

if (-not (Test-Path $objdump)) {
    throw "找不到 objdump：$objdump（请确认 MinGW 工具链已解压到 $SourceDir）"
}

$queue = New-Object System.Collections.Queue
$seen = @{}
$queue.Enqueue($exePath)
$copied = New-Object System.Collections.Generic.List[string]

while ($queue.Count -gt 0) {
    $current = $queue.Dequeue()
    $imports = New-Object System.Collections.Generic.List[string]
    foreach ($line in (& $objdump -p $current 2>$null)) {
        if ($line -match 'DLL Name:\s*(\S+)') {
            $imports.Add($Matches[1])
        }
    }

    foreach ($dll in $imports) {
        $key = $dll.ToLower()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true

        $local = Join-Path $targetDir $dll
        $source = Join-Path $SourceDir $dll
        if (Test-Path $local) {
            # 已经在 exe 旁边（可能是之前复制过的），继续查它的依赖
            $queue.Enqueue($local)
        }
        elseif (Test-Path $source) {
            Copy-Item -LiteralPath $source -Destination $targetDir -Force
            $copied.Add($dll)
            $queue.Enqueue($source)
        }
    }
}

if ($copied.Count -eq 0) {
    Write-Host "运行时 DLL 已齐全，无需复制：$targetDir" -ForegroundColor Green
}
else {
    Write-Host "已复制 $($copied.Count) 个运行时 DLL 到 $targetDir：" -ForegroundColor Green
    $copied | ForEach-Object { Write-Host "  $_" }
}
