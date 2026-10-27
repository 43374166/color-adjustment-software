# iOS 切「RAW」/ 进出编辑页卡顿（9999+ms）定位与修复

> 现象：设备页筛选从「全部」切到「RAW」，主线程卡住十几秒（Xcode hang 报告 9999+ ms），
> 之后才逐格出现 RAW 缩略图；点进一张 RAW 编辑要十几秒才出图，退出再进又要十几秒。

三条独立链路叠加，下面按「症状 → 根因 → 改法」记录，方便回归时对照。

## 1. 切筛选时主线程 XPC 风暴

**根因**

- `DeviceTabView.swift` 的 `visibleAssets` 是 `body` 里的计算属性，过滤时对**整库每一张**调
  `model.isRaw(asset)`；而 `PHAsset.isRaw` 内部先取 `displayName`（`PHAssetResource.assetResources`）
  再查一次资源，等于每张照片 **2 次跨进程查询**。
- 5000 张照片 ≈ 1 万次 XPC，全部压在主线程 → 10 秒级 hang。
- `loadAssets()` / `photoLibraryDidChange` 还会清空 `nameCache` / `rawCache`，所以导出到相册、
  拍一张照片之后，下一次切 RAW 会再卡一遍。

**改法**（`PhotoIndex.swift` + `DeviceTabView.swift`）

- 新增 `PhotoIndexEntry`（文件名 + 是否 RAW）与 `PhotoIndexStore`：索引落到 Caches 目录，
  冷启动先同步读盘（1 万张约几百 KB / 十几毫秒），`body` 里只做字典查询。
- `PhotoLibraryModel` 增加 `index` / `indexProgress`：`loadAssets` 之后在
  `fdxmp.ios.photo-index-build` 队列上**后台**补索引，只查缓存里没有的照片（新增 / 导入），
  每 96 张回报一次进度；工具栏显示「正在识别照片格式…」。
- 一次 `PHAssetResource.assetResources` 同时拿文件名和 RAW 判定（`PhotoRaw.isRawResource`），
  从 2 次查询降到 1 次。
- `visibleAssets` 改为纯内存过滤；「最新优先」直接用相册返回顺序（本来就按创建时间倒序），
  「最早优先」反转，不再每次 body 都排序。

**预期**：切 RAW 从「10s 级主线程卡死」变成「毫秒级过滤」；首次安装的后台建索引不影响交互。

## 2. 缩略图没有缓存，重新进页面整屏重来

**根因**

- `AssetThumbnail` 每次 `onAppear` 都重新 `requestImage`，`onDisappear` 立刻取消；
  进出编辑页 / 全屏预览一走，整屏格子全部重新排队。
- 用的是 `PHCachingImageManager`，但从来没调过 `startCachingImages`，等于没有预取：
  切到 RAW 时几十上百张 RAW 缩略图只能一张一张排队让 PhotoKit 渲染。
- 首屏渲染本身省不掉（PhotoKit 第一次给某个 RAW 出缩略图要读内嵌预览 + 渲染），
  但**重复**渲染完全可以省。

**改法**（`PhotoIndex.swift` + `DeviceTabView.swift`）

- 新增 `ThumbnailCache`（`NSCache`，96MB 上限）：命中直接出图；只缓存非降质图，
  避免缓存里全是糊图。
- 统一缩略图参数 `PhotoLibraryModel.thumbnailOptions`（沿用 1.1.0 的
  `opportunistic` + `exact`，不动清晰度），请求与预取共用同一份参数。
- `warmThumbnails` 在筛选 / 排序 / 搜索变化后预取首屏约 30 张（`startCachingImages`），
  换批次时先 `stopCachingImagesForAllAssets`，不会无限堆积。

**预期**：退出编辑页 / 关预览再回来立即出图（命中缓存，零渲染）；
首次切 RAW 因为预取能并行起跑，首屏填充更快。

## 3. 编辑页 RAW 解码链路每次重跑

**根因**

- `EditorModel` 是 `EditorTabView` 的 `@StateObject`，退出编辑页就销毁 → 再进：
  `PhotoRaw.requestData`（iCloud「优化存储」下要下载几十 MB 原始文件）→ LibRaw 解调 →
  RGBA 转换 → 首帧渲染，整条链路重来。
- 出图后**立刻**再解一版 2560 细节缓冲（12MP 以下的 RAW 走 `fullSize: true` 全尺寸解调）：
  白白多花几秒 CPU + 一两百 MB 内存，正好卡在用户刚进来要拖滑块的时候；
  而用户可能根本不放大，这份成本是纯投机。

**改法**（`RawFrameCache.swift` + `EditorModel.swift`）

- 新增 `RawFrameCache`（`NSCache`，160MB / 2 条上限）：按「资产 id + 修改时间 + 预览尺寸」
  缓存原始字节 + 预览缓冲 +（可选）细节缓冲；收到内存警告由 NSCache 自动清空。
- `EditorModel.load` 先查缓存：命中就直接用缓冲渲染首帧，跳过下载与解调（十几秒 → 几十毫秒）。
- 首次解码完成后写入缓存；细节缓冲不再预解，改到 `requestDetailPreview` 第一次放大时再解
  （分辨率规则不变：≤24MP 全尺寸、更大的半尺寸，保证 2560 细节帧不缩水），
  解好后回填缓存并自动重渲染一帧。

**预期**：同一张 RAW 第二次进入只剩一次首帧渲染；首次进入不再有那次投机的 2560 解调，
进图后的滑块 / 缩放不会再有几秒钟被后台解调占着渲染队列。

## 验证方法

1. 设备页：准备几千张照片的相册（含 RAW）。
   - 切「RAW」→ 观察是否还有主线程卡顿（Xcode Debug Navigator 的 CPU / 内存曲线，
     或 Instruments 的 Time Profiler 里 `PHAssetResource.assetResources` 是否还出现在主线程）。
   - 首次安装后看工具栏进度条；第二次冷启动应该直接秒切。
2. 网格：切到 RAW 后记录首屏出图时间；退出编辑页 / 关全屏预览再回来，应立刻出图（缓存命中）。
3. 编辑页：同一张 RAW 进入 → 退出 → 再进入，对比第二次的加载时间；看 Xcode 内存曲线
   （`RawFrameCache` 约等于 原始文件 + 25MB）。
4. 放大 1:1：第一次放大应立刻出现（预览缓冲上采样），1–3 秒后自动换成 2560 精细帧。

## 已知边界 / 后续可做

- 首次切到 RAW 时，PhotoKit 仍要为屏幕上的每张 RAW 生成一次缩略图（这是系统行为，
  省不掉），所以首屏仍可能有 1–3 秒的逐格填充；但主线程不再冻结、不会再出现 9999+ms，
  而且同一批照片第二次进来直接命中缓存（0 渲染）。
- 首次打开一张**从未打开过**的 RAW 仍然要真实解调（12MP 半尺寸约 1–3s，48MP ProRAW 更久），
  这是 LibRaw 的固有成本。想更快可以先用 `use_embedded_preview` 出一版相机内嵌预览占位，
  再换真解调，但会和最终色彩管线有观感跳变（`crates/core/src/preview.rs` 里明确选择不用内嵌预览）。
- iCloud「优化存储」下首次仍要下载原始文件，可以给 `PHAssetResourceRequestOptions.progressHandler`
  接一个「正在从 iCloud 下载 RAW…」的进度提示。
- 两个缓存的 `totalCostLimit` 是按常见机型估的，可按真机 `Instruments → Allocations` 实测再调。
