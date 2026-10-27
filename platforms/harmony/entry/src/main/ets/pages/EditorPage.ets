// 编辑页：顶部栏 + 预览 + 底部分类面板。
//
// 这一版先接通「颜色 → 均衡器」：曲线由 Rust 核心烘焙（NativeBridge.equalizerCurve），
// 8 个节点可拖、双击复位，其余分类 / 子页先占位。

import { router } from '@kit.ArkUI';
import NativeBridge from '../core/NativeBridge';

/** 均衡器参数（与 Rust `EqualizerParams` 的 serde 字段一一对应）。 */
interface EqualizerPayload {
  enabled: boolean;
  threshold: number;
  smoothing_hue: number;
  contrast: number;
  white_level: number;
  chroma_size: number;
  param_size: number;
  use_filter: boolean;
  hue_shift: number;
  saturation: number[];
  hue: number[];
  brightness: number[];
}

/** 整份渲染参数（Rust 侧只读 `equalizer`）。 */
interface ParamsPayload {
  equalizer: EqualizerPayload;
}

/** 路由参数。 */
interface EditorParams {
  uri: string;
  name: string;
}

/** 颜色分类的子页。 */
const COLOR_TABS: string[] = ['颜色', 'HSL', '均衡器', '分级'];
/** 均衡器三通道。 */
const EQ_CHANNELS: string[] = ['色相', '饱和度', '亮度'];
/** 8 个节点等距 45°（与 Rust 的 RBF 节点一致）。 */
const BAND_DEGREES: number[] = [0, 45, 90, 135, 180, 225, 270, 315];

@Entry
@Component
struct EditorPage {
  @State photoUri: string = '';
  @State photoName: string = '';
  @State categoryIndex: number = 0;
  @State colorTab: number = 2;
  @State eqChannel: number = 0;
  @State showOptions: boolean = false;

  // 均衡器参数（与 Rust EqualizerParams 对齐）。
  @State eqHue: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State eqSat: number[] = [1, 1, 1, 1, 1, 1, 1, 1];
  @State eqBri: number[] = [1, 1, 1, 1, 1, 1, 1, 1];
  @State hueShift: number = 0;
  @State smoothing: number = 1;
  @State threshold: number = 0.1;
  @State eqContrast: number = 0;
  @State useFilter: boolean = false;

  private settings: RenderingContextSettings = new RenderingContextSettings(true);
  private context: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.settings);
  private activeNode: number = -1;
  private dragStartY: number = 0;
  private lastTapNode: number = -1;
  private lastTapTime: number = 0;

  aboutToAppear(): void {
    const params = router.getParams() as EditorParams;
    if (params !== undefined && params !== null) {
      this.photoUri = params.uri;
      this.photoName = params.name;
    }
  }

  // MARK: - 参数 JSON / 曲线

  /** 只含 equalizer 的参数 JSON（Rust 侧只读这一段来烘焙曲线）。 */
  private paramsJson(): string {
    const payload: ParamsPayload = {
      equalizer: {
        enabled: true,
        threshold: this.threshold,
        smoothing_hue: this.smoothing,
        contrast: this.eqContrast,
        white_level: 1.0,
        chroma_size: 1.5,
        param_size: 1.0,
        use_filter: this.useFilter,
        hue_shift: this.hueShift,
        saturation: this.eqSat,
        hue: this.eqHue,
        brightness: this.eqBri
      }
    };
    return JSON.stringify(payload);
  }

  private curve(): Float32Array {
    return NativeBridge.equalizerCurve(this.eqChannel, this.paramsJson());
  }

  private nodeValue(index: number): number {
    if (this.eqChannel === 0) {
      return this.eqHue[index];
    }
    if (this.eqChannel === 1) {
      return this.eqSat[index];
    }
    return this.eqBri[index];
  }

  private setNodeValue(index: number, value: number): void {
    if (this.eqChannel === 0) {
      const next = this.eqHue.slice();
      next[index] = value;
      this.eqHue = next;
    } else if (this.eqChannel === 1) {
      const next = this.eqSat.slice();
      next[index] = Math.max(value, 0);
      this.eqSat = next;
    } else {
      const next = this.eqBri.slice();
      next[index] = Math.max(value, 0);
      this.eqBri = next;
    }
  }

  private resetNode(index: number): void {
    this.setNodeValue(index, this.eqChannel === 0 ? 0 : 1);
    this.drawGraph();
  }

  /** 数值 → 归一化 0..1（0 = 底、1 = 顶）。 */
  private normalize(value: number): number {
    const t = this.eqChannel === 0 ? (value + 180) / 360 : value / 2;
    return Math.min(Math.max(t, 0), 1);
  }

  private valueAt(y: number, height: number): number {
    const t = 1 - Math.min(Math.max(y / height, 0), 1);
    return this.eqChannel === 0 ? t * 360 - 180 : t * 2;
  }

  private nodeX(index: number, width: number): number {
    let deg = (BAND_DEGREES[index] + this.hueShift) % 360;
    if (deg < 0) {
      deg += 360;
    }
    return (deg / 360) * width;
  }

  // MARK: - 绘制

  private drawGraph(): void {
    const ctx = this.context;
    const width = ctx.width;
    const height = ctx.height;
    if (width <= 0 || height <= 0) {
      return;
    }

    // 背景：36 段色相条 + 纵向明暗。
    const strips = 36;
    for (let i = 0; i < strips; i++) {
      const hue = Math.round((i * 360) / strips);
      ctx.fillStyle = 'hsl(' + hue + ', 62%, 52%)';
      ctx.fillRect((width * i) / strips, 0, width / strips + 1, height);
    }
    ctx.fillStyle = 'rgba(0, 0, 0, 0.18)';
    ctx.fillRect(0, 0, width, height);

    // 网格
    ctx.strokeStyle = 'rgba(0, 0, 0, 0.22)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 8; i++) {
      const x = (width * i) / 8;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
    }
    for (let i = 1; i < 4; i++) {
      const y = (height * i) / 4;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }
    ctx.beginPath();
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.35)';
    ctx.moveTo(0, height / 2);
    ctx.lineTo(width, height / 2);
    ctx.stroke();

    // 曲线：512 点 LUT，按常规色相 0..360 采样（与 Windows 端同一套映射）。
    const lut = this.curve();
    if (lut.length === 512) {
      ctx.beginPath();
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.92)';
      ctx.lineWidth = 1.6;
      for (let step = 0; step <= 360; step++) {
        const index = Math.round(((step + 200) / 360) * 512) % 512;
        let raw = lut[index];
        if (this.eqChannel === 0) {
          raw = (raw * 180) / Math.PI;
        }
        const x = (width * step) / 360;
        const y = height * (1 - this.normalize(raw));
        if (step === 0) {
          ctx.moveTo(x, y);
        } else {
          ctx.lineTo(x, y);
        }
      }
      ctx.stroke();
    }

    // 节点
    for (let index = 0; index < 8; index++) {
      const x = this.nodeX(index, width);
      const y = height * (1 - this.normalize(this.nodeValue(index)));
      const deg = (BAND_DEGREES[index] + this.hueShift + 360) % 360;
      ctx.beginPath();
      ctx.arc(x, y, 7, 0, Math.PI * 2);
      ctx.fillStyle = '#1c1c1c';
      ctx.fill();
      ctx.lineWidth = 2.5;
      ctx.strokeStyle = 'hsl(' + deg + ', 85%, 62%)';
      ctx.stroke();
      if (index === this.activeNode) {
        ctx.beginPath();
        ctx.arc(x, y, 11, 0, Math.PI * 2);
        ctx.lineWidth = 1.4;
        ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)';
        ctx.stroke();
      }
      // 周期闭合：贴边的节点在对侧再画一半。
      if (x < 10 || x > width - 10) {
        const ghost = x < 10 ? x + width : x - width;
        ctx.beginPath();
        ctx.arc(ghost, y, 7, 0, Math.PI * 2);
        ctx.fillStyle = '#1c1c1c';
        ctx.fill();
        ctx.lineWidth = 2.5;
        ctx.strokeStyle = 'hsl(' + deg + ', 85%, 62%)';
        ctx.stroke();
      }
    }
  }

  // MARK: - 交互

  private handleTouch(x: number, y: number, height: number, phase: number): void {
    if (phase === TouchType.Down) {
      let best = -1;
      let bestDistance = 999;
      const width = this.context.width;
      for (let index = 0; index < 8; index++) {
        const raw = Math.abs(x - this.nodeX(index, width));
        const distance = Math.min(raw, width - raw);
        if (distance < bestDistance) {
          bestDistance = distance;
          best = index;
        }
      }
      if (bestDistance > 30) {
        return;
      }
      this.activeNode = best;
      this.dragStartY = y;
      const now = Date.now();
      if (this.lastTapNode === best && now - this.lastTapTime < 400) {
        this.resetNode(best);
        this.lastTapNode = -1;
        this.activeNode = -1;
        return;
      }
      this.lastTapNode = best;
      this.lastTapTime = now;
      this.drawGraph();
      return;
    }
    if (phase === TouchType.Move && this.activeNode >= 0) {
      this.setNodeValue(this.activeNode, this.valueAt(y, height));
      this.drawGraph();
      return;
    }
    if (phase === TouchType.Up || phase === TouchType.Cancel) {
      this.activeNode = -1;
      this.drawGraph();
    }
  }

  // MARK: - UI

  build() {
    Column() {
      this.topBar()
      Image(this.photoUri)
        .objectFit(ImageFit.Contain)
        .width('100%')
        .layoutWeight(1)
        .backgroundColor('#000000')
      this.bottomPanel()
    }
    .width('100%')
    .height('100%')
    .backgroundColor('#111111')
  }

  @Builder
  topBar() {
    Row() {
      Text('‹ 返回')
        .fontSize(15)
        .fontColor('#E8E8E8')
        .onClick(() => {
          router.back();
        })
      Text(this.photoName)
        .fontSize(13)
        .fontColor('#BBBBBB')
        .layoutWeight(1)
        .textAlign(TextAlign.Center)
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
      Text('完成')
        .fontSize(15)
        .fontColor('#4A9EFF')
        .onClick(() => {
          router.back();
        })
    }
    .width('100%')
    .height(48)
    .padding({ left: 14, right: 14 })
    .backgroundColor('#1A1A1A')
  }

  @Builder
  bottomPanel() {
    Column() {
      Row() {
        ForEach(['颜色', '曲线', '光学', '细节'], (label: string, index: number) => {
          Text(label)
            .fontSize(13)
            .fontColor(this.categoryIndex === index ? '#4A9EFF' : '#999999')
            .layoutWeight(1)
            .textAlign(TextAlign.Center)
            .onClick(() => {
              this.categoryIndex = index;
            })
        })
      }
      .width('100%')
      .height(34)

      if (this.categoryIndex === 0) {
        this.colorPanel()
      } else {
        Text('该分类下一阶段接入')
          .fontSize(12)
          .fontColor('#777777')
          .margin({ top: 24, bottom: 24 })
      }
    }
    .width('100%')
    .height(380)
    .backgroundColor('#1A1A1A')
  }

  @Builder
  colorPanel() {
    Column() {
      Row() {
        ForEach(COLOR_TABS, (label: string, index: number) => {
          Text(label)
            .fontSize(12)
            .fontColor(this.colorTab === index ? '#FFFFFF' : '#888888')
            .backgroundColor(this.colorTab === index ? '#3A3A3A' : '#242424')
            .borderRadius(6)
            .padding({ left: 12, right: 12, top: 5, bottom: 5 })
            .margin({ right: 6 })
            .onClick(() => {
              this.colorTab = index;
            })
        })
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 8, bottom: 6 })

      if (this.colorTab === 2) {
        this.equalizerPanel()
      } else {
        Text('该子页下一阶段接入')
          .fontSize(12)
          .fontColor('#777777')
          .margin({ top: 30 })
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  equalizerPanel() {
    Column() {
      Row() {
        ForEach(EQ_CHANNELS, (label: string, index: number) => {
          Text(label)
            .fontSize(13)
            .fontColor(this.eqChannel === index ? '#4A9EFF' : '#999999')
            .layoutWeight(1)
            .textAlign(TextAlign.Center)
            .onClick(() => {
              this.eqChannel = index;
              this.drawGraph();
            })
        })
        Text(this.showOptions ? '收起' : '选项')
          .fontSize(12)
          .fontColor('#4A9EFF')
          .padding({ left: 10, right: 12 })
          .onClick(() => {
            this.showOptions = !this.showOptions;
          })
      }
      .width('100%')
      .height(34)

      Canvas(this.context)
        .width('100%')
        .layoutWeight(1)
        .margin({ left: 10, right: 10 })
        .onReady(() => {
          this.drawGraph();
        })
        .onTouch((event: TouchEvent) => {
          if (event.touches.length === 0) {
            return;
          }
          const touch = event.touches[0];
          this.handleTouch(touch.x, touch.y, this.context.height, event.type);
        })

      if (this.showOptions) {
        this.optionSliders()
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  optionSliders() {
    Column({ space: 6 }) {
      this.optionRow('阈值', this.threshold, 0, 0.3, 0.01, (value: number) => {
        this.threshold = value;
        this.drawGraph();
      })
      this.optionRow('曲线宽度', this.smoothing, 0.05, 2, 0.05, (value: number) => {
        this.smoothing = value;
        this.drawGraph();
      })
      this.optionRow('对比', this.eqContrast, -1, 1, 0.05, (value: number) => {
        this.eqContrast = value;
        this.drawGraph();
      })
    }
    .width('100%')
    .padding({ left: 14, right: 14, top: 4, bottom: 8 })
  }

  @Builder
  optionRow(label: string, value: number, min: number, max: number, step: number,
    onChange: (value: number) => void) {
    Row({ space: 8 }) {
      Text(label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(56)
      Slider({ value: value, min: min, max: max, step: step })
        .layoutWeight(1)
        .onChange((changed: number) => {
          onChange(changed);
        })
      Text(value.toFixed(2))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(40)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }
}
