// 编辑页：顶部栏 + 预览（Rust 渲染）+ 直方图 + 底部分类面板。
//
// 渲染链路：解码（ImageKit）→ NativeBridge.render（Rust 核心）→ PixelMap → Image。
// 拖动滑块 / 均衡器节点时用 480px 的小图快渲，松手后按 1280px 重渲。

import { router } from '@kit.ArkUI';
import { image } from '@kit.ImageKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import NativeBridge from '../core/NativeBridge';

/** 解码后的像素缓冲。 */
interface PixelBuffer {
  width: number;
  height: number;
  data: ArrayBuffer;
}

/** 光照段（只用到曝光相关字段，其余发默认值）。 */
interface LightPayload {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
}

/** 颜色段。 */
interface ColorPayload {
  temperature: number;
  tint: number;
  vibrance: number;
  saturation: number;
  black_white: boolean;
}

/** 单个 HSL 色带。 */
interface HslBandPayload {
  hue: number;
  saturation: number;
  luminance: number;
}

/** HSL 8 段。 */
interface HslPayload {
  red: HslBandPayload;
  orange: HslBandPayload;
  yellow: HslBandPayload;
  green: HslBandPayload;
  aqua: HslBandPayload;
  blue: HslBandPayload;
  purple: HslBandPayload;
  magenta: HslBandPayload;
}

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

/** 整份渲染参数（Rust 侧 `#[serde(default)]`，缺字段按默认值）。 */
interface ParamsPayload {
  light: LightPayload;
  color: ColorPayload;
  hsl: HslPayload;
  equalizer: EqualizerPayload;
}

/** 路由参数。 */
interface EditorParams {
  uri: string;
  name: string;
}

const COLOR_TABS: string[] = ['颜色', 'HSL', '均衡器', '分级'];
const EQ_CHANNELS: string[] = ['色相', '饱和度', '亮度'];
const BAND_DEGREES: number[] = [0, 45, 90, 135, 180, 225, 270, 315];
const BAND_LABELS: string[] = ['红', '橙', '黄', '绿', '青', '蓝', '紫', '品红'];
const BAND_KEYS: string[] = ['red', 'orange', 'yellow', 'green', 'aqua', 'blue', 'purple', 'magenta'];
const HIST_COLUMNS: number = 96;
const HIST_BINS: number = 48;

@Entry
@Component
struct EditorPage {
  @State photoUri: string = '';
  @State photoName: string = '';
  @State categoryIndex: number = 0;
  @State colorTab: number = 2;
  @State eqChannel: number = 0;
  @State showOptions: boolean = false;
  @State statusText: string = '正在解码…';
  @State preview: image.PixelMap | null = null;
  @State histogramWaveform: boolean = false;

  // 颜色
  @State temperature: number = 0;
  @State tint: number = 0;
  @State vibrance: number = 0;
  @State saturation: number = 0;

  // HSL（8 段）
  @State hslBand: number = 0;
  @State hslHue: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State hslSat: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State hslLum: number[] = [0, 0, 0, 0, 0, 0, 0, 0];

  // 均衡器
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
  private histSettings: RenderingContextSettings = new RenderingContextSettings(true);
  private histContext: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.histSettings);

  private full: PixelBuffer | null = null;
  private fast: PixelBuffer | null = null;
  private renderTimer: number = -1;
  private activeNode: number = -1;
  private lastTapNode: number = -1;
  private lastTapTime: number = 0;

  // 直方图缓存
  private histR: number[] = this.zeros(256);
  private histG: number[] = this.zeros(256);
  private histB: number[] = this.zeros(256);
  private histCols: number[] = this.zeros(HIST_COLUMNS * HIST_BINS * 3);
  private histPeak: number = 1;

  aboutToAppear(): void {
    const params = router.getParams() as EditorParams;
    if (params !== undefined && params !== null) {
      this.photoUri = params.uri;
      this.photoName = params.name;
    }
    this.loadPhoto();
  }

  aboutToDisappear(): void {
    if (this.renderTimer >= 0) {
      clearTimeout(this.renderTimer);
      this.renderTimer = -1;
    }
  }

  private zeros(count: number): number[] {
    const result: number[] = [];
    for (let i = 0; i < count; i++) {
      result.push(0);
    }
    return result;
  }

  // MARK: - 解码 / 渲染

  private async decode(uri: string, maxEdge: number): Promise<PixelBuffer | null> {
    try {
      const file = fs.openSync(uri, fs.OpenMode.READ_ONLY);
      const source = image.createImageSource(file.fd);
      const info = await source.getImageInfo();
      let width = info.size.width;
      let height = info.size.height;
      const longest = Math.max(width, height);
      if (longest > maxEdge) {
        const scale = maxEdge / longest;
        width = Math.max(1, Math.round(width * scale));
        height = Math.max(1, Math.round(height * scale));
      }
      const options: image.DecodingOptions = {
        desiredSize: { width: width, height: height }
      };
      const pixelMap = await source.createPixelMap(options);
      const buffer = new ArrayBuffer(width * height * 4);
      await pixelMap.readPixelsToBuffer(buffer);
      pixelMap.release();
      source.release();
      fs.closeSync(file);
      const result: PixelBuffer = { width: width, height: height, data: buffer };
      return result;
    } catch (error) {
      return null;
    }
  }

  private async loadPhoto(): Promise<void> {
    this.full = await this.decode(this.photoUri, 1280);
    this.fast = await this.decode(this.photoUri, 480);
    if (this.full === null) {
      this.statusText = '解码失败';
      return;
    }
    this.statusText = this.full.width + ' × ' + this.full.height;
    await this.renderNow(false);
  }

  private async renderNow(useFast: boolean): Promise<void> {
    const buffer = useFast ? this.fast : this.full;
    if (buffer === null) {
      return;
    }
    try {
      const result = NativeBridge.render(buffer.data, buffer.width, buffer.height, this.paramsJson());
      const options: image.InitializationOptions = {
        size: { width: result.width, height: result.height },
        srcPixelFormat: image.PixelMapFormat.RGBA_8888,
        pixelFormat: image.PixelMapFormat.RGBA_8888,
        editable: false
      };
      const pixelMap = await image.createPixelMap(result.data, options);
      const previous = this.preview;
      this.preview = pixelMap;
      if (previous !== null) {
        previous.release();
      }
      this.updateHistogram(new Uint8Array(result.data), result.width, result.height);
      this.drawHistogram();
    } catch (error) {
      this.statusText = '渲染失败：' + NativeBridge.lastError();
    }
  }

  private scheduleRender(useFast: boolean): void {
    if (this.renderTimer >= 0) {
      clearTimeout(this.renderTimer);
    }
    this.renderTimer = setTimeout(() => {
      this.renderTimer = -1;
      this.renderNow(useFast);
    }, useFast ? 70 : 180);
  }

  // MARK: - 参数 JSON

  private hslBandPayload(index: number): HslBandPayload {
    const band: HslBandPayload = {
      hue: this.hslHue[index],
      saturation: this.hslSat[index],
      luminance: this.hslLum[index]
    };
    return band;
  }

  private paramsJson(): string {
    const payload: ParamsPayload = {
      light: { exposure: 0, contrast: 0, highlights: 0, shadows: 0, whites: 0, blacks: 0 },
      color: {
        temperature: this.temperature,
        tint: this.tint,
        vibrance: this.vibrance,
        saturation: this.saturation,
        black_white: false
      },
      hsl: {
        red: this.hslBandPayload(0),
        orange: this.hslBandPayload(1),
        yellow: this.hslBandPayload(2),
        green: this.hslBandPayload(3),
        aqua: this.hslBandPayload(4),
        blue: this.hslBandPayload(5),
        purple: this.hslBandPayload(6),
        magenta: this.hslBandPayload(7)
      },
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

  // MARK: - 直方图

  private updateHistogram(pixels: Uint8Array, width: number, height: number): void {
    for (let i = 0; i < 256; i++) {
      this.histR[i] = 0;
      this.histG[i] = 0;
      this.histB[i] = 0;
    }
    for (let i = 0; i < this.histCols.length; i++) {
      this.histCols[i] = 0;
    }
    const total = width * height;
    const stride = Math.max(1, Math.floor(total / 200000));
    let peak = 1;
    let index = 0;
    while (index < total) {
      const offset = index * 4;
      const r = pixels[offset];
      const g = pixels[offset + 1];
      const b = pixels[offset + 2];
      this.histR[r] += 1;
      this.histG[g] += 1;
      this.histB[b] += 1;
      if (this.histR[r] > peak) {
        peak = this.histR[r];
      }
      if (this.histG[g] > peak) {
        peak = this.histG[g];
      }
      if (this.histB[b] > peak) {
        peak = this.histB[b];
      }
      const column = Math.floor(((index % width) * HIST_COLUMNS) / width);
      const base = column * HIST_BINS * 3;
      this.histCols[base + r % HIST_BINS] += 1;
      this.histCols[base + HIST_BINS + g % HIST_BINS] += 1;
      this.histCols[base + HIST_BINS * 2 + b % HIST_BINS] += 1;
      index += stride;
    }
    this.histPeak = peak;
  }

  private drawHistogram(): void {
    const ctx = this.histContext;
    const width = ctx.width;
    const height = ctx.height;
    if (width <= 0 || height <= 0) {
      return;
    }
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = '#1E1E1E';
    ctx.fillRect(0, 0, width, height);

    if (this.histogramWaveform) {
      for (let column = 0; column < HIST_COLUMNS; column++) {
        const base = column * HIST_BINS * 3;
        for (let bin = 0; bin < HIST_BINS; bin++) {
          for (let channel = 0; channel < 3; channel++) {
            const value = this.histCols[base + channel * HIST_BINS + bin];
            if (value <= 0) {
              continue;
            }
            const alpha = Math.min(1, Math.sqrt(value / 12));
            if (alpha < 0.08) {
              continue;
            }
            const color = channel === 0 ? '255,90,90' : (channel === 1 ? '90,220,110' : '90,140,255');
            ctx.fillStyle = 'rgba(' + color + ',' + alpha.toFixed(2) + ')';
            const x = (width * column) / HIST_COLUMNS;
            const y = height - (height * (bin + 1)) / HIST_BINS;
            ctx.fillRect(x, y, width / HIST_COLUMNS + 0.5, height / HIST_BINS + 0.5);
          }
        }
      }
    } else {
      this.drawChannelBars(ctx, width, height, this.histR, 'rgba(240,90,90,0.55)');
      this.drawChannelBars(ctx, width, height, this.histG, 'rgba(92,208,110,0.55)');
      this.drawChannelBars(ctx, width, height, this.histB, 'rgba(90,140,240,0.55)');
    }

    // 网格 + 边框
    ctx.strokeStyle = 'rgba(255,255,255,0.10)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      const x = (width * i) / 4;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
    }
    ctx.strokeStyle = 'rgba(255,255,255,0.25)';
    ctx.strokeRect(0.5, 0.5, width - 1, height - 1);
  }

  private drawChannelBars(ctx: CanvasRenderingContext2D, width: number, height: number,
    bins: number[], color: string): void {
    ctx.fillStyle = color;
    const step = width / 256;
    for (let i = 0; i < 256; i++) {
      if (bins[i] <= 0) {
        continue;
      }
      const normalized = Math.sqrt(bins[i] / this.histPeak);
      const bar = normalized * (height - 2);
      if (bar < 0.6) {
        continue;
      }
      ctx.fillRect(width * (i / 256), height - bar, Math.max(step, 1), bar);
    }
  }

  // MARK: - 均衡器曲线

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

  private drawGraph(): void {
    const ctx = this.context;
    const width = ctx.width;
    const height = ctx.height;
    if (width <= 0 || height <= 0) {
      return;
    }
    const strips = 36;
    for (let i = 0; i < strips; i++) {
      const hue = Math.round((i * 360) / strips);
      ctx.fillStyle = 'hsl(' + hue + ', 62%, 52%)';
      ctx.fillRect((width * i) / strips, 0, width / strips + 1, height);
    }
    ctx.fillStyle = 'rgba(0, 0, 0, 0.18)';
    ctx.fillRect(0, 0, width, height);

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

  private handleGraphTouch(x: number, y: number, height: number, phase: number): void {
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
      const now = Date.now();
      if (this.lastTapNode === best && now - this.lastTapTime < 400) {
        this.setNodeValue(best, this.eqChannel === 0 ? 0 : 1);
        this.lastTapNode = -1;
        this.activeNode = -1;
        this.drawGraph();
        this.scheduleRender(false);
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
      this.scheduleRender(true);
      return;
    }
    if (phase === TouchType.Up || phase === TouchType.Cancel) {
      if (this.activeNode >= 0) {
        this.scheduleRender(false);
      }
      this.activeNode = -1;
      this.drawGraph();
    }
  }

  // MARK: - UI

  build() {
    Column() {
      this.topBar()
      Stack() {
        if (this.preview !== null) {
          Image(this.preview)
            .objectFit(ImageFit.Contain)
            .width('100%')
            .height('100%')
        } else {
          Image(this.photoUri)
            .objectFit(ImageFit.Contain)
            .width('100%')
            .height('100%')
        }
      }
      .width('100%')
      .layoutWeight(1)
      .backgroundColor('#000000')

      Canvas(this.histContext)
        .width('100%')
        .height(88)
        .onReady(() => {
          this.drawHistogram();
        })
        .onClick(() => {
          this.histogramWaveform = !this.histogramWaveform;
          this.drawHistogram();
        })

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
      Text(this.statusText)
        .fontSize(11)
        .fontColor('#888888')
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
    .height(390)
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

      if (this.colorTab === 0) {
        this.basicColorPanel()
      } else if (this.colorTab === 1) {
        this.hslPanel()
      } else if (this.colorTab === 2) {
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
  basicColorPanel() {
    Column({ space: 2 }) {
      this.sliderRow('色温', this.temperature, -100, 100, 1, 0)
      this.sliderRow('色调', this.tint, -150, 150, 1, 1)
      this.sliderRow('自然饱和度', this.vibrance, -100, 100, 1, 2)
      this.sliderRow('饱和度', this.saturation, -100, 100, 1, 3)
    }
    .width('100%')
    .padding({ left: 14, right: 14, top: 4 })
  }

  @Builder
  hslPanel() {
    Column() {
      Row() {
        ForEach(BAND_LABELS, (label: string, index: number) => {
          Text(label)
            .fontSize(11)
            .fontColor(this.hslBand === index ? '#FFFFFF' : '#888888')
            .backgroundColor(this.hslBand === index ? '#3A3A3A' : '#242424')
            .borderRadius(5)
            .padding({ left: 7, right: 7, top: 4, bottom: 4 })
            .margin({ right: 4 })
            .onClick(() => {
              this.hslBand = index;
            })
        })
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 2, bottom: 6 })

      this.sliderRow('色相', this.hslHue[this.hslBand], -100, 100, 1, 4)
      this.sliderRow('饱和度', this.hslSat[this.hslBand], -100, 100, 1, 5)
      this.sliderRow('明亮度', this.hslLum[this.hslBand], -100, 100, 1, 6)
    }
    .width('100%')
    .padding({ left: 14, right: 14 })
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
      .height(32)

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
          this.handleGraphTouch(touch.x, touch.y, this.context.height, event.type);
        })

      if (this.showOptions) {
        Column({ space: 4 }) {
          this.sliderRow('阈值', this.threshold, 0, 0.3, 0.01, 7)
          this.sliderRow('曲线宽度', this.smoothing, 0.05, 2, 0.05, 8)
          this.sliderRow('对比', this.eqContrast, -1, 1, 0.05, 9)
        }
        .padding({ left: 14, right: 14, top: 2, bottom: 6 })
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  /** 滑块改动统一入口（@Builder 不能带函数参数，用 kind 分发）。 */
  private applySlider(kind: number, value: number): void {
    if (kind === 0) {
      this.temperature = value;
    } else if (kind === 1) {
      this.tint = value;
    } else if (kind === 2) {
      this.vibrance = value;
    } else if (kind === 3) {
      this.saturation = value;
    } else if (kind === 4) {
      const next = this.hslHue.slice();
      next[this.hslBand] = value;
      this.hslHue = next;
    } else if (kind === 5) {
      const next = this.hslSat.slice();
      next[this.hslBand] = value;
      this.hslSat = next;
    } else if (kind === 6) {
      const next = this.hslLum.slice();
      next[this.hslBand] = value;
      this.hslLum = next;
    } else if (kind === 7) {
      this.threshold = value;
      this.drawGraph();
    } else if (kind === 8) {
      this.smoothing = value;
      this.drawGraph();
    } else {
      this.eqContrast = value;
      this.drawGraph();
    }
    this.scheduleRender(true);
  }

  @Builder
  sliderRow(label: string, value: number, min: number, max: number, step: number, kind: number) {
    Row({ space: 8 }) {
      Text(label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(64)
      Slider({ value: value, min: min, max: max, step: step })
        .layoutWeight(1)
        .blockColor('#E8E8E8')
        .trackColor('#3A3A3A')
        .selectedColor('#4A9EFF')
        .onChange((changed: number) => {
          this.applySlider(kind, changed);
        })
      Text(value.toFixed(step < 0.1 ? 2 : 0))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(42)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }
}
