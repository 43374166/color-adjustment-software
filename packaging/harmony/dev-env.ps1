# fdxmp HarmonyOS build environment (DevEco Studio 26 on E:; command-line only, no GUI needed)
#
# Usage:
#   . .\packaging\harmony\dev-env.ps1
#   cd platforms\harmony
#   .\hvigorw.bat assembleHap --mode module -p product=default

$DevEcoHome = if ($env:DEVECO_STUDIO_HOME) { $env:DEVECO_STUDIO_HOME } else { 'E:\installs\DevEco Studio' }
if (-not (Test-Path -LiteralPath (Join-Path $DevEcoHome 'tools\hvigor\bin\hvigorw.js'))) {
    throw "DevEco Studio not found at '$DevEcoHome'. Set DEVECO_STUDIO_HOME to the install dir."
}

$env:DEVECO_STUDIO_HOME = $DevEcoHome
$env:DEVECO_SDK_HOME = Join-Path $DevEcoHome 'sdk'
$env:NODE_HOME = Join-Path $DevEcoHome 'tools\node'

# Keep all user-level caches on F: (never write to C:); TEMP follows the project convention.
$cacheRoot = 'F:\HarmonyOS'
foreach ($d in @("$cacheRoot\hvigor-home", "$cacheRoot\ohpm-home", 'F:\FD-Room\.tmp')) {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}
$env:HVIGOR_USER_HOME = Join-Path $cacheRoot 'hvigor-home'
$env:OHPM_HOME = Join-Path $cacheRoot 'ohpm-home'
$env:TEMP = 'F:\FD-Room\.tmp'
$env:TMP = 'F:\FD-Room\.tmp'

$toolPaths = @(
    (Join-Path $DevEcoHome 'tools\hvigor\bin'),
    (Join-Path $DevEcoHome 'tools\ohpm\bin'),
    (Join-Path $DevEcoHome 'tools\node')
)
$env:Path = (($toolPaths + $env:Path) -join ';')

Write-Host "HarmonyOS dev-env ready:" -ForegroundColor Green
Write-Host "  DEVECO_STUDIO_HOME = $env:DEVECO_STUDIO_HOME"
Write-Host "  DEVECO_SDK_HOME    = $env:DEVECO_SDK_HOME"
Write-Host "  HVIGOR_USER_HOME   = $env:HVIGOR_USER_HOME"
