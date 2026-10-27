import Photos
import PhotosUI
import SwiftUI

/// 设备 Tab（iOS）：相册权限 → 网格 / 列表 → 搜索 / 排序 / 筛选 →
/// 长按多选 → 全屏预览 → 导入 / 相机 / 分享。
///
/// 说明：iOS 工程需要在 macOS + Xcode 上编译验证；本文件与 Android 端
/// MainActivity/DeviceTab 保持同一套交互与状态机。

// MARK: - 排序 / 筛选

enum DeviceSort: String, CaseIterable, Identifiable {
    case newest = "最新优先"
    case oldest = "最早优先"
    case name = "文件名"

    var id: String { rawValue }
}

enum DeviceFilter: String, CaseIterable, Identifiable {
    case all = "全部"
    case raw = "RAW"
    case standard = "常见格式"

    var id: String { rawValue }
}

// MARK: - 相册数据源

/// 相册数据源。
///
/// 性能约束（改动前 iOS 上切「RAW」筛选会卡死十几秒，Xcode 报 9999+ms）：
/// `PHAssetResource.assetResources(for:)` 是跨进程查询，每张约 0.3–3ms、iCloud 库更慢；
/// 旧代码在 `body` 里对整库逐张调用（`PHAsset.isRaw` 查一次、`displayName` 又查一次），
/// 5000 张照片 = 上万次 XPC 全压在主线程上。现在文件名 / 是否 RAW 先读磁盘索引，
/// 后台只补新增照片，主线程只查字典。
final class PhotoLibraryModel: NSObject, ObservableObject, PHPhotoLibraryChangeObserver {
    @Published var authorization: PHAuthorizationStatus =
        PHPhotoLibrary.authorizationStatus(for: .readWrite)
    @Published private(set) var assets: [PHAsset] = []
    /// 元信息索引（localIdentifier → 文件名 / 是否 RAW）。
    @Published private(set) var index: [String: PhotoIndexEntry] = PhotoIndexStore.load()
    /// 后台补索引进度（0..1；1 = 完成）。
    @Published private(set) var indexProgress: Double = 1

    let imageManager = PHCachingImageManager()

    private let indexQueue = DispatchQueue(
        label: "fdxmp.ios.photo-index-build",
        qos: .userInitiated
    )
    /// 索引代际（换库 / 重新加载后旧的构建结果作废）。
    private var indexToken = 0
    /// 已预取的缩略图批次（避免同一屏反复 startCaching）。
    private var cachedIds: [String] = []
    private var cachedSize = CGSize.zero

    override init() {
        super.init()
        PHPhotoLibrary.shared().register(self)
    }

    deinit {
        PHPhotoLibrary.shared().unregisterChangeObserver(self)
    }

    /// 索引是否已建完（RAW 筛选 / 搜索可以直接用）。
    var indexComplete: Bool { indexProgress >= 1 }

    func requestAuthorizationAndLoad() {
        let status = PHPhotoLibrary.authorizationStatus(for: .readWrite)
        authorization = status
        switch status {
        case .authorized, .limited:
            loadAssets()
        case .notDetermined:
            PHPhotoLibrary.requestAuthorization(for: .readWrite) { [weak self] newStatus in
                DispatchQueue.main.async {
                    self?.authorization = newStatus
                    if newStatus == .authorized || newStatus == .limited {
                        self?.loadAssets()
                    }
                }
            }
        default:
            break
        }
    }

    func loadAssets() {
        let options = PHFetchOptions()
        options.sortDescriptors = [NSSortDescriptor(key: "creationDate", ascending: false)]
        let result = PHAsset.fetchAssets(with: .image, options: options)
        var list: [PHAsset] = []
        list.reserveCapacity(result.count)
        result.enumerateObjects { asset, _, _ in
            list.append(asset)
        }
        assets = list
        rebuildIndex(for: list)
    }

    /// 后台补索引：只查缓存里没有的照片（新增 / 刚导入），不重跑整库。
    private func rebuildIndex(for list: [PHAsset]) {
        indexToken &+= 1
        let token = indexToken
        let known = index
        // 删掉已经不在相册里的条目，避免缓存无限增长。
        let aliveIds = Set(list.map { $0.localIdentifier })
        let base = known.filter { aliveIds.contains($0.key) }

        var missing: [PHAsset] = []
        for asset in list where known[asset.localIdentifier] == nil {
            missing.append(asset)
        }
        guard !missing.isEmpty else {
            indexProgress = 1
            if base.count != known.count {
                index = base
                PhotoIndexStore.save(base)
            }
            return
        }

        indexProgress = 0
        indexQueue.async { [weak self] in
            var built: [String: PhotoIndexEntry] = [:]
            built.reserveCapacity(missing.count)
            var done = 0
            var published = 0
            for asset in missing {
                built[asset.localIdentifier] = PhotoLibraryModel.makeEntry(for: asset)
                done += 1
                // 每 96 张回报一次进度：足够平滑，又不会把主线程刷爆。
                if done - published >= 96 {
                    published = done
                    let progress = Double(done) / Double(missing.count)
                    DispatchQueue.main.async {
                        guard let self, self.indexToken == token else { return }
                        self.indexProgress = progress
                    }
                }
            }
            DispatchQueue.main.async {
                guard let self, self.indexToken == token else { return }
                var merged = base
                merged.merge(built) { _, new in new }
                self.index = merged
                self.indexProgress = 1
                PhotoIndexStore.save(merged)
            }
        }
    }

    /// 一次 `PHAssetResource` 查询同时拿到文件名与 RAW 判定。
    private static func makeEntry(for asset: PHAsset) -> PhotoIndexEntry {
        let resources = PHAssetResource.assetResources(for: asset)
        let name = resources.first?.originalFilename ?? "未命名照片"
        PhotoNames.remember(name, for: asset)
        let isRaw = resources.contains { PhotoRaw.isRawResource($0) }
        return PhotoIndexEntry(name: name, isRaw: isRaw)
    }

    /// 原始文件名（索引查询，主线程安全）。
    func fileName(for asset: PHAsset) -> String {
        index[asset.localIdentifier]?.name ?? ""
    }

    /// 是否 RAW（索引查询，主线程安全）。
    func isRaw(_ asset: PHAsset) -> Bool {
        index[asset.localIdentifier]?.isRaw ?? false
    }

    /// 索引里是否已经有这张照片（还没轮到时不参与 RAW / 常见格式筛选）。
    func isIndexed(_ asset: PHAsset) -> Bool {
        index[asset.localIdentifier] != nil
    }

    // MARK: 缩略图

    /// 缩略图请求参数（与 `warmThumbnails` 的预取参数必须一致）。
    ///
    /// 沿用 1.1.0 实机对比后定下的 `.opportunistic` + `.exact`（先快图再清晰图），
    /// 不靠降清晰度换速度；提速靠三件事：内存缓存、预取、以及不再在主线程逐张查资源。
    static let thumbnailOptions: PHImageRequestOptions = {
        let options = PHImageRequestOptions()
        options.deliveryMode = .opportunistic
        options.resizeMode = .exact
        options.isNetworkAccessAllowed = true
        options.version = .current
        return options
    }()

    func thumbnailKey(for asset: PHAsset, size: CGSize) -> String {
        "\(asset.localIdentifier)|\(Int(size.width))x\(Int(size.height))"
    }

    func cachedThumbnail(for asset: PHAsset, size: CGSize) -> UIImage? {
        ThumbnailCache.shared.image(for: thumbnailKey(for: asset, size: size))
    }

    func storeThumbnail(_ image: UIImage, for asset: PHAsset, size: CGSize) {
        ThumbnailCache.shared.store(image, for: thumbnailKey(for: asset, size: size))
    }

    /// 预取一屏缩略图（筛完 / 排序后可见的前几十张），滚动到时直接命中。
    func warmThumbnails(_ list: [PHAsset], size: CGSize) {
        let ids = list.map { $0.localIdentifier }
        if ids == cachedIds, size == cachedSize { return }
        if !cachedIds.isEmpty {
            imageManager.stopCachingImagesForAllAssets()
        }
        cachedIds = ids
        cachedSize = size
        guard !list.isEmpty else { return }
        imageManager.startCachingImages(
            for: list,
            targetSize: size,
            contentMode: .aspectFill,
            options: Self.thumbnailOptions
        )
    }

    func photoLibraryDidChange(_ changeInstance: PHChange) {
        DispatchQueue.main.async { [weak self] in
            self?.loadAssets()
        }
    }
}

extension PHAsset {
    /// 原始文件名（进程内缓存，界面反复读也不会重复查 PhotoKit）。
    var displayName: String {
        PhotoNames.name(for: self)
    }

    var pixelDescription: String {
        "\(pixelWidth) × \(pixelHeight)"
    }

    var createdDescription: String {
        guard let date = creationDate else { return "时间未知" }
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter.string(from: date)
    }
}

// MARK: - 缩略图

struct AssetThumbnail: View {
    let asset: PHAsset
    let targetSize: CGSize

    @EnvironmentObject private var model: PhotoLibraryModel
    @State private var image: UIImage?
    @State private var requestId: PHImageRequestID?

    var body: some View {
        ZStack {
            Color(white: 0.10)
            if let image {
                Image(uiImage: image)
                    .resizable()
                    .scaledToFill()
            } else {
                Image(systemName: "photo")
                    .foregroundStyle(.gray)
            }
        }
        .clipped()
        .onAppear(perform: load)
        .onDisappear(perform: cancel)
    }

    private func load() {
        // 拿到强引用再进异步回调：视图结构体销毁后 environment 已经不可靠。
        let library = model
        // 命中缓存直接出图：退出编辑页 / 关掉预览再回来不再重新排队渲染。
        if let cached = library.cachedThumbnail(for: asset, size: targetSize) {
            image = cached
            return
        }
        // opportunistic 会回调两次：先一版快图保证不空屏，后到的清晰版覆盖并进缓存。
        requestId = library.imageManager.requestImage(
            for: asset,
            targetSize: targetSize,
            contentMode: .aspectFill,
            options: PhotoLibraryModel.thumbnailOptions
        ) { result, info in
            guard let result else { return }
            let degraded = (info?[PHImageResultIsDegradedKey] as? Bool) ?? false
            DispatchQueue.main.async {
                image = result
                if !degraded {
                    library.storeThumbnail(result, for: asset, size: targetSize)
                }
            }
        }
    }

    private func cancel() {
        if let requestId {
            model.imageManager.cancelImageRequest(requestId)
        }
        requestId = nil
    }
}

// MARK: - 全屏预览

struct AssetFullImage: View {
    let asset: PHAsset

    @EnvironmentObject private var model: PhotoLibraryModel
    @State private var image: UIImage?
    @State private var scale: CGFloat = 1

    var body: some View {
        GeometryReader { geometry in
            ZStack {
                Color.black
                if let image {
                    Image(uiImage: image)
                        .resizable()
                        .scaledToFit()
                        .scaleEffect(scale)
                        .gesture(
                            MagnificationGesture()
                                .onChanged { value in
                                    scale = min(max(value, 1), 6)
                                }
                                .onEnded { _ in
                                    if scale < 1.05 { scale = 1 }
                                }
                        )
                        .onTapGesture(count: 2) {
                            withAnimation {
                                scale = scale > 1 ? 1 : 2.5
                            }
                        }
                } else {
                    ProgressView().tint(.white)
                }
            }
            .onAppear {
                load(target: geometry.size)
            }
        }
    }

    private func load(target: CGSize) {
        let options = PHImageRequestOptions()
        options.deliveryMode = .highQualityFormat
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = true
        let scaleFactor = UIScreen.main.scale
        let size = CGSize(
            width: min(target.width * scaleFactor, 2048),
            height: min(target.height * scaleFactor, 2048)
        )
        model.imageManager.requestImage(
            for: asset,
            targetSize: size,
            contentMode: .aspectFit,
            options: options
        ) { result, _ in
            guard let result else { return }
            DispatchQueue.main.async {
                image = result
            }
        }
    }
}

struct PhotoPreviewView: View {
    let assets: [PHAsset]
    let onEdit: (PHAsset) -> Void

    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var model: PhotoLibraryModel
    @State private var currentId: String
    @State private var shareItems: [Any] = []
    @State private var showShare = false

    init(assets: [PHAsset], startId: String, onEdit: @escaping (PHAsset) -> Void) {
        self.assets = assets
        self.onEdit = onEdit
        _currentId = State(initialValue: startId)
    }

    private var currentAsset: PHAsset? {
        assets.first { $0.localIdentifier == currentId }
    }

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            TabView(selection: $currentId) {
                ForEach(assets, id: \.localIdentifier) { asset in
                    AssetFullImage(asset: asset)
                        .tag(asset.localIdentifier)
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
            .ignoresSafeArea()

            VStack {
                HStack {
                    Spacer()
                    if let currentAsset {
                        Text(currentAsset.displayName)
                            .font(.footnote)
                            .foregroundStyle(.white)
                            .lineLimit(1)
                    }
                    Spacer()
                    Button {
                        dismiss()
                    } label: {
                        Image(systemName: "xmark")
                            .foregroundStyle(.white)
                    }
                }
                .padding(12)
                .background(Color.black.opacity(0.55))

                Spacer()

                HStack(spacing: 32) {
                    Button {
                        if let currentAsset {
                            dismiss()
                            onEdit(currentAsset)
                        }
                    } label: {
                        Label("编辑", systemImage: "slider.horizontal.3")
                    }
                    Button {
                        shareSelected()
                    } label: {
                        Label("分享", systemImage: "square.and.arrow.up")
                    }
                }
                .foregroundStyle(.white)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity)
                .background(Color.black.opacity(0.55))
            }
        }
        .sheet(isPresented: $showShare) {
            ActivityView(items: shareItems)
        }
    }

    private func shareSelected() {
        guard let asset = currentAsset else { return }
        let options = PHImageRequestOptions()
        options.isNetworkAccessAllowed = true
        model.imageManager.requestImage(
            for: asset,
            targetSize: PHImageManagerMaximumSize,
            contentMode: .default,
            options: options
        ) { image, _ in
            guard let image else { return }
            DispatchQueue.main.async {
                shareItems = [image]
                showShare = true
            }
        }
    }
}

// MARK: - 系统分享 / 相机

struct ActivityView: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

struct CameraPicker: UIViewControllerRepresentable {
    let onCapture: (UIImage) -> Void

    @Environment(\.dismiss) private var dismiss

    func makeUIViewController(context: Context) -> UIImagePickerController {
        let picker = UIImagePickerController()
        picker.sourceType = .camera
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ controller: UIImagePickerController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(onCapture: onCapture, dismiss: { dismiss() })
    }

    final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
        let onCapture: (UIImage) -> Void
        let dismiss: () -> Void

        init(onCapture: @escaping (UIImage) -> Void, dismiss: @escaping () -> Void) {
            self.onCapture = onCapture
            self.dismiss = dismiss
        }

        func imagePickerController(
            _ picker: UIImagePickerController,
            didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
        ) {
            if let image = info[.originalImage] as? UIImage {
                onCapture(image)
            }
            dismiss()
        }

        func imagePickerControllerDidCancel(_ picker: UIImagePickerController) {
            dismiss()
        }
    }
}

// MARK: - 设备 Tab

struct DeviceTabView: View {
    /// 打开编辑页：照片 + 当前可见列表（编辑页内可左右滑动切上一张 / 下一张）。
    let onOpenEditor: (PHAsset, [PHAsset]) -> Void

    @StateObject private var model = PhotoLibraryModel()
    @State private var searchText = ""
    @State private var sort: DeviceSort = .newest
    @State private var filter: DeviceFilter = .all
    @State private var gridMode = true
    @State private var selectionMode = false
    @State private var selectedIds = Set<String>()
    @State private var previewId: String?
    @State private var importItems: [PhotosPickerItem] = []
    @State private var showCamera = false
    @State private var message: String?

    /// 当前可见照片。
    ///
    /// 只做内存字典查询 + 过滤：切「RAW」时不再逐张跨进程查 PhotoKit
    /// （那是之前卡死十几秒的原因），1 万张的过滤 / 排序都在毫秒级。
    private var visibleAssets: [PHAsset] {
        let tokens = searchTokens
        let list = model.assets.filter { asset in
            if !tokens.isEmpty {
                let name = model.fileName(for: asset)
                let hit = tokens.contains { name.localizedCaseInsensitiveContains($0) }
                guard hit else { return false }
            }
            switch filter {
            case .all:
                return true
            case .raw:
                return model.isRaw(asset)
            case .standard:
                // 还没进索引的照片先不参与「常见格式」，避免一闪而过。
                return model.isIndexed(asset) && !model.isRaw(asset)
            }
        }
        // 相册本身按创建时间倒序取回：最新优先不用再排，最早优先直接反转。
        switch sort {
        case .newest:
            return list
        case .oldest:
            return Array(list.reversed())
        case .name:
            return list.sorted {
                model.fileName(for: $0).localizedStandardCompare(model.fileName(for: $1))
                    == .orderedAscending
            }
        }
    }

    /// 搜索词：支持逗号（中英文）分隔多个文件名，任意一个命中即显示。
    private var searchTokens: [String] {
        searchText
            .split(whereSeparator: { $0 == "," || $0 == "，" })
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
    }

    /// 缩略图预取的触发键：筛选 / 排序 / 搜索 / 索引完成状态变化时重新预取一屏。
    private var warmKey: String {
        [
            filter.rawValue,
            sort.rawValue,
            searchText,
            "\(model.assets.count)",
            "\(model.indexComplete)",
        ].joined(separator: "|")
    }

    var body: some View {
        Group {
            switch model.authorization {
            case .authorized, .limited:
                gallery
            default:
                permissionGuide
            }
        }
        // 网格 / 列表 / 全屏预览里的缩略图使用 @EnvironmentObject 读取该 model，
        // 必须在根节点注入，否则授权成功后渲染缩略图会直接崩溃。
        .environmentObject(model)
        .background(Color(white: 0.06))
        .onAppear {
            model.requestAuthorizationAndLoad()
        }
    }

    private var permissionGuide: some View {
        VStack(spacing: 14) {
            Image(systemName: "photo.on.rectangle")
                .font(.system(size: 48))
                .foregroundStyle(Color.accentColor)
            Text("访问你的照片")
                .font(.title3)
            Text("需要相册权限才能浏览和编辑设备上的照片。如果系统不再弹出授权框，请到 设置 → fdxmp 里手动开启。")
                .font(.footnote)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button("授予相册权限") {
                model.requestAuthorizationAndLoad()
            }
            .buttonStyle(.borderedProminent)
        }
        .padding(32)
    }

    private var gallery: some View {
        VStack(spacing: 0) {
            toolbar
            if model.assets.isEmpty {
                emptyState
            } else if visibleAssets.isEmpty {
                noMatchState
            } else if gridMode {
                grid
            } else {
                list
            }
        }
        .fullScreenCover(item: Binding(
            get: { previewId.map(PreviewTarget.init) },
            set: { previewId = $0?.id }
        )) { target in
            PhotoPreviewView(assets: visibleAssets, startId: target.id) { asset in
                previewId = nil
                onOpenEditor(asset, visibleAssets)
            }
            .environmentObject(model)
        }
        .fullScreenCover(isPresented: $showCamera) {
            CameraPicker { image in
                saveCameraImage(image)
            }
        }
        .alert(item: Binding(
            get: { message.map(MessageTarget.init) },
            set: { message = $0?.text }
        )) { target in
            Alert(title: Text(target.text), dismissButton: .default(Text("好")))
        }
        .onChange(of: importItems) { items in
            if !items.isEmpty {
                message = "已选择 \(items.count) 张照片（导入到编辑队列将在编辑页接入）"
                importItems = []
            }
        }
    }

    private var toolbar: some View {
        VStack(spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(.secondary)
                TextField("搜索文件名（多个用逗号分隔）", text: $searchText)
                    .textFieldStyle(.plain)
                if !searchText.isEmpty {
                    Button {
                        searchText = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(10)
            .background(Color(white: 0.12))
            .clipShape(RoundedRectangle(cornerRadius: 10))

            HStack(spacing: 8) {
                Picker("排序", selection: $sort) {
                    ForEach(DeviceSort.allCases) { option in
                        Text(option.rawValue).tag(option)
                    }
                }
                .pickerStyle(.menu)

                Picker("筛选", selection: $filter) {
                    ForEach(DeviceFilter.allCases) { option in
                        Text(option.rawValue).tag(option)
                    }
                }
                .pickerStyle(.segmented)

                Button {
                    gridMode.toggle()
                } label: {
                    Image(systemName: gridMode ? "list.bullet" : "square.grid.2x2")
                }
                .buttonStyle(.bordered)
            }

            // 首次安装 / 大量新照片：后台补索引时给一条进度，点「RAW」不会再卡住。
            if !model.indexComplete {
                HStack(spacing: 8) {
                    ProgressView(value: model.indexProgress)
                        .tint(Color.accentColor)
                    Text("正在识别照片格式…")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .fixedSize()
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var selectionBar: some View {
        HStack {
            Text("已选 \(selectedIds.count) 张")
                .font(.footnote)
            Spacer()
            Button("全选") {
                selectedIds = Set(visibleAssets.map { $0.localIdentifier })
            }
            .font(.footnote)
            Button("分享") {
                shareSelected()
            }
            .font(.footnote)
            .disabled(selectedIds.isEmpty)
            Button("完成") {
                selectionMode = false
                selectedIds = []
            }
            .font(.footnote)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(Color(white: 0.12))
    }

    private var grid: some View {
        ScrollView {
            LazyVGrid(
                columns: Array(repeating: GridItem(.flexible(), spacing: 2), count: 3),
                spacing: 2
            ) {
                ForEach(visibleAssets, id: \.localIdentifier) { asset in
                    ZStack(alignment: .topTrailing) {
                        // 固定 1:1 方格：用 Color 撑出方形再叠加缩略图，
                        // 否则单元格高度会跟随图片自身比例，网格会参差不齐。
                        Color(white: 0.10)
                            .aspectRatio(1, contentMode: .fit)
                            .overlay {
                                AssetThumbnail(
                                    asset: asset,
                                    targetSize: CGSize(width: 480, height: 480)
                                )
                            }
                            .clipShape(RoundedRectangle(cornerRadius: 4))
                        if model.isRaw(asset) {
                            Text("RAW")
                                .font(.system(size: 9, weight: .semibold))
                                .padding(.horizontal, 4)
                                .padding(.vertical, 1)
                                .background(Color.black.opacity(0.7))
                                .clipShape(RoundedRectangle(cornerRadius: 3))
                                .padding(4)
                                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                        }
                        if selectionMode {
                            Image(systemName: selectedIds.contains(asset.localIdentifier)
                                ? "checkmark.circle.fill" : "circle")
                                .foregroundStyle(selectedIds.contains(asset.localIdentifier)
                                    ? Color.accentColor : Color.white.opacity(0.8))
                                .padding(4)
                        }
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        handleTap(asset)
                    }
                }
            }
            .padding(2)
        }
        .background(Color(white: 0.06))
        // 预取首屏缩略图：滚到 / 切回来时直接命中缓存，不再一格一格排队渲染。
        // 只预取约一屏半（30 张）：再多会和屏幕上的请求抢 PhotoKit 的渲染队列，
        // RAW 反而更慢。
        .task(id: warmKey) {
            model.warmThumbnails(
                Array(visibleAssets.prefix(30)),
                size: CGSize(width: 480, height: 480)
            )
        }
    }

    private var list: some View {
        List(visibleAssets, id: \.localIdentifier) { asset in
            HStack(spacing: 12) {
                AssetThumbnail(asset: asset, targetSize: CGSize(width: 240, height: 240))
                    .frame(width: 56, height: 56)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                VStack(alignment: .leading, spacing: 3) {
                    Text(model.fileName(for: asset))
                        .font(.subheadline)
                        .lineLimit(1)
                    Text("\(asset.createdDescription) · \(asset.pixelDescription)\(model.isRaw(asset) ? " · RAW" : "")")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if selectionMode {
                    Image(systemName: selectedIds.contains(asset.localIdentifier)
                        ? "checkmark.circle.fill" : "circle")
                        .foregroundStyle(selectedIds.contains(asset.localIdentifier)
                            ? Color.accentColor : Color.secondary)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                handleTap(asset)
            }
        }
        .listStyle(.plain)
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "photo.on.rectangle")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text("相册里还没有照片")
                .font(.headline)
            Text("可以用下方相机拍一张，或把照片同步到设备相册")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// 有照片、但当前筛选 / 搜索没有命中（例如索引还没建完就看到 RAW 页）。
    private var noMatchState: some View {
        VStack(spacing: 10) {
            Image(systemName: "line.3.horizontal.decrease.circle")
                .font(.system(size: 36))
                .foregroundStyle(.secondary)
            Text(model.indexComplete ? "没有符合当前筛选的照片" : "正在识别照片格式…")
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var bottomBar: some View {
        HStack {
            PhotosPicker(
                selection: $importItems,
                maxSelectionCount: 50,
                matching: .images
            ) {
                Label("导入", systemImage: "photo.badge.plus")
            }
            .frame(maxWidth: .infinity)

            Button {
                if UIImagePickerController.isSourceTypeAvailable(.camera) {
                    showCamera = true
                } else {
                    message = "当前设备没有可用相机（模拟器上用真机验证）"
                }
            } label: {
                Label("相机", systemImage: "camera")
            }
            .frame(maxWidth: .infinity)

            Button {
                shareSelected()
            } label: {
                Label(
                    selectedIds.isEmpty ? "分享" : "分享(\(selectedIds.count))",
                    systemImage: "square.and.arrow.up"
                )
            }
            .frame(maxWidth: .infinity)
            .disabled(selectedIds.isEmpty)
        }
        .font(.footnote)
        .padding(.vertical, 10)
        .background(Color(white: 0.12))
    }

    private func handleTap(_ asset: PHAsset) {
        if selectionMode {
            toggleSelection(asset)
        } else {
            // 与 Android 一致：点缩略图直接进入编辑页（连同当前列表，编辑页可左右滑动切图）。
            onOpenEditor(asset, visibleAssets)
        }
    }

    private func enterSelection(_ asset: PHAsset) {
        selectionMode = true
        selectedIds.insert(asset.localIdentifier)
    }

    private func toggleSelection(_ asset: PHAsset) {
        if selectedIds.contains(asset.localIdentifier) {
            selectedIds.remove(asset.localIdentifier)
        } else {
            selectedIds.insert(asset.localIdentifier)
        }
    }

    private func saveCameraImage(_ image: UIImage) {
        PHPhotoLibrary.shared().performChanges {
            PHAssetChangeRequest.creationRequestForAsset(from: image)
        } completionHandler: { success, _ in
            DispatchQueue.main.async {
                if success {
                    model.loadAssets()
                    message = "照片已保存到相册"
                } else {
                    message = "保存照片失败"
                }
            }
        }
    }

    private func shareSelected() {
        let targets = visibleAssets.filter { selectedIds.contains($0.localIdentifier) }
        guard !targets.isEmpty else {
            message = "长按照片进入多选后再分享"
            return
        }
        let options = PHImageRequestOptions()
        options.isNetworkAccessAllowed = true
        let group = DispatchGroup()
        var images: [UIImage] = []
        for asset in targets {
            group.enter()
            model.imageManager.requestImage(
                for: asset,
                targetSize: PHImageManagerMaximumSize,
                contentMode: .default,
                options: options
            ) { image, _ in
                if let image {
                    images.append(image)
                }
                group.leave()
            }
        }
        group.notify(queue: .main) {
            if images.isEmpty {
                message = "没有可分享的照片"
            } else {
                SharePresenter.present(images: images)
            }
        }
    }
}

private struct PreviewTarget: Identifiable {
    let id: String
}

private struct MessageTarget: Identifiable {
    let id: String
    let text: String

    init(_ text: String) {
        self.id = text
        self.text = text
    }
}

/// 用当前 key window 顶层控制器弹出系统分享面板。
enum SharePresenter {
    static func present(images: [UIImage]) {
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene,
              let root = scene.keyWindow?.rootViewController else {
            return
        }
        var top = root
        while let presented = top.presentedViewController {
            top = presented
        }
        let controller = UIActivityViewController(activityItems: images, applicationActivities: nil)
        top.present(controller, animated: true)
    }
}
