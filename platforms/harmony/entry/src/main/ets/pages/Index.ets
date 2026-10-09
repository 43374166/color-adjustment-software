// fdxmp HarmonyOS 主界面（阶段 5 骨架）
//
// 严格按规格实现三大 Tab：设备（默认）/ 编辑 / 我的，底部导航，无侧边栏。
import nativeCore from '../core/NativeBridge';

@Entry
@Component
struct Index {
  @State activeTab: number = 0;
  @State coreVersion: string = '未加载';
  private tabs: string[] = ['设备', '编辑', '我的'];

  aboutToAppear(): void {
    this.coreVersion = nativeCore.version();
  }

  @Builder
  deviceTab() {
    Column({ space: 8 }) {
      Text('设备：相册网格 / 列表、RAW 与常见格式、全屏预览')
        .fontSize(14)
      Text('阶段 5 接入媒体库权限与缩略图网格')
        .fontSize(12)
        .fontColor('#888888')
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  editTab() {
    Column({ space: 8 }) {
      Text('编辑：已编辑记录 + 缩略图 + 预设名')
        .fontSize(14)
      Text('空状态时引导去设备 Tab 选图')
        .fontSize(12)
        .fontColor('#888888')
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
  }

  @Builder
  meTab() {
    Column({ space: 8 }) {
      Text('我的：账号 / 缓存管理 / 预设管理 / 主题 / 关于')
        .fontSize(14)
      Text('清除缓存为核心功能（对应 Rust 侧 CacheManager）')
        .fontSize(12)
        .fontColor('#888888')
    }
    .width('100%')
    .justifyContent(FlexAlign.Center)
  }

  build() {
    Column() {
      // 顶部标题栏随 Tab 变化
      Row() {
        Text(`fdxmp · ${this.tabs[this.activeTab]}`)
          .fontSize(18)
          .fontWeight(FontWeight.Medium)
        Blank()
        Text(`core ${this.coreVersion}`)
          .fontSize(11)
          .fontColor('#888888')
      }
      .width('100%')
      .padding({ left: 16, right: 16, top: 12, bottom: 12 })

      // 内容区
      Column() {
        if (this.activeTab === 0) {
          this.deviceTab()
        } else if (this.activeTab === 1) {
          this.editTab()
        } else {
          this.meTab()
        }
      }
      .layoutWeight(1)
      .width('100%')

      // 底部三大 Tab
      Row() {
        ForEach(this.tabs, (label: string, index: number) => {
          Column() {
            Text(label)
              .fontSize(14)
              .fontColor(this.activeTab === index ? '#4A9EFF' : '#666666')
          }
          .layoutWeight(1)
          .height(48)
          .justifyContent(FlexAlign.Center)
          .onClick(() => {
            this.activeTab = index;
          })
        })
      }
      .width('100%')
      .borderWidth({ top: 1 })
      .borderColor('#22000000')
    }
    .width('100%')
    .height('100%')
  }
}

