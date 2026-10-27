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
    # 交叉编译 Rust 核心（crates/ffi，C ABI）到两个 ABI：
    #   arm64-v8a  → 真机
    #   x86_64     → 本地模拟器（Mate 90 Pro 是 x86_64）
    $ndk = $env:FDXMP_OHOS_NDK
    if (-not $ndk -or -not (Test-Path -LiteralPath $ndk)) {
        throw "OHOS NDK 未找到（dev-env.ps1 会设置 FDXMP_OHOS_NDK）"
    }
    $clang = Join-Path $ndk 'llvm\bin\clang.exe'
    $sysroot = Join-Path $ndk 'sysroot'

    $targets = @(
        @{ Triple = 'aarch64-unknown-linux-ohos'; Abi = 'arm64-v8a'; ClangTarget = 'aarch64-linux-ohos' },
        @{ Triple = 'x86_64-unknown-linux-ohos'; Abi = 'x86_64'; ClangTarget = 'x86_64-linux-ohos' }
    )

    foreach ($target in $targets) {
        $upper = $target.Triple.ToUpper().Replace('-', '_')
        Set-Item -Path "env:CARGO_TARGET_${upper}_LINKER" -Value $clang
        Set-Item -Path "env:CARGO_TARGET_${upper}_RUSTFLAGS" -Value "-C link-arg=--target=$($target.ClangTarget) -C link-arg=--sysroot=$sysroot"
        Set-Item -Path "env:CC_$($target.Triple.Replace('-', '_'))" -Value $clang
        Set-Item -Path "env:AR_$($target.Triple.Replace('-', '_'))" -Value (Join-Path $ndk 'llvm\bin\llvm-ar.exe')
        Set-Item -Path "env:CFLAGS_$($target.Triple.Replace('-', '_'))" -Value "--target=$($target.ClangTarget) --sysroot=$sysroot"

        Push-Location $root
        try {
            cargo build --release -p fdxmp-ffi --target $target.Triple
            if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $($target.Triple)" }
        }
        finally {
            Pop-Location
        }

        $so = Join-Path $root "target/$($target.Triple)/release/libfdxmp_ffi.so"
        if (-not (Test-Path -LiteralPath $so)) {
            throw "Rust artifact not found: $so"
        }
        # 一份给 CMake 链接，一份给 HAP 打包（DT_NEEDED 在运行时解析）。
        foreach ($dir in @("entry/src/main/cpp/libs/$($target.Abi)", "entry/libs/$($target.Abi)")) {
            $libDir = Join-Path $platform $dir
            New-Item -ItemType Directory -Force -Path $libDir | Out-Null
            Copy-Item -LiteralPath $so -Destination (Join-Path $libDir 'libfdxmp_ffi.so') -Force
        }
    }
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
