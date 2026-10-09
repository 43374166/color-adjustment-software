// fdxmp HarmonyOS N-API 桥接封装（对应 crates/ohos_bridge，产物 libfdxmp_ohos_bridge.so）
//
// 当前 Rust 桥接尚未启用：ohos-rs 的 napi-ohos 依赖需要 GitHub 源（见 crates/ohos_bridge/README.md），
// 先用同接口的 stub 保证 HAP 可以独立打包。桥接产物就绪后按两步切回：
//   1) 把 libfdxmp_ohos_bridge.so 放到 entry/libs/arm64-v8a/
//      （或运行 packaging/harmony/build.ps1 -WithRustBridge 自动交叉编译并复制）；
//   2) 打开下面的 import，删除 stub 实现。
// import fdxmp from 'libfdxmp_ohos_bridge.so';

export interface FdxmpNative {
  abiVersion(): number;
  version(): string;
}

/** 桥接未启用时的占位实现（ArkTS 严格模式下不能用对象字面量实现接口）。 */
class FdxmpNativeStub implements FdxmpNative {
  abiVersion(): number {
    return 0;
  }

  version(): string {
    return 'stub';
  }
}

const nativeCore: FdxmpNative = new FdxmpNativeStub();

export default nativeCore;
