# Android 模拟器验证记录

日期：2026-10-25  
目标：验证同一份 Rust 色彩核心在 Android 模拟器上运行，而不是另写一套 Android 实现。

## 环境

- WSL2 Kali Linux，KVM 可用（`/dev/kvm`）
- Android SDK：`/usr/lib/android-sdk`
- Build Tools：29.0.3（系统包）
- NDK：27.2.12479018
- 系统镜像：`system-images;android-35;default;x86_64`
- AVD：`fdxmp`（Pixel，API 35）

## 安装与启动

```bash
# 系统包（Kali）
apt-get install -y android-sdk sdkmanager android-sdk-platform-tools \
  google-android-emulator-installer google-android-ndk-r27c-installer

# 平台 + 镜像（root；安装到 /usr/lib/android-sdk）
yes | sdkmanager 'platforms;android-35' \
  'system-images;android-35;default;x86_64' 'cmdline-tools;latest'

# AVD
avdmanager create avd -n fdxmp \
  -k 'system-images;android-35;default;x86_64' --device pixel

# headless 模拟器（KVM）
emulator -avd fdxmp -no-window -no-audio -no-boot-anim \
  -gpu swiftshader_indirect -no-snapshot -accel on
adb wait-for-device
adb shell getprop sys.boot_completed   # 1 = 完成
```

## 交叉编译核心测试

```bash
rustup target add x86_64-linux-android

NDK=/usr/lib/android-sdk/ndk/27.2.12479018
TC=$NDK/toolchains/llvm/prebuilt/linux-x86_64
export CC_x86_64_linux_android=$TC/bin/x86_64-linux-android35-clang
export AR_x86_64_linux_android=$TC/bin/llvm-ar
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER=$TC/bin/x86_64-linux-android35-clang
export CFLAGS_x86_64_linux_android="--sysroot=$TC/sysroot"
export CARGO_TARGET_DIR=/home/Lico/target-fdxmp-android

cargo test -p fdxmp-color --target x86_64-linux-android --no-run
cargo test -p fdxmp-core --lib --target x86_64-linux-android --no-run
cargo test -p fdxmp-core --test color_extremes --target x86_64-linux-android --no-run
cargo test -p fdxmp-ffi --lib --target x86_64-linux-android --no-run
```

## 推送到模拟器执行

```bash
adb shell mkdir -p /data/local/tmp/fdxmp
adb push <test-binary> /data/local/tmp/fdxmp/
adb shell chmod 755 /data/local/tmp/fdxmp/<test-binary>
adb shell /data/local/tmp/fdxmp/<test-binary> --test-threads=1
```

## 结果

| 测试 | 结果 |
|---|---|
| `fdxmp-color` 单元 | 42 通过 |
| `extreme_sweep` | 3 通过 |
| `fdxmp-core --lib` | 173 通过、1 ignored |
| `color_extremes` | 12 通过（含饱和度 +100 色相漂移 <2°） |
| `fdxmp-ffi --lib` | 12 通过 |

结论：Android 模拟器上运行的是与 Windows/Linux 相同的 `crates/color` / `crates/core` / `crates/ffi` 代码；色彩数学和 FFI 测试均通过。

## 未覆盖

- HarmonyOS 模拟器：本机未装 DevEco Studio/OHOS SDK；只对 `aarch64-unknown-linux-ohos` 做了 `fdxmp-color` 的 `cargo check`。
- Android 完整 APK/UI 尚未构建（需要 Gradle + NDK 打包工程）；本次验证的是共享核心与 FFI，不是 Compose UI。
