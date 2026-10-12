#!/usr/bin/env bash
# fdxmp iOS 打包脚本（本机 macOS / 云打包通用）
#
# 产出：dist/ios/Fdxmp-unsigned.ipa（未签名，用 Sideloadly / AltStore 签名安装）
#
# 依赖：Xcode、Rust（rustup）、XcodeGen（brew install xcodegen）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

DEVICE_TARGET="aarch64-apple-ios"
SIM_TARGET="aarch64-apple-ios-sim"
APP_PATH="platforms/ios/build/Build/Products/Release-iphoneos/Fdxmp.app"

echo "==> 1/5 构建 Rust 静态库（$DEVICE_TARGET / $SIM_TARGET）"
rustup target add "$DEVICE_TARGET" "$SIM_TARGET"

# 智能蒙版模型（随包进 App；已有则跳过）：与 .github/workflows/ios-ipa.yml 同一组 URL + 哈希。
echo "    下载智能蒙版模型（hf-mirror，缺哪个补哪个）"
mkdir -p dist/ios/models
download_model() {
    local file="$1" url="$2" sha="$3"
    if [ -f "dist/ios/models/$file" ] && echo "$sha  dist/ios/models/$file" | shasum -a 256 -c - >/dev/null 2>&1; then
        return
    fi
    curl -L --fail --retry 3 -o "dist/ios/models/$file" "$url"
    echo "$sha  dist/ios/models/$file" | shasum -a 256 -c -
}
download_model face_detector_scrfd2p5g_bnkps_fp32.onnx \
    "https://hf-mirror.com/RuteNL/SCRFD-face-detection-ONNX/resolve/main/2.5g_bnkps.onnx" \
    3f1ac54e769cb5fd76eda11ac3c088eed78d1f51a935a839d04d49b0e770219e
download_model person_coarse_modnet_fp32.onnx \
    "https://hf-mirror.com/Xenova/modnet/resolve/main/onnx/model.onnx" \
    07c308cf0fc7e6e8b2065a12ed7fc07e1de8febb7dc7839d7b7f15dd66584df9
download_model face_parsing_quantized.onnx \
    "https://hf-mirror.com/Xenova/face-parsing/resolve/main/onnx/model_quantized.onnx" \
    5bab9bfb3cb979f3098ac3b934b1641dbf87f835e0b03c2ca6d88dcf18c83d27
download_model u2netp.onnx \
    "https://hf-mirror.com/BritishWerewolf/U-2-Netp/resolve/main/onnx/model.onnx" \
    309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8

# gpu feature：预览走 wgpu（iOS = Metal），失败自动回退 CPU；
# raw feature：LibRaw（rsraw）解相机 RAW / DNG，工程侧需链接 libc++（见 project.yml）；
# ai-static feature：随包 ONNX Runtime（pyke 预编译 + CoreML 后端）静态链接。模型文件见 dist/ios/models。
cargo rustc -p fdxmp-ffi --release --features gpu,raw,ai-static --target "$DEVICE_TARGET" --crate-type staticlib
cargo rustc -p fdxmp-ffi --release --features gpu,raw,ai-static --target "$SIM_TARGET" --crate-type staticlib

echo "==> 2/5 合并 FdxmpCore.xcframework"
rm -rf dist/ios/FdxmpCore.xcframework
mkdir -p dist/ios
xcodebuild -create-xcframework \
    -library "target/$DEVICE_TARGET/release/libfdxmp_ffi.a" \
    -library "target/$SIM_TARGET/release/libfdxmp_ffi.a" \
    -output dist/ios/FdxmpCore.xcframework

echo "==> 3/5 生成 Xcode 工程"
if ! command -v xcodegen >/dev/null 2>&1; then
    echo "未找到 xcodegen，请先安装：brew install xcodegen" >&2
    exit 1
fi
(cd platforms/ios && xcodegen generate)

echo "==> 4/5 编译 App（未签名）"
xcodebuild -project platforms/ios/Fdxmp.xcodeproj \
    -scheme Fdxmp \
    -configuration Release \
    -sdk iphoneos \
    -destination 'generic/platform=iOS' \
    -derivedDataPath platforms/ios/build \
    CODE_SIGNING_ALLOWED=NO \
    CODE_SIGNING_REQUIRED=NO \
    CODE_SIGN_IDENTITY="" \
    build

echo "==> 5/5 打包 IPA"
PLIST="$APP_PATH/Info.plist"
echo "CFBundleIdentifier: $(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")"
echo "CFBundleExecutable: $(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$PLIST")"
rm -rf dist/ios/Payload dist/ios/Fdxmp-unsigned.ipa
mkdir -p dist/ios/Payload
cp -R "$APP_PATH" dist/ios/Payload/
(cd dist/ios && zip -qry Fdxmp-unsigned.ipa Payload)

echo
echo "完成：$ROOT/dist/ios/Fdxmp-unsigned.ipa"
echo "安装：用 Sideloadly / AltStore 以你的 Apple ID 签名后装到手机（免费账号 7 天有效）。"
