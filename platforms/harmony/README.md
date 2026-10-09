# HarmonyOS 平台工程（阶段 5）

- UI：ArkTS / ArkUI，三大 Tab（设备 / 编辑 / 我的），顶部标题随 Tab 变化。
- 桥接：`libfdxmp_ohos_bridge.so`（ohos-rs / N-API，对应 `crates/ohos_bridge`）；当前未启用，先用同接口 stub。
- 权限：`ohos.permission.READ_IMAGEVIDEO`。
- 工具链：DevEco Studio 26（SDK = HarmonyOS 26.0.0 / API 26）安装在 `E:\installs\DevEco Studio`；
  打包完全走命令行（hvigorw），不需要打开 IDE。

## 构建（2026-10-03 本机已验证）

```powershell
. .\packaging\harmony\dev-env.ps1        # 配置 DEVECO_SDK_HOME / hvigor / ohpm / node，缓存全部在 F 盘
cd platforms\harmony
.\hvigorw.bat assembleHap --mode module -p product=default
```

或者一步到位（打包 + 复制产物）：

```powershell
.\packaging\harmony\build.ps1            # → dist/harmony/entry-default-unsigned.hap
```

要点：

- API 26 起 `compatibleSdkVersion` 用点分制（`"26.0.0"`），不再是旧的 `"5.0.0(12)"` 形式。
- `hvigorw.bat` 是 DevEco 自带 hvigor 的薄封装；`DEVECO_STUDIO_HOME` 可覆盖安装路径。
- 缓存位置：`F:\HarmonyOS\hvigor-home`、`F:\HarmonyOS\ohpm-home`；`TEMP` 指向 `F:\FD-Room\.tmp`，不写 C 盘。
- 无签名配置时产出 `entry-default-unsigned.hap`；装真机需要签名（DevEco 里做一次「自动签名」，
  或配置 AGC 证书 / Profile → `entry-default-signed.hap`，再用 `hdc install` 安装）。

## 没有华为真机怎么调试 / 签名

1. **本地模拟器（推荐）**：DevEco Studio → 设备管理器 → Local Emulator，下载 HarmonyOS 系统镜像
   （`phone_all_x86`，本机已下 1.4 GB+），新建并启动模拟器。模拟器通过本地 hdc 暴露为设备
   （`hdc list targets` 可见），「自动签名」不再要求 IP / USB，会自动把模拟器 UDID 注册进调试 Profile。
2. **云调试（远程真机）**：DevEco 设备管理器里的远程真机入口，用华为云上的真机跑，无需自己买手机
   （需要开发者实名）。
3. **只要签名包、不装设备**：AGC 手动签名 —— 发布证书（.cer）+ 发布 Profile（.p7b，发布 Profile 不需要设备），
   本地 keytool 生成 .p12，写进 `build-profile.json5` 的 `signingConfigs`，命令行即可打出可上架的 signed HAP。

> 模拟器镜像默认落在 `C:\Users\Lico\AppData\Local\Huawei\Sdk`，如需迁到 F 盘可在 DevEco 里改 SDK 位置后重下。

（hdc 位于 `E:\installs\DevEco Studio\sdk\default\openharmony\toolchains\hdc.exe`，
安装命令：`hdc install -r entry-default-signed.hap`。）

## 下一步（阶段 5 后续）

1. 启用 ohos-rs 依赖（`crates/ohos_bridge/README.md`），交叉编译桥接 `.so`
   （`build.ps1 -WithRustBridge`）并在 `NativeBridge.ets` 切回真实 import。
2. 设备 Tab：`photoAccessHelper` 查询相册 + 缩略图网格 + 全屏预览。
3. 编辑 Tab：Preferences / 关系型数据库记录已编辑文件与预设名。
4. 我的 Tab：缓存管理（CacheManager）、预设导入导出、主题跟随系统。
5. 编辑界面：底部弹出工具面板 + GPU 预览。
