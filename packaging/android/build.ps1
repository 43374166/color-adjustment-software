# fdxmp Android 打包：cargo ndk（Rust 桥接）→ Gradle（APK）→ dist/android/
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File packaging/android/build.ps1            # debug APK（可直接装手机）
#   powershell -ExecutionPolicy Bypass -File packaging/android/build.ps1 -Release   # release APK（未签名）
#   powershell -ExecutionPolicy Bypass -File packaging/android/build.ps1 -SkipRust # 只跑 Gradle
#
# 环境（本机布局）：SDK/NDK 在 E:\dev\android-sdk，Gradle 与缓存在 F:\Android，
# 依赖走阿里云 Maven 镜像（见 packaging/android/dev-env.ps1 与 settings.gradle.kts）。
param(
    [switch]$Release,
    [switch]$SkipRust
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$platform = Join-Path $root "platforms/android"
$dist = Join-Path $root "dist/android"

. (Join-Path $PSScriptRoot "dev-env.ps1") *> $null

if (-not (Get-Command cargo-ndk -ErrorAction SilentlyContinue)) {
    throw "缺少 cargo-ndk：请执行 cargo install cargo-ndk"
}
if (-not (Test-Path $platform)) {
    throw "未找到 platforms/android"
}

if (-not $SkipRust) {
    Push-Location $root
    try {
        cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 `
            -o (Join-Path $platform "app/src/main/jniLibs") `
            build --release -p fdxmp-android-bridge
    }
    finally {
        Pop-Location
    }
}

$task = if ($Release) { "assembleRelease" } else { "assembleDebug" }
Push-Location $platform
try {
    if (Test-Path ".\gradlew.bat") {
        & ".\gradlew.bat" $task
    }
    else {
        gradle --no-daemon --console=plain $task
    }
}
finally {
    Pop-Location
}

$flavor = if ($Release) { "release" } else { "debug" }
$apk = Get-ChildItem (Join-Path $platform "app/build/outputs/apk/$flavor") -Filter "*.apk" -ErrorAction SilentlyContinue |
    Select-Object -First 1
if (-not $apk) {
    throw "未找到 $flavor APK"
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
$target = Join-Path $dist ("fdxmp-1.1.6-{0}.apk" -f $flavor)
Copy-Item $apk.FullName $target -Force
Write-Host ("Android 打包完成：{0}（{1:N1} MB）" -f $target, ($apk.Length / 1MB)) -ForegroundColor Green

