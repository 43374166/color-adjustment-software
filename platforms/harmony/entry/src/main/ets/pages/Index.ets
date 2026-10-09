// fdxmp HarmonyOS 主界面。
//
// 严格按规格实现三大 Tab：设备（默认）/ 编辑 / 我的，底部导航，无侧边栏。
// 设备 Tab 已接入相册（权限、网格 / 列表、搜索 / 排序 / 筛选、多选、全屏预览、
// 系统相册导入与相机入口）；编辑与我的待后续阶段。
import abilityAccessCtrl, { Permissions } from '@ohos.abilityAccessCtrl';
import common from '@ohos.app.ability.common';
import dataSharePredicates from '@ohos.data.dataSharePredicates';
import photoAccessHelper from '@ohos.file.photoAccessHelper';
import picker from '@ohos.file.picker';
import promptAction from '@ohos.promptAction';
import camera from '@ohos.multimedia.camera';
import cameraPicker from '@ohos.multimedia.cameraPicker';
import { PhotoItem } from '../model/PhotoItem';
import nativeCore from '../core/NativeBridge';

@Entry
@Component
struct Index {
  @State activeTab: number = 0;
  @State coreVersion: string = '未加载';
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
      this.previewIndex = index;
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
    promptAction.showToast({
      message: '已选 ' + count.toString() + ' 张，系统分享阶段 6 接入',
    });
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
        Column() {
          Text(label)
            .fontSize(14)
            .fontColor(this.activeTab === index ? '#4A9EFF' : '#777777')
        }
        .layoutWeight(1)
        .height(48)
        .justifyContent(FlexAlign.Center)
        .onClick(() => {
          this.activeTab = index;
        })
      }, (label: string) => label)
    }
    .width('100%')
    .backgroundColor('#141414')
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
        this.bottomActions()
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
      Text('导入')
        .fontSize(13)
        .fontColor('#E8E8E8')
        .textAlign(TextAlign.Center)
        .layoutWeight(1)
        .height(52)
        .onClick(() => {
          this.importPhotos();
        })
      Text('相机')
        .fontSize(13)
        .fontColor('#E8E8E8')
        .textAlign(TextAlign.Center)
        .layoutWeight(1)
        .height(52)
        .onClick(() => {
          this.takePhoto();
        })
      Text(this.selectedUris.length > 0
        ? '分享(' + this.selectedUris.length.toString() + ')'
        : '分享')
        .fontSize(13)
        .fontColor(this.selectedUris.length > 0 ? '#E8E8E8' : '#666666')
        .textAlign(TextAlign.Center)
        .layoutWeight(1)
        .height(52)
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
        Text('编辑')
          .fontSize(14)
          .fontColor('#E8E8E8')
          .textAlign(TextAlign.Center)
          .layoutWeight(1)
          .height(52)
          .onClick(() => {
            this.previewIndex = -1;
            this.activeTab = 1;
            promptAction.showToast({ message: '编辑界面阶段 6 接入' });
          })
        Text('分享')
          .fontSize(14)
          .fontColor('#E8E8E8')
          .textAlign(TextAlign.Center)
          .layoutWeight(1)
          .height(52)
          .onClick(() => {
            promptAction.showToast({ message: '系统分享阶段 6 接入' });
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
    Column({ space: 8 }) {
      Text('编辑')
        .fontSize(18)
        .fontColor('#E8E8E8')
      Text('已编辑记录 + 缩略图 + 预设名')
        .fontSize(14)
        .fontColor('#CCCCCC')
      Text('空状态时引导去设备 Tab 选图；编辑界面阶段 6 接入')
        .fontSize(12)
        .fontColor('#888888')
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  meTab() {
    Column({ space: 8 }) {
      Text('我的')
        .fontSize(18)
        .fontColor('#E8E8E8')
      Text('账号 / 缓存管理 / 预设管理 / 主题 / 关于')
        .fontSize(14)
        .fontColor('#CCCCCC')
      Text('清除缓存为核心功能（对应 Rust 侧 CacheManager）')
        .fontSize(12)
        .fontColor('#888888')
    }
    .width('100%')
    .layoutWeight(1)
    .justifyContent(FlexAlign.Center)
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
  }
}
