# fdxmp Android 构建环境（本机布局：SDK/NDK 在 E 盘，Gradle 与缓存放 F 盘）
#
# 用法：
#   . .\packaging\android\dev-env.ps1
#   cargo ndk -t arm64-v8a -o platforms/android/app/src/main/jniLibs build --release -p fdxmp-android-bridge
#   cd platforms/android; gradle assembleDebug

$env:ANDROID_HOME = "E:\dev\android-sdk"
$env:ANDROID_SDK_ROOT = $env:ANDROID_HOME
$env:ANDROID_NDK_HOME = Join-Path $env:ANDROID_HOME "ndk\28.2.13676358"
$env:GRADLE_USER_HOME = "F:\Android\gradle-home"
$env:JAVA_HOME = "C:\Program Files\Microsoft\jdk-17.0.20.101-hotspot"
$env:RUSTUP_DIST_SERVER = "https://rsproxy.cn"

# 使用 GNU 宿主工具链（已安装 Android 目标），并清掉 Windows/MinGW 的头文件路径，
# 否则 NDK clang 会误用 MinGW 的 stdio.h 等头文件导致 lcms2 等 C 依赖编译失败。
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-gnu"
foreach ($name in @("CPATH", "BINDGEN_EXTRA_CLANG_ARGS", "LIBCLANG_PATH", "PKG_CONFIG_PATH")) {
    Remove-Item ("Env:" + $name) -ErrorAction SilentlyContinue
}

$gradleBin = "F:\Android\gradle-8.7\bin"
$prepend = @()
if (Test-Path (Join-Path $env:JAVA_HOME "bin")) { $prepend += (Join-Path $env:JAVA_HOME "bin") }
if (Test-Path $gradleBin) { $prepend += $gradleBin }
if ($prepend.Count -gt 0) {
    $env:PATH = (($prepend -join ";") + ";" + $env:PATH)
}

foreach ($pair in @(
        @{ Label = "Android SDK"; Path = $env:ANDROID_HOME },
        @{ Label = "Android NDK"; Path = $env:ANDROID_NDK_HOME },
        @{ Label = "Gradle"; Path = (Join-Path $gradleBin "gradle.bat") },
        @{ Label = "JDK"; Path = (Join-Path $env:JAVA_HOME "bin\java.exe") }
    )) {
    if (-not (Test-Path $pair.Path)) {
        Write-Warning ("缺少{0}：{1}" -f $pair.Label, $pair.Path)
    }
}

Write-Host "fdxmp Android 环境已就绪" -ForegroundColor Green
Write-Host ("  SDK    : {0}" -f $env:ANDROID_HOME)
Write-Host ("  NDK    : {0}" -f $env:ANDROID_NDK_HOME)
Write-Host ("  Gradle : {0}（缓存 {1}）" -f $gradleBin, $env:GRADLE_USER_HOME)
Write-Host ("  JDK    : {0}" -f $env:JAVA_HOME)
