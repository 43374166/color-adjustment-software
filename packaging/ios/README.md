# iOS 打包（占位）

按规格：iOS 先写代码、不打包。后续接入方式：

1. `cargo build --release --target aarch64-apple-ios -p fdxmp-ffi`
2. 合并 `staticlib` 为 `xcframework`
3. SwiftUI 工程链接 xcframework，通过 `crates/ffi/include/fdxmp.h` 调用

