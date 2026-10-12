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
download_model u2netp.onnx \
    "https://hf-mirror.com/BritishWerewolf/U-2-Netp/resolve/main/onnx/model.onnx" \
    309c8469258dda742793dce0ebea8e6dd393174f89934733ecc8b14c76f4ddd8
# AI 修图：MediaPipe FaceMesh v2（478 点关键点，脸部塑形用）
download_model face_landmarks_mediapipe_v2_fp32.onnx \
    "https://hf-mirror.com/fernandotonon/QtMeshEditor-facemesh-onnx/resolve/main/face_landmarks.onnx" \
    d16e5a55e6a480284d468ee32469692464049518c311662ab3956681de31e3e9
# 人脸解析：自转 BiSeNet-19（MIT 权重）——随仓库携带，直接复制 + 校验哈希。
check_or_copy_local_model() {
    local file="$1" sha="$2"
    local source="models/$file"
    local target="dist/ios/models/$file"
    [ -f "$target" ] && echo "$sha  $target" | shasum -a 256 -c - >/dev/null 2>&1 && return
    if [ -f "$source" ]; then
        cp "$source" "$target"
        echo "$sha  $target" | shasum -a 256 -c -
    else
        echo "缺少 $source（见 packaging/models/README.md 重新生成）" >&2
        exit 1
    fi
}
check_or_copy_local_model face_parsing_bisenet19_fp32.onnx \
    2453dcc1592590d1f9f7b064b3031c20f35bca74375445d23ab7f5ad3c38ea2f

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
