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
# gpu feature：预览走 wgpu（iOS = Metal），失败自动回退 CPU。
cargo rustc -p fdxmp-ffi --release --features gpu --target "$DEVICE_TARGET" --crate-type staticlib
cargo rustc -p fdxmp-ffi --release --features gpu --target "$SIM_TARGET" --crate-type staticlib

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
