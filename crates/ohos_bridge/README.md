# fdxmp-ohos-bridge

HarmonyOS（ArkTS / ArkUI）→ Rust 核心的 N-API 桥接层，基于 **ohos-rs**。

## 为什么阶段 0 没有直接声明依赖

`napi-ohos` 未发布到 crates.io，只能通过 git 引入：

```toml
[target.'cfg(target_env = "ohos")'.dependencies]
napi-ohos = { git = "https://github.com/ohos-rs/ohos-rs", package = "napi-ohos", branch = "main" }
```

阶段 0 的构建环境可以访问 crates.io，但无法完成 GitHub git clone（网络代理限制），
若直接写入该依赖会导致整个 workspace 解析失败。因此这里保留文档化入口，
宿主机构建（Windows / Linux / macOS）不受影响。

## 阶段 5 启用步骤

1. 在能访问 GitHub 的环境执行 `cargo fetch`（或先把 ohos-rs 仓库镜像到内网 Git）；
2. 按上面的 TOML 片段加入依赖；
3. 在 `src/lib.rs` 恢复 `use napi_ohos as _;`，并用 ohos-rs 宏导出：

   ```rust
   #[napi_ohos::module_exports]
   fn init() {}

   #[napi_ohos::js_function]
   pub fn abi_version() -> i32 { fdxmp_core::ABI_VERSION as i32 }
   ```

4. 安装 OHOS 目标：`rustup target add aarch64-unknown-linux-ohos`；
5. 构建：`cargo build --release -p fdxmp-ohos-bridge --target aarch64-unknown-linux-ohos`；
6. 产物 `libfdxmp_ohos_bridge.so` 放入 DevEco 工程 `entry/libs/arm64-v8a/`。

导出对照表见 `src/lib.rs::exports()` 与 `docs/FFI.md`。

