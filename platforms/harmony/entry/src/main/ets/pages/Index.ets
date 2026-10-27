// fdxmp HarmonyOS 主界面。
//
// 严格按规格实现三大 Tab：设备（默认）/ 编辑 / 我的，底部导航，无侧边栏。
// 设备 Tab 已接入相册（权限、网格 / 列表、搜索 / 排序 / 筛选、多选、全屏预览、
// 系统相册导入与相机入口）；编辑与我的待后续阶段。
import { router } from '@kit.ArkUI';
import { fileIo as fs } from '@kit.CoreFileKit';
import { preferences } from '@kit.ArkData';
import abilityAccessCtrl, { Permissions } from '@ohos.abilityAccessCtrl';
import common from '@ohos.app.ability.common';
import dataSharePredicates from '@ohos.data.dataSharePredicates';
import photoAccessHelper from '@ohos.file.photoAccessHelper';
import picker from '@ohos.file.picker';
import promptAction from '@ohos.promptAction';
import camera from '@ohos.multimedia.camera';
import cameraPicker from '@ohos.multimedia.cameraPicker';
import { systemShare } from '@kit.ShareKit';
import { PhotoItem } from '../model/PhotoItem';
import nativeCore from '../core/NativeBridge';

/** 最近编辑记录（编辑 Tab 用）。 */
interface RecentEntry {
  uri: string;
  name: string;
  time: number;
}

@Entry
@Component
struct Index {
  @State activeTab: number = 0;
  @State coreVersion: string = '未加载';
  @State recent: RecentEntry[] = [];
  @State cacheBytes: number = 0;
  @State exportFormat: string = 'jpeg';
  @State exportQuality: number = 95;
  // 0 = 还没请求过，1 = 已授权，2 = 被拒绝
  @State permissionState: number = 0;
  @State loading: boolean = false;
  @State photos: PhotoItem[] = [];
  @State query: string = '';
  @State sortIndex: number = 0;
  @State filterIndex: number = 0;
  @State gridMode: boolean = true;
  @State selectionMode: boolean = false;
  @State selectedUris: string[] = [];
  @State previewIndex: number = -1;

  private tabs: string[] = ['设备', '编辑', '我的'];
  private tabIcons: string[] = ['▦', '✎', '☰'];
  private filters: string[] = ['全部', 'RAW', '常见格式'];
  private permissions: Permissions[] = ['ohos.permission.READ_IMAGEVIDEO'];
  private sortOptions: SelectOption[] = [
    { value: '最新优先' },
    { value: '最早优先' },
    { value: '文件名' },
    { value: '文件大小' },
  ];

  aboutToAppear(): void {
    this.coreVersion = nativeCore.version();
    this.requestPermission();
    this.loadRecent();
    this.refreshCacheSize();
    this.loadExportSettings();
  }

  private async loadExportSettings(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const store = await preferences.getPreferences(context, 'fdxmp_export');
      this.exportFormat = await store.get('format', 'jpeg') as string;
      this.exportQuality = Number(await store.get('quality', '95'));
    } catch (error) {
    }
  }

  private async saveExportSettings(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const store = await preferences.getPreferences(context, 'fdxmp_export');
      await store.put('format', this.exportFormat);
      await store.put('quality', this.exportQuality.toString());
      await store.flush();
    } catch (error) {
    }
  }

  /* ------------------------------------------------------------ 编辑记录 / 缓存 */

  private async loadRecent(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const store = await preferences.getPreferences(context, 'fdxmp_recent');
      const raw = await store.get('items', '[]') as string;
      this.recent = JSON.parse(raw) as RecentEntry[];
    } catch (error) {
      this.recent = [];
    }
  }

  private async refreshCacheSize(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const files = fs.listFileSync(context.cacheDir);
      let total = 0;
      for (let i = 0; i < files.length; i++) {
        try {
          const stat = fs.statSync(context.cacheDir + '/' + files[i]);
          total += stat.size;
        } catch (error) {
        }
      }
      this.cacheBytes = total;
    } catch (error) {
      this.cacheBytes = 0;
    }
  }

  private async clearCache(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const files = fs.listFileSync(context.cacheDir);
      for (let i = 0; i < files.length; i++) {
        try {
          fs.unlinkSync(context.cacheDir + '/' + files[i]);
        } catch (error) {
        }
      }
      promptAction.showToast({ message: '缓存已清理' });
      this.refreshCacheSize();
    } catch (error) {
      promptAction.showToast({ message: '清理失败' });
    }
  }

  private openEditor(uri: string, name: string): void {
    router.pushUrl({ url: 'pages/EditorPage', params: { uri: uri, name: name } });
  }

  private formatTime(ms: number): string {
    const date = new Date(ms);
    const month = (date.getMonth() + 1).toString().padStart(2, '0');
    const day = date.getDate().toString().padStart(2, '0');
    const hour = date.getHours().toString().padStart(2, '0');
    const minute = date.getMinutes().toString().padStart(2, '0');
    return month + '-' + day + ' ' + hour + ':' + minute;
  }

  private formatBytes(bytes: number): string {
    if (bytes >= 1024 * 1024) {
      return (bytes / 1024 / 1024).toFixed(1) + ' MB';
    }
    if (bytes >= 1024) {
      return (bytes / 1024).toFixed(0) + ' KB';
    }
    return bytes.toString() + ' B';
  }

  /* ------------------------------------------------------------ 数据 */

  private async requestPermission(): Promise<void> {
    const context = getContext(this) as common.UIAbilityContext;
    const atManager = abilityAccessCtrl.createAtManager();
    try {
      const result = await atManager.requestPermissionsFromUser(context, this.permissions);
      let granted = false;
      for (let i = 0; i < result.authResults.length; i++) {
        if (result.authResults[i] === 0) {
          granted = true;
        }
      }
      this.permissionState = granted ? 1 : 2;
      if (granted) {
        await this.loadPhotos();
      }
    } catch (err) {
      this.permissionState = 2;
    }
  }

  private async loadPhotos(): Promise<void> {
    this.loading = true;
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const helper = photoAccessHelper.getPhotoAccessHelper(context);
      const predicates = new dataSharePredicates.DataSharePredicates();
      predicates.orderByDesc(photoAccessHelper.PhotoKeys.DATE_ADDED);
      const options: photoAccessHelper.FetchOptions = {
        fetchColumns: [
          photoAccessHelper.PhotoKeys.URI,
          photoAccessHelper.PhotoKeys.DISPLAY_NAME,
          photoAccessHelper.PhotoKeys.SIZE,
          photoAccessHelper.PhotoKeys.DATE_ADDED,
          photoAccessHelper.PhotoKeys.WIDTH,
          photoAccessHelper.PhotoKeys.HEIGHT,
        ],
        predicates: predicates,
      };
      const result = await helper.getAssets(options);
      const list: PhotoItem[] = [];
      const count = result.getCount();
      for (let i = 0; i < count; i++) {
        const asset = await result.getObjectByPosition(i);
        const name = (await asset.get(photoAccessHelper.PhotoKeys.DISPLAY_NAME)) as string;
        const size = (await asset.get(photoAccessHelper.PhotoKeys.SIZE)) as number;
        const dateAdded = (await asset.get(photoAccessHelper.PhotoKeys.DATE_ADDED)) as number;
        const width = (await asset.get(photoAccessHelper.PhotoKeys.WIDTH)) as number;
        const height = (await asset.get(photoAccessHelper.PhotoKeys.HEIGHT)) as number;
        list.push(new PhotoItem(asset.uri, name, size, dateAdded, width, height));
      }
      result.close();
      this.photos = list;
    } catch (err) {
      promptAction.showToast({ message: '读取相册失败，请检查权限' });
    }
    this.loading = false;
  }

  private visiblePhotos(): PhotoItem[] {
    const keyword = this.query.trim().toLowerCase();
    const list: PhotoItem[] = [];
    for (let i = 0; i < this.photos.length; i++) {
      const item = this.photos[i];
      if (keyword.length > 0 && !item.name.toLowerCase().includes(keyword)) {
        continue;
      }
      if (this.filterIndex === 1 && !item.isRaw()) {
        continue;
      }
      if (this.filterIndex === 2 && item.isRaw()) {
        continue;
      }
      list.push(item);
    }
    if (this.sortIndex === 1) {
      list.sort((a: PhotoItem, b: PhotoItem) => a.dateAdded - b.dateAdded);
    } else if (this.sortIndex === 2) {
      list.sort((a: PhotoItem, b: PhotoItem) => a.name.localeCompare(b.name));
    } else if (this.sortIndex === 3) {
      list.sort((a: PhotoItem, b: PhotoItem) => b.sizeBytes - a.sizeBytes);
    } else {
      list.sort((a: PhotoItem, b: PhotoItem) => b.dateAdded - a.dateAdded);
    }
    return list;
  }

  private currentPhoto(): PhotoItem | undefined {
    const list = this.visiblePhotos();
    if (this.previewIndex >= 0 && this.previewIndex < list.length) {
      return list[this.previewIndex];
    }
    return undefined;
  }

  /* ---------------------------------------------------------- 选择 */

  private isSelected(uri: string): boolean {
    return this.selectedUris.includes(uri);
  }

  private toggleSelected(uri: string): void {
    if (this.isSelected(uri)) {
      const next: string[] = [];
      for (let i = 0; i < this.selectedUris.length; i++) {
        if (this.selectedUris[i] !== uri) {
          next.push(this.selectedUris[i]);
        }
      }
      this.selectedUris = next;
    } else {
      this.selectedUris = this.selectedUris.concat([uri]);
    }
  }

  private enterSelection(uri: string): void {
    this.selectionMode = true;
    if (!this.isSelected(uri)) {
      this.selectedUris = this.selectedUris.concat([uri]);
    }
  }

  private exitSelection(): void {
    this.selectionMode = false;
    this.selectedUris = [];
  }

  private selectAll(): void {
    const list = this.visiblePhotos();
    const uris: string[] = [];
    for (let i = 0; i < list.length; i++) {
      uris.push(list[i].uri);
    }
    this.selectedUris = uris;
  }

  private onPhotoClick(item: PhotoItem, index: number): void {
    if (this.selectionMode) {
      this.toggleSelected(item.uri);
    } else {
      // 直接进调色界面（不再经过「编辑 / 分享」预览层）。
      this.openEditor(item.uri, item.name);
    }
  }

  /* -------------------------------------------------- 导入 / 相机 / 分享 */

  private async importPhotos(): Promise<void> {
    try {
      const photoPicker = new picker.PhotoViewPicker();
      const options = new picker.PhotoSelectOptions();
      options.MIMEType = picker.PhotoViewMIMETypes.IMAGE_TYPE;
      options.maxSelectNumber = 50;
      const result = await photoPicker.select(options);
      if (result.photoUris.length > 0) {
        promptAction.showToast({
          message: '已选择 ' + result.photoUris.length.toString() + ' 张（导入编辑队列阶段 6 接入）',
        });
      }
    } catch (err) {
      promptAction.showToast({ message: '打开系统相册失败' });
    }
  }

  private async takePhoto(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      const profile = new cameraPicker.PickerProfile();
      profile.cameraPosition = camera.CameraPosition.CAMERA_POSITION_BACK;
      const result = await cameraPicker.pick(
        context,
        [cameraPicker.PickerMediaType.PHOTO],
        profile,
      );
      if (result.resultCode === 0) {
        promptAction.showToast({ message: '拍摄完成，正在刷新相册' });
        await this.loadPhotos();
      }
    } catch (err) {
      promptAction.showToast({ message: '相机不可用' });
    }
  }

  private shareSelected(): void {
    const count = this.selectedUris.length;
    if (count === 0) {
      promptAction.showToast({ message: '长按照片进入多选后再分享' });
      return;
    }
    this.shareUris(this.selectedUris);
  }

  /** 调系统分享面板（多张照片一起分享）。 */
  private async shareUris(uris: string[]): Promise<void> {
    if (uris.length === 0) {
      return;
    }
    try {
      const records: systemShare.SharedRecord[] = [];
      for (let i = 0; i < uris.length && i < 9; i++) {
        const record: systemShare.SharedRecord = {
          utd: 'general.image',
          uri: uris[i]
        };
        records.push(record);
      }
      if (records.length === 0) {
        return;
      }
      const data = new systemShare.SharedData(records[0]);
      for (let i = 1; i < records.length; i++) {
        data.addRecord(records[i]);
      }
      const controller = new systemShare.ShareController(data);
      await controller.show(getContext(this) as common.UIAbilityContext, {
        previewMode: systemShare.SharePreviewMode.DETAIL,
        selectionMode: systemShare.SelectionMode.SINGLE
      });
    } catch (error) {
      promptAction.showToast({ message: '分享失败' });
    }
  }

  /* -------------------------------------------------------------- UI */

  @Builder
  topBar() {
    Row() {
      Text('fdxmp · ' + this.tabs[this.activeTab])
        .fontSize(18)
        .fontWeight(FontWeight.Medium)
        .fontColor('#E8E8E8')
      Blank()
      Text('core ' + this.coreVersion)
        .fontSize(11)
        .fontColor('#888888')
    }
    .width('100%')
    .padding({ left: 16, right: 16, top: 12, bottom: 12 })
    .backgroundColor('#141414')
  }

  @Builder
  tabBar() {
    Row() {
      ForEach(this.tabs, (label: string, index: number) => {
        Column({ space: 2 }) {
          Text(this.tabIcons[index])
            .fontSize(16)
            .fontColor(this.activeTab === index ? '#4A9EFF' : '#777777')
          Text(label)
            .fontSize(11)
            .fontColor(this.activeTab === index ? '#4A9EFF' : '#777777')
        }
        .layoutWeight(1)
        .height(52)
        .justifyContent(FlexAlign.Center)
        .onClick(() => {
          this.activeTab = index;
          if (index === 1) {
            this.loadRecent();
          } else if (index === 2) {
            this.refreshCacheSize();
          }
        })
      }, (label: string) => label)
    }
    .width('100%')
    .backgroundColor('#141414')
    .padding({ bottom: 16 })
    .borderWidth({ top: 1 })
    .borderColor('#22000000')
  }

  @Builder
  deviceTab() {
    Column() {
      if (this.permissionState !== 1) {
        this.permissionGuide()
      } else {
        this.deviceToolbar()
        if (this.selectionMode) {
          this.selectionBar()
        }
        if (this.loading) {
          Column() {
            LoadingProgress().width(48).height(48).color('#4A9EFF')
          }
          .width('100%')
          .layoutWeight(1)
          .justifyContent(FlexAlign.Center)
        } else if (this.visiblePhotos().length === 0) {
          Column({ space: 8 }) {
            Text('相册里还没有照片')
              .fontSize(15)
              .fontColor('#CCCCCC')
            Text('用下方相机拍一张，或把照片放进设备相册')
              .fontSize(12)
              .fontColor('#888888')
          }
          .width('100%')
          .layoutWeight(1)
          .justifyContent(FlexAlign.Center)
        } else if (this.gridMode) {
          this.photoGrid()
        } else {
          this.photoList()
        }
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  permissionGuide() {
    Column({ space: 12 }) {
      Text('访问你的照片')
        .fontSize(18)
        .fontColor('#E8E8E8')
      Text(this.permissionState === 2
        ? '相册权限被拒绝。请到 设置 → 应用 → fdxmp → 权限 里打开「照片和视频」，再回到这里。'
        : 'fdxmp 需要读取相册，用于网格浏览、预览和编辑。照片只在本机处理。')
        .fontSize(13)
        .fontColor('#AAAAAA')
        .textAlign(TextAlign.Center)
      Button('授予相册权限')
        .fontSize(14)
        .backgroundColor('#4A9EFF')
        .onClick(() => {
          this.requestPermission();
        })
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: 32, right: 32 })
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  deviceToolbar() {
    Column() {
      Row({ space: 8 }) {
        TextInput({ placeholder: '搜索文件名', text: this.query })
          .layoutWeight(1)
          .height(40)
          .backgroundColor('#1E1E1E')
          .fontColor('#E8E8E8')
          .placeholderColor('#888888')
          .onChange((value: string) => {
            this.query = value;
          })
        Button(this.gridMode ? '列表' : '网格')
          .fontSize(12)
          .height(40)
          .backgroundColor('#1E1E1E')
          .onClick(() => {
            this.gridMode = !this.gridMode;
          })
        Select(this.sortOptions)
          .selected(this.sortIndex)
          .value(this.sortOptions[this.sortIndex].value)
          .font({ size: 12 })
          .onSelect((index: number) => {
            this.sortIndex = index;
          })
      }
      .width('100%')
      .padding({ left: 8, right: 8, top: 6, bottom: 6 })

      Row({ space: 8 }) {
        ForEach(this.filters, (label: string, index: number) => {
          Text(label)
            .fontSize(12)
            .fontColor(this.filterIndex === index ? '#FFFFFF' : '#AAAAAA')
            .backgroundColor(this.filterIndex === index ? '#4A9EFF' : '#1E1E1E')
            .borderRadius(12)
            .padding({ left: 12, right: 12, top: 5, bottom: 5 })
            .onClick(() => {
              this.filterIndex = index;
            })
        }, (label: string) => label)
      }
      .width('100%')
      .padding({ left: 8, right: 8, bottom: 6 })
    }
    .width('100%')
  }

  @Builder
  selectionBar() {
    Row({ space: 8 }) {
      Text('已选 ' + this.selectedUris.length.toString() + ' 张')
        .fontSize(13)
        .fontColor('#E8E8E8')
        .layoutWeight(1)
      Button('全选')
        .fontSize(12)
        .height(32)
        .backgroundColor('#1E1E1E')
        .onClick(() => {
          this.selectAll();
        })
      Button('分享')
        .fontSize(12)
        .height(32)
        .backgroundColor('#1E1E1E')
        .enabled(this.selectedUris.length > 0)
        .onClick(() => {
          this.shareSelected();
        })
      Button('完成')
        .fontSize(12)
        .height(32)
        .backgroundColor('#1E1E1E')
        .onClick(() => {
          this.exitSelection();
        })
    }
    .width('100%')
    .padding({ left: 12, right: 12, top: 4, bottom: 4 })
    .backgroundColor('#1A1A1A')
  }

  @Builder
  photoGrid() {
    Grid() {
      ForEach(this.visiblePhotos(), (item: PhotoItem, index: number) => {
        GridItem() {
          Stack({ alignContent: Alignment.TopEnd }) {
            Image(item.uri)
              .objectFit(ImageFit.Cover)
              .width('100%')
              .height('100%')
              .backgroundColor('#1A1A1A')
            if (item.isRaw()) {
              Text('RAW')
                .fontSize(10)
                .fontColor('#FFFFFF')
                .backgroundColor('#AA000000')
                .borderRadius(4)
                .padding({ left: 4, right: 4, top: 1, bottom: 1 })
                .margin(4)
                .align(Alignment.BottomStart)
            }
            if (this.selectionMode) {
              Text(this.isSelected(item.uri) ? '✓' : '○')
                .fontSize(16)
                .fontColor(this.isSelected(item.uri) ? '#4A9EFF' : '#CCFFFFFF')
                .margin(4)
            }
          }
          .width('100%')
          .height('100%')
        }
        .aspectRatio(1)
        .borderRadius(4)
        .clip(true)
        .onClick(() => {
          this.onPhotoClick(item, index);
        })
        .gesture(LongPressGesture().onAction(() => {
          this.enterSelection(item.uri);
        }))
      }, (item: PhotoItem) => item.uri)
    }
    .columnsTemplate('1fr 1fr 1fr')
    .columnsGap(2)
    .rowsGap(2)
    .width('100%')
    .layoutWeight(1)
    .padding(2)
  }

  @Builder
  photoList() {
    List({ space: 2 }) {
      ForEach(this.visiblePhotos(), (item: PhotoItem, index: number) => {
        ListItem() {
          Row({ space: 10 }) {
            Image(item.uri)
              .width(56)
              .height(56)
              .objectFit(ImageFit.Cover)
              .borderRadius(6)
              .backgroundColor('#1A1A1A')
            Column({ space: 3 }) {
              Text(item.name)
                .fontSize(14)
                .fontColor('#E8E8E8')
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
              Text(item.pixelSize() + ' · ' + item.displaySize() + (item.isRaw() ? ' · RAW' : ''))
                .fontSize(11)
                .fontColor('#999999')
            }
            .alignItems(HorizontalAlign.Start)
            .layoutWeight(1)
            if (this.selectionMode) {
              Text(this.isSelected(item.uri) ? '✓' : '○')
                .fontSize(16)
                .fontColor(this.isSelected(item.uri) ? '#4A9EFF' : '#777777')
            }
          }
          .width('100%')
          .padding({ left: 12, right: 12, top: 8, bottom: 8 })
        }
        .onClick(() => {
          this.onPhotoClick(item, index);
        })
        .gesture(LongPressGesture().onAction(() => {
          this.enterSelection(item.uri);
        }))
      }, (item: PhotoItem) => item.uri)
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  bottomActions() {
    Row() {
      Column({ space: 2 }) {
        Text('⬇')
          .fontSize(18)
          .fontColor('#E8E8E8')
        Text('导入')
          .fontSize(11)
          .fontColor('#E8E8E8')
      }
      .layoutWeight(1)
      .height(56)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.importPhotos();
      })

      Column({ space: 2 }) {
        Text('◉')
          .fontSize(18)
          .fontColor('#E8E8E8')
        Text('相机')
          .fontSize(11)
          .fontColor('#E8E8E8')
      }
      .layoutWeight(1)
      .height(56)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.takePhoto();
      })

      Column({ space: 2 }) {
        Text('↗')
          .fontSize(18)
          .fontColor(this.selectedUris.length > 0 ? '#E8E8E8' : '#666666')
        Text(this.selectedUris.length > 0
          ? '分享(' + this.selectedUris.length.toString() + ')'
          : '分享')
          .fontSize(11)
          .fontColor(this.selectedUris.length > 0 ? '#E8E8E8' : '#666666')
      }
      .layoutWeight(1)
      .height(56)
      .justifyContent(FlexAlign.Center)
      .onClick(() => {
        this.shareSelected();
      })
    }
    .width('100%')
    .backgroundColor('#141414')
  }

  @Builder
  previewOverlay() {
    Column() {
      Row() {
        Text((this.previewIndex + 1).toString() + ' / ' + this.visiblePhotos().length.toString())
          .fontSize(12)
          .fontColor('#CCCCCC')
        Blank()
        Text(this.currentPhoto() !== undefined ? this.currentPhoto()!.name : '')
          .fontSize(13)
          .fontColor('#E8E8E8')
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
          .textAlign(TextAlign.Center)
        Button('关闭')
          .fontSize(12)
          .height(32)
          .backgroundColor('#1E1E1E')
          .onClick(() => {
            this.previewIndex = -1;
          })
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 8, bottom: 8 })
      .backgroundColor('#99000000')

      Swiper() {
        ForEach(this.visiblePhotos(), (item: PhotoItem) => {
          Image(item.uri)
            .objectFit(ImageFit.Contain)
            .width('100%')
            .height('100%')
        }, (item: PhotoItem) => item.uri)
      }
      .index(this.previewIndex)
      .loop(false)
      .indicator(false)
      .layoutWeight(1)
      .onChange((index: number) => {
        this.previewIndex = index;
      })

      Row() {
        Column({ space: 2 }) {
          Text('✎')
            .fontSize(16)
            .fontColor('#E8E8E8')
          Text('编辑')
            .fontSize(12)
            .fontColor('#E8E8E8')
        }
        .layoutWeight(1)
        .height(56)
        .justifyContent(FlexAlign.Center)
        .onClick(() => {
          const photos = this.visiblePhotos();
          if (this.previewIndex >= 0 && this.previewIndex < photos.length) {
            const photo = photos[this.previewIndex];
            router.pushUrl({
              url: 'pages/EditorPage',
              params: { uri: photo.uri, name: photo.name }
            });
          }
          this.previewIndex = -1;
        })

        Column({ space: 2 }) {
          Text('↗')
            .fontSize(16)
            .fontColor('#E8E8E8')
          Text('分享')
            .fontSize(12)
            .fontColor('#E8E8E8')
        }
        .layoutWeight(1)
        .height(56)
        .justifyContent(FlexAlign.Center)
        .onClick(() => {
          const photos = this.visiblePhotos();
          if (this.previewIndex >= 0 && this.previewIndex < photos.length) {
            this.shareUris([photos[this.previewIndex].uri]);
          }
        })
      }
      .width('100%')
      .backgroundColor('#99000000')
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#000000')
    .zIndex(10)
  }

  @Builder
  editTab() {
    Scroll() {
      Column({ space: 12 }) {
        Row() {
          Text('最近编辑')
            .fontSize(14)
            .fontColor('#E8E8E8')
            .layoutWeight(1)
          Text('刷新')
            .fontSize(12)
            .fontColor('#4A9EFF')
            .onClick(() => {
              this.loadRecent();
            })
        }
        .width('100%')

        if (this.recent.length === 0) {
          Column({ space: 8 }) {
            Text('还没有编辑记录')
              .fontSize(13)
              .fontColor('#888888')
            Text('去「设备」选一张照片开始编辑')
              .fontSize(11)
              .fontColor('#666666')
            Button('去设备选照片')
              .fontSize(12)
              .backgroundColor('#1F3B5A')
              .fontColor('#9FD0FF')
              .onClick(() => {
                this.activeTab = 0;
              })
          }
          .width('100%')
          .padding(20)
          .backgroundColor('#151515')
          .borderRadius(10)
        } else {
          ForEach(this.recent, (item: RecentEntry) => {
            Row({ space: 10 }) {
              Image(item.uri)
                .width(56)
                .height(56)
                .objectFit(ImageFit.Cover)
                .borderRadius(6)
              Column({ space: 4 }) {
                Text(item.name)
                  .fontSize(13)
                  .fontColor('#DDDDDD')
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                Text(this.formatTime(item.time))
                  .fontSize(10)
                  .fontColor('#777777')
              }
              .alignItems(HorizontalAlign.Start)
              .layoutWeight(1)

              Text('继续编辑')
                .fontSize(12)
                .fontColor('#4A9EFF')
            }
            .width('100%')
            .padding(10)
            .backgroundColor('#151515')
            .borderRadius(10)
            .onClick(() => {
              this.openEditor(item.uri, item.name);
            })
          }, (item: RecentEntry) => item.uri + item.time.toString())
        }
      }
      .padding(16)
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
  }

  @Builder
  meTab() {
    Scroll() {
      Column({ space: 12 }) {
        Column({ space: 8 }) {
          Text('关于')
            .fontSize(12)
            .fontColor('#888888')
            .width('100%')
          Row() {
            Text('核心版本')
              .fontSize(12)
              .fontColor('#AAAAAA')
              .layoutWeight(1)
            Text(this.coreVersion)
              .fontSize(12)
              .fontColor('#DDDDDD')
          }
          .width('100%')
          Row() {
            Text('C ABI')
              .fontSize(12)
              .fontColor('#AAAAAA')
              .layoutWeight(1)
            Text(nativeCore.abiVersion().toString())
              .fontSize(12)
              .fontColor('#DDDDDD')
          }
          .width('100%')
          Text('调色核心与 Windows / iOS 同源 · 许可证 MIT')
            .fontSize(10)
            .fontColor('#666666')
            .width('100%')
        }
        .width('100%')
        .padding(14)
        .backgroundColor('#151515')
        .borderRadius(10)

        Column({ space: 10 }) {
          Text('导出')
            .fontSize(12)
            .fontColor('#888888')
            .width('100%')
          Row() {
            Text('格式')
              .fontSize(12)
              .fontColor('#AAAAAA')
              .layoutWeight(1)
            Row({ space: 6 }) {
              Text('JPEG')
                .fontSize(12)
                .fontColor(this.exportFormat === 'jpeg' ? '#FFFFFF' : '#888888')
                .backgroundColor(this.exportFormat === 'jpeg' ? '#3A3A3A' : '#242424')
                .borderRadius(6)
                .padding({ left: 10, right: 10, top: 4, bottom: 4 })
                .onClick(() => {
                  this.exportFormat = 'jpeg';
                  this.saveExportSettings();
                })
              Text('PNG')
                .fontSize(12)
                .fontColor(this.exportFormat === 'png' ? '#FFFFFF' : '#888888')
                .backgroundColor(this.exportFormat === 'png' ? '#3A3A3A' : '#242424')
                .borderRadius(6)
                .padding({ left: 10, right: 10, top: 4, bottom: 4 })
                .onClick(() => {
                  this.exportFormat = 'png';
                  this.saveExportSettings();
                })
            }
          }
          .width('100%')

          if (this.exportFormat === 'jpeg') {
            Row({ space: 8 }) {
              Text('质量')
                .fontSize(12)
                .fontColor('#AAAAAA')
                .width(52)
              Slider({ value: this.exportQuality, min: 50, max: 100, step: 1 })
                .layoutWeight(1)
                .blockColor('#E8E8E8')
                .trackColor('#3A3A3A')
                .selectedColor('#4A9EFF')
                .onChange((value: number) => {
                  this.exportQuality = value;
                  this.saveExportSettings();
                })
              Text(this.exportQuality.toFixed(0))
                .fontSize(12)
                .fontColor('#DDDDDD')
                .width(30)
            }
            .width('100%')
          }
        }
        .width('100%')
        .padding(14)
        .backgroundColor('#151515')
        .borderRadius(10)

        Column({ space: 10 }) {
          Text('缓存')
            .fontSize(12)
            .fontColor('#888888')
            .width('100%')
          Row() {
            Text('占用')
              .fontSize(12)
              .fontColor('#AAAAAA')
              .layoutWeight(1)
            Text(this.formatBytes(this.cacheBytes))
              .fontSize(12)
              .fontColor('#DDDDDD')
          }
          .width('100%')
          Row({ space: 10 }) {
            Button('刷新')
              .fontSize(12)
              .backgroundColor('#242424')
              .fontColor('#CCCCCC')
              .onClick(() => {
                this.refreshCacheSize();
              })
            Button('清除缓存')
              .fontSize(12)
              .backgroundColor('#3A2020')
              .fontColor('#FF9A9A')
              .onClick(() => {
                this.clearCache();
              })
          }
          .width('100%')
        }
        .width('100%')
        .padding(14)
        .backgroundColor('#151515')
        .borderRadius(10)
      }
      .padding(16)
    }
    .width('100%')
    .layoutWeight(1)
    .align(Alignment.Top)
  }

  build() {
    Stack() {
      Column() {
        this.topBar()
        if (this.activeTab === 0) {
          this.deviceTab()
        } else if (this.activeTab === 1) {
          this.editTab()
        } else {
          this.meTab()
        }
        this.tabBar()
      }
      .width('100%')
      .height('100%')
      .backgroundColor('#0E0E0E')

      if (this.previewIndex >= 0) {
        this.previewOverlay()
      }
    }
    .width('100%')
    .height('100%')
    .expandSafeArea([SafeAreaType.SYSTEM], [SafeAreaEdge.BOTTOM])
  }
}
