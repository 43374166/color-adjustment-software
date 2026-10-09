# fdxmp HarmonyOS packaging: hvigorw assembleHap -> dist/harmony/
#
# Usage:
#   .\packaging\harmony\build.ps1                   # pack ArkTS HAP only (current default)
#   .\packaging\harmony\build.ps1 -WithRustBridge   # cross-build the ohos bridge .so first (needs ohos-rs enabled)

param(
    [switch]$WithRustBridge
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$platform = Join-Path $root "platforms/harmony"
$dist = Join-Path $root "dist/harmony"

. (Join-Path $PSScriptRoot "dev-env.ps1")

if ($WithRustBridge) {
    Push-Location $root
    try {
        cargo build --release -p fdxmp-ohos-bridge --target aarch64-unknown-linux-ohos
    }
    finally {
        Pop-Location
    }
    $so = Join-Path $root "target/aarch64-unknown-linux-ohos/release/libfdxmp_ohos_bridge.so"
    if (-not (Test-Path -LiteralPath $so)) {
        throw "Bridge artifact not found: $so"
    }
    $libDir = Join-Path $platform "entry/libs/arm64-v8a"
    New-Item -ItemType Directory -Force -Path $libDir | Out-Null
    Copy-Item -LiteralPath $so -Destination $libDir -Force
}

if (-not (Test-Path -LiteralPath $platform)) {
    throw "platforms/harmony not found"
}

Push-Location $platform
try {
    & (Join-Path $platform "hvigorw.bat") assembleHap --mode module -p product=default
    if ($LASTEXITCODE -ne 0) {
        throw "hvigorw failed (exit $LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
Get-ChildItem -Path $platform -Recurse -Filter "*.hap" | Copy-Item -Destination $dist -Force

Write-Host "HarmonyOS package done: $dist" -ForegroundColor Green
