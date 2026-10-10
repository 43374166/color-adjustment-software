# iOS 打包（云打包 / macOS）

产出 **未签名 IPA**，用 Sideloadly / AltStore 以 Apple ID 签名后装到 iPhone。

## 一、云打包（GitHub Actions，推荐）

工作流：[`.github/workflows/ios-ipa.yml`](../../.github/workflows/ios-ipa.yml)

1. 把仓库推到 GitHub（公开库即可，macOS 运行器免费）；
2. 进入 **Actions → iOS 未签名 IPA → Run workflow**；
3. 等待约 10–20 分钟（首次编译 Rust 较慢，之后有缓存）；
4. 在运行页面的 **Artifacts** 下载 `Fdxmp-unsigned-ipa`，解压得到
   `Fdxmp-unsigned.ipa`。

工作流内部执行的步骤与 `packaging/ios/build-ipa.sh` 相同：

```
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo rustc -p fdxmp-ffi --release --target aarch64-apple-ios --crate-type staticlib
cargo rustc -p fdxmp-ffi --release --target aarch64-apple-ios-sim --crate-type staticlib
xcodebuild -create-xcframework ... -output dist/ios/FdxmpCore.xcframework
xcodegen generate            # platforms/ios
xcodebuild build ... CODE_SIGNING_ALLOWED=NO
zip -qry Fdxmp-unsigned.ipa Payload
```

## 二、安装到手机（Windows 也可以）

1. 电脑安装 [Sideloadly](https://sideloadly.io/)（或 AltStore + AltServer）；
2. 数据线连接 iPhone，打开 Sideloadly，把 `Fdxmp-unsigned.ipa` 拖进去；
3. 填写自己的 Apple ID（免费账号即可），点 Start；
4. 手机 **设置 → 通用 → VPN 与设备管理** 里信任开发者证书；
5. 打开 App，允许相册权限。

免费 Apple ID 的签名 **7 天过期**，过期后用同样步骤重新安装即可。
加入 Apple Developer Program（99 美元/年）后可用开发证书签 1 年，
并把 `ExportOptions.plist` 配好走 `xcodebuild -exportArchive` 直接产出可安装的
signed IPA。

## 三、本机（macOS）打包

```bash
brew install xcodegen
./packaging/ios/build-ipa.sh
# 产物：dist/ios/Fdxmp-unsigned.ipa
```

## 四、用自己的开发者证书签名（可选）

有付费开发者账号时，在 Xcode / CI 中配置 `DEVELOPMENT_TEAM`、证书与描述文件后：

```bash
xcodebuild -project platforms/ios/Fdxmp.xcodeproj -scheme Fdxmp \
  -configuration Release -destination 'generic/platform=iOS' \
  -archivePath build/Fdxmp.xcarchive archive

xcodebuild -exportArchive -archivePath build/Fdxmp.xcarchive \
  -exportOptionsPlist ExportOptions.plist -exportPath dist/ios
```

模板见 [`ExportOptions.plist`](ExportOptions.plist)：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key>
    <string>development</string>
    <key>teamID</key>
    <string>你的 Team ID</string>
    <key>compileBitcode</key>
    <false/>
</dict>
</plist>
```

## 五、注意事项

- iOS 端只用核心默认 feature（系统解码 HEIC / RAW），无需 LibRaw / libheif；
- 模拟器调试需要 `aarch64-apple-ios-sim`（Apple Silicon）或 `x86_64-apple-ios`（Intel）；
- 当前是 CPU 预览（1440px，逐像素 rayon 并行），手机上首帧约几百毫秒；GPU（wgpu Metal）版本后续接入；
- 编辑功能已覆盖亮度 / 颜色 / HSL / 颜色分级 / 效果 / 细节 / 曲线 / 裁切 / 蒙版（画笔 · 线性 · 径向）。
