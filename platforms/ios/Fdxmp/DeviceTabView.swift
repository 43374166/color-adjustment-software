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

final class PhotoLibraryModel: NSObject, ObservableObject, PHPhotoLibraryChangeObserver {
    @Published var authorization: PHAuthorizationStatus =
        PHPhotoLibrary.authorizationStatus(for: .readWrite)
    @Published var assets: [PHAsset] = []

    let imageManager = PHCachingImageManager()

    /// 文件名缓存（`PHAssetResource` 查询较慢，网格/搜索会频繁用到）。
    private var nameCache: [String: String] = [:]
    /// RAW 判定缓存（同上，避免每次渲染都查资源）。
    private var rawCache: [String: Bool] = [:]

    override init() {
        super.init()
        PHPhotoLibrary.shared().register(self)
    }

    deinit {
        PHPhotoLibrary.shared().unregisterChangeObserver(self)
    }

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
        result.enumerateObjects { asset, _, _ in
            list.append(asset)
        }
        nameCache.removeAll()
        rawCache.removeAll()
        assets = list
    }

    /// 原始文件名（带缓存）。
    func fileName(for asset: PHAsset) -> String {
        if let cached = nameCache[asset.localIdentifier] {
            return cached
        }
        let name = PHAssetResource.assetResources(for: asset).first?.originalFilename
            ?? "未命名照片"
        nameCache[asset.localIdentifier] = name
        return name
    }

    /// 是否 RAW（带缓存）。
    func isRaw(_ asset: PHAsset) -> Bool {
        if let cached = rawCache[asset.localIdentifier] {
            return cached
        }
        let value = asset.isRaw
        rawCache[asset.localIdentifier] = value
        return value
    }

    func photoLibraryDidChange(_ changeInstance: PHChange) {
        DispatchQueue.main.async { [weak self] in
            self?.loadAssets()
        }
    }
}

extension PHAsset {
    var displayName: String {
        PHAssetResource.assetResources(for: self).first?.originalFilename
            ?? "未命名照片"
    }

    var isRaw: Bool {
        let name = displayName.lowercased()
        let rawExtensions = [
            ".dng", ".cr2", ".cr3", ".nef", ".nrw", ".arw", ".raf", ".rw2",
            ".orf", ".pef", ".srw", ".raw", ".rwl", ".x3f", ".3fr",
        ]
        if rawExtensions.contains(where: { name.hasSuffix($0) }) {
            return true
        }
        return PHAssetResource.assetResources(for: self).contains {
            $0.uniformTypeIdentifier.lowercased().contains("raw")
        }
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
        let options = PHImageRequestOptions()
        // 缩略图用 fastFormat：RAW 时明显更快，240px 尺寸下画质损失可忽略。
        options.deliveryMode = .fastFormat
        options.resizeMode = .fast
        options.isNetworkAccessAllowed = true
        requestId = model.imageManager.requestImage(
            for: asset,
            targetSize: targetSize,
            contentMode: .aspectFill,
            options: options
        ) { result, _ in
            guard let result else { return }
            DispatchQueue.main.async {
                image = result
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
    let onOpenEditor: (PHAsset) -> Void

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

    private var visibleAssets: [PHAsset] {
        let tokens = searchTokens
        var list = model.assets.filter { asset in
            let matchesQuery = tokens.isEmpty
                || tokens.contains { token in
                    model.fileName(for: asset).localizedCaseInsensitiveContains(token)
                }
            let matchesFilter: Bool
            switch filter {
            case .all:
                matchesFilter = true
            case .raw:
                matchesFilter = model.isRaw(asset)
            case .standard:
                matchesFilter = !model.isRaw(asset)
            }
            return matchesQuery && matchesFilter
        }
        switch sort {
        case .newest:
            list.sort { ($0.creationDate ?? .distantPast) > ($1.creationDate ?? .distantPast) }
        case .oldest:
            list.sort { ($0.creationDate ?? .distantPast) < ($1.creationDate ?? .distantPast) }
        case .name:
            list.sort {
                model.fileName(for: $0).localizedStandardCompare(model.fileName(for: $1))
                    == .orderedAscending
            }
        }
        return list
    }

    /// 搜索词：支持逗号（中英文）分隔多个文件名，任意一个命中即显示。
    private var searchTokens: [String] {
        searchText
            .split(whereSeparator: { $0 == "," || $0 == "，" })
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }
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
            if selectionMode {
                selectionBar
            }
            if model.assets.isEmpty {
                emptyState
            } else if gridMode {
                grid
            } else {
                list
            }
            bottomBar
        }
        .fullScreenCover(item: Binding(
            get: { previewId.map(PreviewTarget.init) },
            set: { previewId = $0?.id }
        )) { target in
            PhotoPreviewView(assets: visibleAssets, startId: target.id) { asset in
                previewId = nil
                onOpenEditor(asset)
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
                                    targetSize: CGSize(width: 240, height: 240)
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
                    .onLongPressGesture {
                        enterSelection(asset)
                    }
                }
            }
            .padding(2)
        }
        .background(Color(white: 0.06))
    }

    private var list: some View {
        List(visibleAssets, id: \.localIdentifier) { asset in
            HStack(spacing: 12) {
                AssetThumbnail(asset: asset, targetSize: CGSize(width: 120, height: 120))
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
            .onLongPressGesture {
                enterSelection(asset)
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
            // 与 Android 一致：点缩略图直接进入编辑页。
            onOpenEditor(asset)
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
