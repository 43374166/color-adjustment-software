# fdxmp HarmonyOS build environment (DevEco Studio 26; command-line only, no GUI needed)
#
# Usage:
#   . .\packaging\harmony\dev-env.ps1
#   cd platforms\harmony
#   .\hvigorw.bat assembleHap --mode module -p product=default

function Test-DevEcoHome([string]$path) {
    if (-not $path) { return $false }
    try {
        $probe = [System.IO.Path]::Combine($path, 'tools\hvigor\bin\hvigorw.js')
        return Test-Path -LiteralPath $probe
    } catch {
        return $false
    }
}

$candidates = @(
    $env:DEVECO_STUDIO_HOME,
    'D:\install\DevEco Studio',
    'E:\installs\DevEco Studio'
) | Where-Object { Test-DevEcoHome $_ }
$DevEcoHome = if (@($candidates).Count -gt 0) { @($candidates)[0] } else { $null }
if (-not $DevEcoHome) {
    throw "DevEco Studio not found. Set DEVECO_STUDIO_HOME to the install dir."
}

$env:DEVECO_STUDIO_HOME = $DevEcoHome
$env:DEVECO_SDK_HOME = Join-Path $DevEcoHome 'sdk'
$env:NODE_HOME = Join-Path $DevEcoHome 'tools\node'

# Keep all user-level caches off the system drive (F: is gone on this machine, use D:).
$cacheRoot = if ($env:FDXMP_HARMONY_CACHE) { $env:FDXMP_HARMONY_CACHE } else { 'D:\HarmonyHome' }
$tmp = Join-Path $cacheRoot 'tmp'
foreach ($d in @("$cacheRoot\hvigor-home", "$cacheRoot\ohpm-home", $tmp)) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}
$env:HVIGOR_USER_HOME = Join-Path $cacheRoot 'hvigor-home'
$env:OHPM_HOME = Join-Path $cacheRoot 'ohpm-home'
$env:TEMP = $tmp
$env:TMP = $tmp

$toolPaths = @(
    (Join-Path $DevEcoHome 'tools\hvigor\bin'),
    (Join-Path $DevEcoHome 'tools\ohpm\bin'),
    (Join-Path $DevEcoHome 'tools\node')
)
$env:Path = (($toolPaths + $env:Path) -join ';')

# NDK 路径含空格（"DevEco Studio"），cc-rs 会把 CC 的值截断在空格处，
# 这里建一个无空格的目录联接给交叉编译用。
$ndkSource = Join-Path $DevEcoHome 'sdk\default\openharmony\native'
$ndkLink = Join-Path $env:SystemDrive 'ohos-ndk'
if (Test-Path -LiteralPath $ndkSource) {
    if (-not (Test-Path -LiteralPath $ndkLink)) {
        cmd /c "mklink /J `"$ndkLink`" `"$ndkSource`"" | Out-Null
    }
    $env:FDXMP_OHOS_NDK = if (Test-Path -LiteralPath $ndkLink) { $ndkLink } else { $ndkSource }
}

Write-Host "HarmonyOS dev-env ready:" -ForegroundColor Green
Write-Host "  DEVECO_STUDIO_HOME = $env:DEVECO_STUDIO_HOME"
Write-Host "  DEVECO_SDK_HOME    = $env:DEVECO_SDK_HOME"
Write-Host "  HVIGOR_USER_HOME   = $env:HVIGOR_USER_HOME"
