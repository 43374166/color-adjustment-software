# Android 平台工程（阶段 5 骨架）

- UI：Jetpack Compose，三大 Tab（设备 / 编辑 / 我的），底部导航 + 顶部标题随 Tab 变化。
- 桥接：`com.fdxmp.core.NativeBridge`（JNI，对应 `crates/android_bridge`）。
- 相册权限：`READ_MEDIA_IMAGES`（Android 13+）/ `READ_EXTERNAL_STORAGE`（≤12）。

## 构建

环境已就绪（本机）：SDK/NDK `E:\dev\android-sdk`（build-tools 35/36、platforms 35/36、NDK 28.2）、
Gradle 8.7 `F:\Android\gradle-8.7`（缓存 `F:\Android\gradle-home`）、JDK 17。
Maven 依赖走阿里云镜像（`settings.gradle.kts`）。

```powershell
. .\packaging\android\dev-env.ps1
powershell -File packaging\android\build.ps1        # ✅ 已实测：debug APK 16 MB（三 ABI）
```

产物：`dist/android/fdxmp-1.1.2-debug.apk`（debug 可直接安装到手机；
`-Release` 生成未签名 release APK，需要 keystore 才能上架）。

已实测（2026-10-03）：Rust 桥接 arm64-v8a / armeabi-v7a / x86_64 全部编译，
Gradle `assembleDebug` 成功，APK 内含三个 ABI 的 `libfdxmp_android_bridge.so`。

## 下一步（阶段 5 后续）

1. 接入 `fdxmp-ffi` 的完整接口：解码、编辑参数、导出（JSON 字符串 / 字节数组）。
2. 设备 Tab：MediaStore 查询 + 缩略图网格 + 全屏预览。
3. 编辑 Tab：本地数据库（Room）记录已编辑文件与预设名。
4. 我的 Tab：缓存管理（对应 core 的 `CacheManager`）、预设导入导出。
5. 编辑界面：底部弹出工具面板 + GPU 预览（wgpu Vulkan 后端）。

> 说明：压缩包 / 镜像等大文件不在 C 盘；Android 相关环境全部落在 E/F 盘。
