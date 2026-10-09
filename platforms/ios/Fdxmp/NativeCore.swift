import Foundation

/// Rust 核心桥接（C ABI，对应 crates/ffi）。
///
/// Xcode 工程需要：
///   1. `cargo build --release --target aarch64-apple-ios -p fdxmp-ffi`
///   2. 把 `libfdxmp_ffi.a` 与 `crates/ffi/include/fdxmp.h` 加入工程
///   3. 在 Build Settings → Other C Flags 加入 `-D FDXMP_IOS`
final class NativeCore {
    static let shared = NativeCore()

    /// 核心版本（通过 C ABI `fdxmp_version`）。
    let version: String

    /// ABI 版本（通过 C ABI `fdxmp_abi_version`）。
    let abiVersion: UInt32

    private init() {
        // C ABI 在链接后可用；此处提供安全的占位实现，
        // 阶段 5 接入静态库后替换为真实调用：
        //   let raw = fdxmp_version()
        //   version = String(cString: raw!)
        version = "0.1.0（静态库接入阶段 5）"
        abiVersion = 1
    }

    /// 预留：后续所有编辑操作都通过 fdxmp C ABI 完成
    /// （engine 生命周期、参数 JSON、预设读写、缓存管理）。
    func engineHandle() -> OpaquePointer? {
        nil
    }
}

