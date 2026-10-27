// 编辑页：顶部栏 + 预览（Rust 渲染）+ 直方图 + 底部分类面板。
//
// 渲染链路：解码（ImageKit）→ NativeBridge.render（Rust 核心）→ PixelMap → Image。
// 拖动滑块 / 均衡器节点时用 480px 小图快渲，松手后按 1280px 重渲。

import { router } from '@kit.ArkUI';
import { image } from '@kit.ImageKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import { preferences } from '@kit.ArkData';
import common from '@ohos.app.ability.common';
import promptAction from '@ohos.promptAction';
import abilityAccessCtrl from '@ohos.abilityAccessCtrl';
import photoAccessHelper from '@ohos.file.photoAccessHelper';
import NativeBridge from '../core/NativeBridge';

/** 解码后的像素缓冲。 */
interface PixelBuffer {
  width: number;
  height: number;
  data: ArrayBuffer;
}

/** 光照段。 */
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

/** 均衡器参数。 */
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

/** 颜色分级的一个区域。 */
interface GradingZonePayload {
  hue: number;
  saturation: number;
  luminance: number;
}

/** 颜色分级。 */
interface GradingPayload {
  shadows: GradingZonePayload;
  midtones: GradingZonePayload;
  highlights: GradingZonePayload;
  global: GradingZonePayload;
  blending: number;
  balance: number;
}

/** 相机校准。 */
interface CalibrationPayload {
  red_hue: number;
  red_sat: number;
  green_hue: number;
  green_sat: number;
  blue_hue: number;
  blue_sat: number;
  shadows_tint: number;
}

/** 黑白混色。 */
interface BwMixPayload {
  red: number;
  orange: number;
  yellow: number;
  green: number;
  aqua: number;
  blue: number;
  purple: number;
  magenta: number;
}

/** 几何。 */
interface GeometryPayload {
  auto: boolean;
  distortion: number;
  vertical: number;
  horizontal: number;
  rotate: number;
  scale: number;
}

/** 镜头。 */
interface LensPayload {
  profile_correction: boolean;
  chromatic_aberration: number;
  vignette: number;
  geometry: GeometryPayload;
}

/** 效果。 */
interface EffectsPayload {
  texture: number;
  clarity: number;
  dehaze: number;
  vignette: number;
  vignette_midpoint: number;
  vignette_feather: number;
  grain: number;
  grain_size: number;
  grain_roughness: number;
}

/** 锐化。 */
interface SharpenPayload {
  amount: number;
  radius: number;
  detail: number;
  masking: number;
}

/** 降噪。 */
interface NoisePayload {
  luminance: number;
  luminance_detail: number;
  luminance_contrast: number;
  color: number;
  color_detail: number;
  color_smoothness: number;
}

/** 细节。 */
interface DetailPayload {
  sharpen: SharpenPayload;
  noise: NoisePayload;
}

/** 曲线控制点。 */
interface CurvePointPayload {
  x: number;
  y: number;
}

/** 单条曲线。 */
interface ToneCurvePayload {
  points: CurvePointPayload[];
}

/** 曲线参数。 */
interface CurvesPayload {
  rgb: ToneCurvePayload;
  red: ToneCurvePayload;
  green: ToneCurvePayload;
  blue: ToneCurvePayload;
}

/** 裁剪。 */
interface CropPayload {
  rect: number[];
}

/** 点颜色样本。 */
interface PointColorPayload {
  lum: number;
  chroma: number;
  hue: number;
  hue_shift: number;
  sat_shift: number;
  lum_shift: number;
  variance: number;
  range: number;
  hue_range: number;
  sat_range: number;
  lum_range: number;
}

/** 整份渲染参数。 */
interface ParamsPayload {
  light: LightPayload;
  color: ColorPayload;
  hsl: HslPayload;
  equalizer: EqualizerPayload;
  grading: GradingPayload;
  calibration: CalibrationPayload;
  bw_mix: BwMixPayload;
  lens: LensPayload;
  effects: EffectsPayload;
  detail: DetailPayload;
  curves: CurvesPayload;
  crop: CropPayload;
  point_colors: PointColorPayload[];
}

/** 曲线控制点（可变）。 */
class CurvePt {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

/** 一个点颜色样本（含取色时的 sRGB，画色块用）。 */
class PointSample {
  r: number = 0;
  g: number = 0;
  b: number = 0;
  lum: number = 0;
  chroma: number = 0;
  hue: number = 0;
  hueShift: number = 0;
  satShift: number = 0;
  lumShift: number = 0;
  hueRange: number = 50;
  satRange: number = 50;
  lumRange: number = 50;
}

/** 路由参数。 */
interface EditorParams {
  uri: string;
  name: string;
}

/** 一个标量滑块的规格（下标就是 nums 里的位置）。 */
class SliderSpec {
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;

  constructor(label: string, min: number, max: number, step: number, value: number) {
    this.label = label;
    this.min = min;
    this.max = max;
    this.step = step;
    this.value = value;
  }
}

// 标量滑块顺序（paramsJson 里按下面的常量取值）。
const I_TEMPERATURE: number = 0;
const I_TINT: number = 1;
const I_VIBRANCE: number = 2;
const I_SATURATION: number = 3;
const I_EQ_THRESHOLD: number = 4;
const I_EQ_SMOOTHING: number = 5;
const I_EQ_CONTRAST: number = 6;
const I_CAL_RED_HUE: number = 7;
const I_CAL_RED_SAT: number = 8;
const I_CAL_GREEN_HUE: number = 9;
const I_CAL_GREEN_SAT: number = 10;
const I_CAL_BLUE_HUE: number = 11;
const I_CAL_BLUE_SAT: number = 12;
const I_CAL_SHADOW_TINT: number = 13;
const I_CA: number = 14;
const I_LENS_VIGNETTE: number = 15;
const I_DISTORTION: number = 16;
const I_VERTICAL: number = 17;
const I_HORIZONTAL: number = 18;
const I_ROTATE: number = 19;
const I_SCALE: number = 20;
const I_TEXTURE: number = 21;
const I_CLARITY: number = 22;
const I_DEHAZE: number = 23;
const I_VIGNETTE: number = 24;
const I_VIGNETTE_MID: number = 25;
const I_VIGNETTE_FEATHER: number = 26;
const I_GRAIN: number = 27;
const I_GRAIN_SIZE: number = 28;
const I_GRAIN_ROUGH: number = 29;
const I_SHARPEN_AMOUNT: number = 30;
const I_SHARPEN_RADIUS: number = 31;
const I_SHARPEN_DETAIL: number = 32;
const I_SHARPEN_MASK: number = 33;
const I_NOISE_LUM: number = 34;
const I_NOISE_LUM_DETAIL: number = 35;
const I_NOISE_LUM_CONTRAST: number = 36;
const I_NOISE_COLOR: number = 37;
const I_NOISE_COLOR_DETAIL: number = 38;
const I_NOISE_COLOR_SMOOTH: number = 39;

const SLIDERS: SliderSpec[] = [
  new SliderSpec('色温', -100, 100, 1, 0),
  new SliderSpec('色调', -150, 150, 1, 0),
  new SliderSpec('自然饱和度', -100, 100, 1, 0),
  new SliderSpec('饱和度', -100, 100, 1, 0),
  new SliderSpec('阈值', 0, 0.3, 0.01, 0.1),
  new SliderSpec('曲线宽度', 0.05, 2, 0.05, 1),
  new SliderSpec('对比', -1, 1, 0.05, 0),
  new SliderSpec('红原色色相', -100, 100, 1, 0),
  new SliderSpec('红原色饱和', -100, 100, 1, 0),
  new SliderSpec('绿原色色相', -100, 100, 1, 0),
  new SliderSpec('绿原色饱和', -100, 100, 1, 0),
  new SliderSpec('蓝原色色相', -100, 100, 1, 0),
  new SliderSpec('蓝原色饱和', -100, 100, 1, 0),
  new SliderSpec('阴影色调', -100, 100, 1, 0),
  new SliderSpec('色差', -100, 100, 1, 0),
  new SliderSpec('晕影校正', -100, 100, 1, 0),
  new SliderSpec('畸变', -100, 100, 1, 0),
  new SliderSpec('垂直', -100, 100, 1, 0),
  new SliderSpec('水平', -100, 100, 1, 0),
  new SliderSpec('旋转', -45, 45, 0.5, 0),
  new SliderSpec('缩放', 50, 200, 1, 100),
  new SliderSpec('纹理', -100, 100, 1, 0),
  new SliderSpec('清晰度', -100, 100, 1, 0),
  new SliderSpec('去雾', -100, 100, 1, 0),
  new SliderSpec('暗角', -100, 100, 1, 0),
  new SliderSpec('暗角中点', 0, 100, 1, 50),
  new SliderSpec('暗角羽化', 0, 100, 1, 50),
  new SliderSpec('颗粒', 0, 100, 1, 0),
  new SliderSpec('颗粒大小', 0, 100, 1, 25),
  new SliderSpec('颗粒粗糙度', 0, 100, 1, 50),
  new SliderSpec('锐化数量', 0, 150, 1, 0),
  new SliderSpec('锐化半径', 0.5, 3, 0.1, 1),
  new SliderSpec('锐化细节', 0, 100, 1, 25),
  new SliderSpec('锐化蒙版', 0, 100, 1, 0),
  new SliderSpec('降噪亮度', 0, 100, 1, 0),
  new SliderSpec('亮度细节', 0, 100, 1, 50),
  new SliderSpec('亮度对比', 0, 100, 1, 0),
  new SliderSpec('颜色降噪', 0, 100, 1, 25),
  new SliderSpec('颜色细节', 0, 100, 1, 50),
  new SliderSpec('颜色平滑度', 0, 100, 1, 50),
];

const COLOR_TABS: string[] = ['颜色', 'HSL', '均衡器', '分级', '点颜色', '校准', '黑白'];
const CATEGORIES: string[] = ['颜色', '曲线', '光学', '细节', '效果', '几何'];
const EQ_CHANNELS: string[] = ['色相', '饱和度', '亮度'];
const BAND_DEGREES: number[] = [0, 45, 90, 135, 180, 225, 270, 315];
const BAND_LABELS: string[] = ['红', '橙', '黄', '绿', '青', '蓝', '紫', '品红'];
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

  /** 所有标量滑块的值（下标见 I_* 常量）。 */
  @State nums: number[] = [];
  @State lensProfile: boolean = false;
  @State curveChannel: number = 0;
  @State showPresets: boolean = false;
  @State presetNames: string[] = [];
  @State cropMode: boolean = false;
  /** 归一化裁剪矩形 [x, y, w, h]。 */
  @State cropRect: number[] = [0, 0, 1, 1];

  // 点颜色
  @State points: PointSample[] = [];
  @State selectedPoint: number = 0;
  @State picking: boolean = false;
  @State areaW: number = 0;
  @State areaH: number = 0;

  // HSL（8 段）
  @State hslBand: number = 0;
  @State hslHue: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State hslSat: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State hslLum: number[] = [0, 0, 0, 0, 0, 0, 0, 0];

  // 分级（阴影 / 中间调 / 高光 / 全局）
  @State gradingZone: number = 0;
  @State gradingHue: number[] = [0, 0, 0, 0];
  @State gradingSat: number[] = [0, 0, 0, 0];
  @State gradingLum: number[] = [0, 0, 0, 0];
  @State gradingBlending: number = 50;
  @State gradingBalance: number = 0;

  // 黑白
  @State blackWhite: boolean = false;
  @State bwMix: number[] = [0, 0, 0, 0, 0, 0, 0, 0];

  // 均衡器（8 节点）
  @State eqHue: number[] = [0, 0, 0, 0, 0, 0, 0, 0];
  @State eqSat: number[] = [1, 1, 1, 1, 1, 1, 1, 1];
  @State eqBri: number[] = [1, 1, 1, 1, 1, 1, 1, 1];
  @State hueShift: number = 0;
  @State useFilter: boolean = false;

  private settings: RenderingContextSettings = new RenderingContextSettings(true);
  private context: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.settings);
  private histSettings: RenderingContextSettings = new RenderingContextSettings(true);
  private histContext: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.histSettings);
  private curveSettings: RenderingContextSettings = new RenderingContextSettings(true);
  private curveContext: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.curveSettings);
  private cropSettings: RenderingContextSettings = new RenderingContextSettings(true);
  private cropContext: CanvasRenderingContext2D = new CanvasRenderingContext2D(this.cropSettings);

  /** 上一帧渲染结果的尺寸（算裁剪框显示位置用）。 */
  private lastW: number = 0;
  private lastH: number = 0;
  private cropSaved: number[] = [0, 0, 1, 1];
  private cropHandle: number = -1;
  private cropLastX: number = 0;
  private cropLastY: number = 0;

  /** 4 条曲线：0 RGB / 1 红 / 2 绿 / 3 蓝。 */
  private curvePoints: CurvePt[][] = [];
  private activeCurvePoint: number = -1;
  private lastTapPoint: number = -1;
  private lastTapPointTime: number = 0;

  private full: PixelBuffer | null = null;
  private fast: PixelBuffer | null = null;
  private renderTimer: number = -1;
  private activeNode: number = -1;
  private lastTapNode: number = -1;
  private lastTapTime: number = 0;

  private histR: number[] = [];
  private histG: number[] = [];
  private histB: number[] = [];
  private histCols: number[] = [];
  private histPeak: number = 1;
  private presetStore: preferences.Preferences | null = null;

  aboutToAppear(): void {
    const params = router.getParams() as EditorParams;
    if (params !== undefined && params !== null) {
      this.photoUri = params.uri;
      this.photoName = params.name;
    }
    this.nums = this.defaultValues();
    this.histR = this.zeros(256);
    this.histG = this.zeros(256);
    this.histB = this.zeros(256);
    this.histCols = this.zeros(HIST_COLUMNS * HIST_BINS * 3);
    this.curvePoints = [
      [new CurvePt(0, 0), new CurvePt(1, 1)],
      [new CurvePt(0, 0), new CurvePt(1, 1)],
      [new CurvePt(0, 0), new CurvePt(1, 1)],
      [new CurvePt(0, 0), new CurvePt(1, 1)]
    ];
    this.initPresets();
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

  private defaultValues(): number[] {
    const result: number[] = [];
    for (let i = 0; i < SLIDERS.length; i++) {
      result.push(SLIDERS[i].value);
    }
    return result;
  }

  private num(index: number): number {
    if (index < 0 || index >= this.nums.length) {
      return 0;
    }
    return this.nums[index];
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
      this.lastW = result.width;
      this.lastH = result.height;
      if (this.cropMode) {
        this.drawCropOverlay();
      }
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

  private gradingZonePayload(index: number): GradingZonePayload {
    const zone: GradingZonePayload = {
      hue: this.gradingHue[index],
      saturation: this.gradingSat[index],
      luminance: this.gradingLum[index]
    };
    return zone;
  }

  private pointsJson(channel: number): CurvePointPayload[] {
    const result: CurvePointPayload[] = [];
    const points = this.curvePoints[channel];
    for (let i = 0; i < points.length; i++) {
      const point: CurvePointPayload = { x: points[i].x, y: points[i].y };
      result.push(point);
    }
    return result;
  }

  private curvesPayload(): CurvesPayload {
    const payload: CurvesPayload = {
      rgb: { points: this.pointsJson(0) },
      red: { points: this.pointsJson(1) },
      green: { points: this.pointsJson(2) },
      blue: { points: this.pointsJson(3) }
    };
    return payload;
  }

  private paramsJson(): string {
    const payload: ParamsPayload = {
      light: { exposure: 0, contrast: 0, highlights: 0, shadows: 0, whites: 0, blacks: 0 },
      color: {
        temperature: this.num(I_TEMPERATURE),
        tint: this.num(I_TINT),
        vibrance: this.num(I_VIBRANCE),
        saturation: this.num(I_SATURATION),
        black_white: this.blackWhite
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
      grading: {
        shadows: this.gradingZonePayload(0),
        midtones: this.gradingZonePayload(1),
        highlights: this.gradingZonePayload(2),
        global: this.gradingZonePayload(3),
        blending: this.gradingBlending,
        balance: this.gradingBalance
      },
      calibration: {
        red_hue: this.num(I_CAL_RED_HUE),
        red_sat: this.num(I_CAL_RED_SAT),
        green_hue: this.num(I_CAL_GREEN_HUE),
        green_sat: this.num(I_CAL_GREEN_SAT),
        blue_hue: this.num(I_CAL_BLUE_HUE),
        blue_sat: this.num(I_CAL_BLUE_SAT),
        shadows_tint: this.num(I_CAL_SHADOW_TINT)
      },
      bw_mix: {
        red: this.bwMix[0],
        orange: this.bwMix[1],
        yellow: this.bwMix[2],
        green: this.bwMix[3],
        aqua: this.bwMix[4],
        blue: this.bwMix[5],
        purple: this.bwMix[6],
        magenta: this.bwMix[7]
      },
      lens: {
        profile_correction: this.lensProfile,
        chromatic_aberration: this.num(I_CA),
        vignette: this.num(I_LENS_VIGNETTE),
        geometry: {
          auto: false,
          distortion: this.num(I_DISTORTION),
          vertical: this.num(I_VERTICAL),
          horizontal: this.num(I_HORIZONTAL),
          rotate: this.num(I_ROTATE),
          scale: this.num(I_SCALE)
        }
      },
      effects: {
        texture: this.num(I_TEXTURE),
        clarity: this.num(I_CLARITY),
        dehaze: this.num(I_DEHAZE),
        vignette: this.num(I_VIGNETTE),
        vignette_midpoint: this.num(I_VIGNETTE_MID),
        vignette_feather: this.num(I_VIGNETTE_FEATHER),
        grain: this.num(I_GRAIN),
        grain_size: this.num(I_GRAIN_SIZE),
        grain_roughness: this.num(I_GRAIN_ROUGH)
      },
      detail: {
        sharpen: {
          amount: this.num(I_SHARPEN_AMOUNT),
          radius: this.num(I_SHARPEN_RADIUS),
          detail: this.num(I_SHARPEN_DETAIL),
          masking: this.num(I_SHARPEN_MASK)
        },
        noise: {
          luminance: this.num(I_NOISE_LUM),
          luminance_detail: this.num(I_NOISE_LUM_DETAIL),
          luminance_contrast: this.num(I_NOISE_LUM_CONTRAST),
          color: this.num(I_NOISE_COLOR),
          color_detail: this.num(I_NOISE_COLOR_DETAIL),
          color_smoothness: this.num(I_NOISE_COLOR_SMOOTH)
        }
      },
      curves: this.curvesPayload(),
      crop: { rect: this.cropMode ? [0, 0, 1, 1] : this.cropRect },
      point_colors: this.pointColorsPayload(),
      equalizer: {
        enabled: true,
        threshold: this.num(I_EQ_THRESHOLD),
        smoothing_hue: this.num(I_EQ_SMOOTHING),
        contrast: this.num(I_EQ_CONTRAST),
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
        if (this.cropMode) {
          Canvas(this.cropContext)
            .width('100%')
            .height('100%')
            .onReady(() => {
              this.drawCropOverlay();
            })
            .onTouch((event: TouchEvent) => {
              if (event.touches.length === 0) {
                return;
              }
              const touch = event.touches[0];
              this.handleCropTouch(touch.x, touch.y, event.type);
            })
        }
        if (this.showPresets) {
          this.presetOverlay()
        }
      }
      .onAreaChange((oldArea: Area, newArea: Area) => {
        this.areaW = Number(newArea.width);
        this.areaH = Number(newArea.height);
        if (this.cropMode) {
          this.drawCropOverlay();
        }
      })
      .onClick((event: ClickEvent) => {
        if (this.picking) {
          this.sampleAt(event.x, event.y);
        }
      })
      .width('100%')
      .layoutWeight(1)
      .backgroundColor('#000000')

      Canvas(this.histContext)
        .width('100%')
        .height(84)
        .onReady(() => {
          this.drawHistogram();
        })
        .onClick(() => {
          this.histogramWaveform = !this.histogramWaveform;
          this.drawHistogram();
        })

      if (this.cropMode) {
        this.cropBar()
      } else {
        this.bottomPanel()
      }
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
      Text('预设')
        .fontSize(13)
        .fontColor('#4A9EFF')
        .padding({ right: 10 })
        .onClick(() => {
          this.showPresets = true;
        })
      Text('裁剪')
        .fontSize(13)
        .fontColor('#4A9EFF')
        .padding({ right: 10 })
        .onClick(() => {
          if (!this.cropMode) {
            this.enterCrop();
          }
        })
      Text('导出')
        .fontSize(13)
        .fontColor('#4A9EFF')
        .padding({ right: 10 })
        .onClick(() => {
          this.exportPhoto();
        })
      Text(this.statusText)
        .fontSize(11)
        .fontColor('#888888')
    }
    .width('100%')
    .height(48)
    .padding({ left: 14, right: 14 })
    .backgroundColor('#1A1A1A')
  }

  @Builder
  bottomPanel() {
    Column() {
      Scroll() {
        Row() {
          ForEach(CATEGORIES, (label: string, index: number) => {
            Text(label)
              .fontSize(13)
              .fontColor(this.categoryIndex === index ? '#4A9EFF' : '#999999')
              .padding({ left: 14, right: 14, top: 8, bottom: 8 })
              .onClick(() => {
                this.categoryIndex = index;
              })
          })
        }
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')
      .height(34)

      if (this.categoryIndex === 0) {
        this.colorPanel()
      } else if (this.categoryIndex === 1) {
        this.curvePanel()
      } else if (this.categoryIndex === 2) {
        this.opticsPanel()
      } else if (this.categoryIndex === 3) {
        this.detailPanel()
      } else if (this.categoryIndex === 4) {
        this.effectsPanel()
      } else {
        this.geometryPanel()
      }
    }
    .width('100%')
    .height(400)
    .backgroundColor('#1A1A1A')
  }

  @Builder
  cropBar() {
    Column() {
      Row() {
        ForEach(['自由', '1:1', '4:3', '16:9', '3:2'], (label: string, index: number) => {
          Text(label)
            .fontSize(12)
            .fontColor('#DDDDDD')
            .backgroundColor('#2A2A2A')
            .borderRadius(6)
            .padding({ left: 10, right: 10, top: 5, bottom: 5 })
            .margin({ right: 6 })
            .onClick(() => {
              const ratios: number[] = [0, 1, 4 / 3, 16 / 9, 3 / 2];
              this.setAspect(ratios[index]);
            })
        })
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 10, bottom: 8 })

      Row() {
        Text('取消')
          .fontSize(14)
          .fontColor('#CCCCCC')
          .layoutWeight(1)
          .textAlign(TextAlign.Center)
          .height(44)
          .onClick(() => {
            this.cancelCrop();
          })
        Text('应用')
          .fontSize(14)
          .fontColor('#4A9EFF')
          .layoutWeight(1)
          .textAlign(TextAlign.Center)
          .height(44)
          .onClick(() => {
            this.applyCrop();
          })
      }
      .width('100%')

      Text('拖动四角改裁剪范围，拖框内移动')
        .fontSize(11)
        .fontColor('#777777')
        .margin({ top: 6 })
    }
    .width('100%')
    .height(160)
    .backgroundColor('#1A1A1A')
  }

  private pointValue(kind: number): number {
    if (this.points.length === 0) {
      return 0;
    }
    const index = Math.min(this.selectedPoint, this.points.length - 1);
    const point = this.points[index];
    if (kind === 200) {
      return point.hueShift;
    }
    if (kind === 201) {
      return point.satShift;
    }
    if (kind === 202) {
      return point.lumShift;
    }
    if (kind === 203) {
      return point.hueRange;
    }
    if (kind === 204) {
      return point.satRange;
    }
    return point.lumRange;
  }

  @Builder
  pointColorPanel() {
    Column({ space: 4 }) {
      Row() {
        Text(this.picking ? '点击预览取色…' : '＋ 点预览取色')
          .fontSize(12)
          .fontColor('#4A9EFF')
          .onClick(() => {
            this.picking = !this.picking;
          })
        Text('最多 4 个')
          .fontSize(10)
          .fontColor('#777777')
          .padding({ left: 8 })
      }
      .width('100%')

      if (this.points.length === 0) {
        Text('取色后可以用色相 / 饱和度 / 明亮度 + 范围做局部调整')
          .fontSize(11)
          .fontColor('#777777')
          .margin({ top: 6 })
      } else {
        Row({ space: 8 }) {
          ForEach(this.points, (point: PointSample, index: number) => {
            Stack() {
              Circle({ width: 30, height: 30 })
                .fill('rgb(' + point.r + ',' + point.g + ',' + point.b + ')')
              if (this.selectedPoint === index) {
                Circle({ width: 38, height: 38 })
                  .fill(Color.Transparent)
                  .stroke('#4A9EFF')
                  .strokeWidth(2)
              }
            }
            .width(44)
            .height(44)
            .onClick(() => {
              this.selectedPoint = index;
            })
          }, (point: PointSample, index: number) => index.toString())

          Text('删除')
            .fontSize(11)
            .fontColor('#F06060')
            .padding({ left: 6 })
            .onClick(() => {
              this.removePoint(Math.min(this.selectedPoint, this.points.length - 1));
            })
        }
        .width('100%')

        this.pointSliderRow('色相', 200, -100, 100)
        this.pointSliderRow('饱和度', 201, -100, 100)
        this.pointSliderRow('明亮度', 202, -100, 100)
        this.pointSliderRow('范围·色相', 203, 0, 100)
        this.pointSliderRow('范围·饱和度', 204, 0, 100)
        this.pointSliderRow('范围·明亮度', 205, 0, 100)
      }
    }
    .width('100%')
    .padding({ left: 14, right: 14, top: 4 })
  }

  @Builder
  pointSliderRow(label: string, kind: number, min: number, max: number) {
    Row({ space: 8 }) {
      Text(label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(78)
      Slider({ value: this.pointValue(kind), min: min, max: max, step: 1 })
        .layoutWeight(1)
        .blockColor('#E8E8E8')
        .trackColor('#3A3A3A')
        .selectedColor('#4A9EFF')
        .onChange((changed: number) => {
          this.applyPoint(kind, changed);
        })
      Text(this.formatValue(this.pointValue(kind), 1))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(38)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }

  private applyPoint(kind: number, value: number): void {
    const index = Math.min(this.selectedPoint, this.points.length - 1);
    this.updatePoint(index, (point: PointSample) => {
      if (kind === 200) {
        point.hueShift = value;
      } else if (kind === 201) {
        point.satShift = value;
      } else if (kind === 202) {
        point.lumShift = value;
      } else if (kind === 203) {
        point.hueRange = value;
      } else if (kind === 204) {
        point.satRange = value;
      } else {
        point.lumRange = value;
      }
    });
  }

  @Builder
  presetOverlay() {
    Column() {
      Row() {
        Text('预设')
          .fontSize(14)
          .fontColor('#FFFFFF')
          .layoutWeight(1)
        Text('保存当前')
          .fontSize(12)
          .fontColor('#4A9EFF')
          .padding({ right: 14 })
          .onClick(() => {
            this.savePreset();
          })
        Text('关闭')
          .fontSize(12)
          .fontColor('#888888')
          .onClick(() => {
            this.showPresets = false;
          })
      }
      .width('100%')
      .height(42)
      .padding({ left: 14, right: 14 })

      if (this.presetNames.length === 0) {
        Text('还没有预设：调好参数后点「保存当前」')
          .fontSize(11)
          .fontColor('#888888')
          .margin({ top: 24 })
      } else {
        Scroll() {
          Column() {
            ForEach(this.presetNames, (name: string) => {
              Row() {
                Text(name)
                  .fontSize(13)
                  .fontColor('#DDDDDD')
                  .layoutWeight(1)
                Text('删除')
                  .fontSize(11)
                  .fontColor('#F06060')
                  .padding({ left: 12 })
                  .onClick(() => {
                    this.deletePreset(name);
                  })
              }
              .width('100%')
              .height(44)
              .padding({ left: 14, right: 14 })
              .onClick(() => {
                this.applyPreset(name);
              })
            }, (name: string) => name)
          }
        }
        .layoutWeight(1)
        .scrollBar(BarState.Off)
      }
    }
    .width('72%')
    .height('58%')
    .backgroundColor('#E61E1E1E')
    .borderRadius(10)
    .clip(true)
  }

  @Builder
  curvePanel() {
    Column() {
      Row() {
        ForEach(['RGB', '红', '绿', '蓝'], (label: string, index: number) => {
          Text(label)
            .fontSize(12)
            .fontColor(this.curveChannel === index ? '#FFFFFF' : '#888888')
            .backgroundColor(this.curveChannel === index ? '#3A3A3A' : '#242424')
            .borderRadius(6)
            .padding({ left: 10, right: 10, top: 4, bottom: 4 })
            .margin({ right: 6 })
            .onClick(() => {
              this.curveChannel = index;
              this.drawCurveEditor();
            })
        })
        Text('重置')
          .fontSize(12)
          .fontColor('#4A9EFF')
          .layoutWeight(1)
          .textAlign(TextAlign.End)
          .padding({ right: 4 })
          .onClick(() => {
            this.resetCurve();
          })
      }
      .width('100%')
      .padding({ left: 12, right: 12, top: 2, bottom: 6 })

      Canvas(this.curveContext)
        .width('100%')
        .layoutWeight(1)
        .margin({ left: 10, right: 10, bottom: 8 })
        .onReady(() => {
          this.drawCurveEditor();
        })
        .onTouch((event: TouchEvent) => {
          if (event.touches.length === 0) {
            return;
          }
          const touch = event.touches[0];
          this.handleCurveTouch(touch.x, touch.y, this.curveContext.width,
            this.curveContext.height, event.type);
        })
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  curvePlaceholder() {
    Column() {
      Text('曲线编辑下一阶段接入')
        .fontSize(12)
        .fontColor('#777777')
        .margin({ top: 30 })
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  colorPanel() {
    Column() {
      Scroll() {
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
        .padding({ left: 12, right: 12 })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')
      .height(34)

      if (this.colorTab === 0) {
        this.basicColorPanel()
      } else if (this.colorTab === 1) {
        this.hslPanel()
      } else if (this.colorTab === 2) {
        this.equalizerPanel()
      } else if (this.colorTab === 3) {
        this.gradingPanel()
      } else if (this.colorTab === 4) {
        this.pointColorPanel()
      } else if (this.colorTab === 5) {
        this.calibrationPanel()
      } else {
        this.bwPanel()
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  @Builder
  basicColorPanel() {
    Column({ space: 2 }) {
      this.sliderRow(I_TEMPERATURE)
      this.sliderRow(I_TINT)
      this.sliderRow(I_VIBRANCE)
      this.sliderRow(I_SATURATION)
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

      this.bandSliderRow('色相', this.hslHue[this.hslBand], 100)
      this.bandSliderRow('饱和度', this.hslSat[this.hslBand], 101)
      this.bandSliderRow('明亮度', this.hslLum[this.hslBand], 102)
    }
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  gradingPanel() {
    Column({ space: 2 }) {
      Row() {
        ForEach(['阴影', '中间调', '高光', '全局'], (label: string, index: number) => {
          Text(label)
            .fontSize(11)
            .fontColor(this.gradingZone === index ? '#FFFFFF' : '#888888')
            .backgroundColor(this.gradingZone === index ? '#3A3A3A' : '#242424')
            .borderRadius(5)
            .padding({ left: 8, right: 8, top: 4, bottom: 4 })
            .margin({ right: 5 })
            .onClick(() => {
              this.gradingZone = index;
            })
        })
      }
      .width('100%')
      .padding({ bottom: 4 })

      this.bandSliderRow('色相', this.gradingHue[this.gradingZone], 103)
      this.bandSliderRow('饱和度', this.gradingSat[this.gradingZone], 104)
      this.bandSliderRow('明亮度', this.gradingLum[this.gradingZone], 105)
      this.plainSliderRow('混合', this.gradingBlending, 0, 100, 1, 106)
      this.plainSliderRow('平衡', this.gradingBalance, -100, 100, 1, 107)
    }
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  calibrationPanel() {
    Scroll() {
      Column({ space: 2 }) {
        this.sliderRow(I_CAL_RED_HUE)
        this.sliderRow(I_CAL_RED_SAT)
        this.sliderRow(I_CAL_GREEN_HUE)
        this.sliderRow(I_CAL_GREEN_SAT)
        this.sliderRow(I_CAL_BLUE_HUE)
        this.sliderRow(I_CAL_BLUE_SAT)
        this.sliderRow(I_CAL_SHADOW_TINT)
      }
      .width('100%')
    }
    .scrollBar(BarState.Off)
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  bwPanel() {
    Column({ space: 2 }) {
      Row() {
        Text('黑白模式')
          .fontSize(12)
          .fontColor('#CCCCCC')
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.blackWhite })
          .onChange((isOn: boolean) => {
            this.blackWhite = isOn;
            this.scheduleRender(false);
          })
      }
      .width('100%')
      .padding({ bottom: 4 })

      Scroll() {
        Column({ space: 2 }) {
          ForEach([0, 1, 2, 3, 4, 5, 6, 7], (index: number) => {
            this.bwSliderRow(BAND_LABELS[index], this.bwMix[index], index)
          })
        }
        .width('100%')
      }
      .scrollBar(BarState.Off)
      .width('100%')
      .layoutWeight(1)
      .opacity(this.blackWhite ? 1 : 0.4)
    }
    .width('100%')
    .layoutWeight(1)
    .padding({ left: 14, right: 14 })
  }

  @Builder
  opticsPanel() {
    Column({ space: 2 }) {
      Row() {
        Text('配置文件校正')
          .fontSize(12)
          .fontColor('#CCCCCC')
          .layoutWeight(1)
        Toggle({ type: ToggleType.Switch, isOn: this.lensProfile })
          .onChange((isOn: boolean) => {
            this.lensProfile = isOn;
            this.scheduleRender(false);
          })
      }
      .width('100%')
      .padding({ bottom: 4 })

      this.sliderRow(I_CA)
      this.sliderRow(I_LENS_VIGNETTE)
      Text('几何校正在「几何」分类里')
        .fontSize(11)
        .fontColor('#666666')
        .margin({ top: 8 })
    }
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  geometryPanel() {
    Scroll() {
      Column({ space: 2 }) {
        this.sliderRow(I_DISTORTION)
        this.sliderRow(I_VERTICAL)
        this.sliderRow(I_HORIZONTAL)
        this.sliderRow(I_ROTATE)
        this.sliderRow(I_SCALE)
      }
      .width('100%')
    }
    .scrollBar(BarState.Off)
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  detailPanel() {
    Scroll() {
      Column({ space: 2 }) {
        Text('锐化')
          .fontSize(11)
          .fontColor('#888888')
          .width('100%')
          .padding({ top: 2, bottom: 2 })
        this.sliderRow(I_SHARPEN_AMOUNT)
        this.sliderRow(I_SHARPEN_RADIUS)
        this.sliderRow(I_SHARPEN_DETAIL)
        this.sliderRow(I_SHARPEN_MASK)
        Text('降噪')
          .fontSize(11)
          .fontColor('#888888')
          .width('100%')
          .padding({ top: 8, bottom: 2 })
        this.sliderRow(I_NOISE_LUM)
        this.sliderRow(I_NOISE_LUM_DETAIL)
        this.sliderRow(I_NOISE_LUM_CONTRAST)
        this.sliderRow(I_NOISE_COLOR)
        this.sliderRow(I_NOISE_COLOR_DETAIL)
        this.sliderRow(I_NOISE_COLOR_SMOOTH)
      }
      .width('100%')
    }
    .scrollBar(BarState.Off)
    .width('100%')
    .padding({ left: 14, right: 14 })
  }

  @Builder
  effectsPanel() {
    Scroll() {
      Column({ space: 2 }) {
        this.sliderRow(I_TEXTURE)
        this.sliderRow(I_CLARITY)
        this.sliderRow(I_DEHAZE)
        Text('暗角')
          .fontSize(11)
          .fontColor('#888888')
          .width('100%')
          .padding({ top: 8, bottom: 2 })
        this.sliderRow(I_VIGNETTE)
        this.sliderRow(I_VIGNETTE_MID)
        this.sliderRow(I_VIGNETTE_FEATHER)
        Text('颗粒')
          .fontSize(11)
          .fontColor('#888888')
          .width('100%')
          .padding({ top: 8, bottom: 2 })
        this.sliderRow(I_GRAIN)
        this.sliderRow(I_GRAIN_SIZE)
        this.sliderRow(I_GRAIN_ROUGH)
      }
      .width('100%')
    }
    .scrollBar(BarState.Off)
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
        Column({ space: 2 }) {
          this.sliderRow(I_EQ_THRESHOLD)
          this.sliderRow(I_EQ_SMOOTHING)
          this.sliderRow(I_EQ_CONTRAST)
        }
        .padding({ left: 14, right: 14, top: 2, bottom: 6 })
      }
    }
    .width('100%')
    .layoutWeight(1)
  }

  // MARK: - 曲线

  /** 与 Rust `eval_curve` 同一套单调三次 Hermite。 */
  private curveTangent(points: CurvePt[], index: number): number {
    const count = points.length;
    if (count < 2) {
      return 0;
    }
    const secant = (i: number): number => {
      const span = Math.max(points[i + 1].x - points[i].x, 1e-6);
      return (points[i + 1].y - points[i].y) / span;
    };
    if (index === 0) {
      return secant(0);
    }
    if (index + 1 >= count) {
      return secant(count - 2);
    }
    const previous = secant(index - 1);
    const next = secant(index);
    if (previous * next <= 0) {
      return 0;
    }
    const slope = (previous + next) * 0.5;
    const limit = 3 * Math.min(Math.abs(previous), Math.abs(next));
    return Math.min(Math.max(slope, -limit), limit);
  }

  private evalCurve(points: CurvePt[], x: number): number {
    const count = points.length;
    if (count === 0) {
      return x;
    }
    if (count === 1) {
      return points[0].y;
    }
    if (x <= points[0].x) {
      return points[0].y;
    }
    const last = points[count - 1];
    if (x >= last.x) {
      return last.y;
    }
    let index = 0;
    while (index + 2 < count && x > points[index + 1].x) {
      index++;
    }
    const left = points[index];
    const right = points[index + 1];
    const span = Math.max(right.x - left.x, 1e-6);
    const t = Math.min(Math.max((x - left.x) / span, 0), 1);
    const m0 = this.curveTangent(points, index);
    const m1 = this.curveTangent(points, index + 1);
    const t2 = t * t;
    const t3 = t2 * t;
    const h00 = 2 * t3 - 3 * t2 + 1;
    const h10 = t3 - 2 * t2 + t;
    const h01 = -2 * t3 + 3 * t2;
    const h11 = t3 - t2;
    return h00 * left.y + h10 * span * m0 + h01 * right.y + h11 * span * m1;
  }

  private drawCurveEditor(): void {
    const ctx = this.curveContext;
    const width = ctx.width;
    const height = ctx.height;
    if (width <= 0 || height <= 0) {
      return;
    }
    ctx.fillStyle = '#141414';
    ctx.fillRect(0, 0, width, height);

    // 网格 + 对角线
    ctx.strokeStyle = 'rgba(255,255,255,0.10)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      const x = (width * i) / 4;
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
      ctx.stroke();
      const y = (height * i) / 4;
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
      ctx.stroke();
    }
    ctx.strokeStyle = 'rgba(255,255,255,0.16)';
    ctx.beginPath();
    ctx.moveTo(0, height);
    ctx.lineTo(width, 0);
    ctx.stroke();

    // 曲线
    const points = this.curvePoints[this.curveChannel];
    const color = this.curveChannel === 1 ? '#F06060' : (this.curveChannel === 2 ? '#60D070' : (this.curveChannel === 3 ? '#6090F0' : '#E8E8E8'));
    ctx.beginPath();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.8;
    for (let i = 0; i <= 128; i++) {
      const x = i / 128;
      const value = Math.min(Math.max(this.evalCurve(points, x), 0), 1);
      const px = x * width;
      const py = (1 - value) * height;
      if (i === 0) {
        ctx.moveTo(px, py);
      } else {
        ctx.lineTo(px, py);
      }
    }
    ctx.stroke();

    // 控制点
    for (let i = 0; i < points.length; i++) {
      const px = points[i].x * width;
      const py = (1 - points[i].y) * height;
      ctx.beginPath();
      ctx.arc(px, py, 5, 0, Math.PI * 2);
      ctx.fillStyle = i === this.activeCurvePoint ? '#FFFFFF' : '#141414';
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = color;
      ctx.stroke();
    }
  }

  private handleCurveTouch(x: number, y: number, width: number, height: number, phase: number): void {
    if (width <= 0 || height <= 0) {
      return;
    }
    const px = Math.min(Math.max(x / width, 0), 1);
    const py = Math.min(Math.max(1 - y / height, 0), 1);
    const points = this.curvePoints[this.curveChannel];

    if (phase === TouchType.Down) {
      let best = -1;
      let bestDistance = 999;
      for (let i = 0; i < points.length; i++) {
        const dx = (points[i].x - px) * width;
        const dy = (points[i].y - py) * height;
        const distance = Math.sqrt(dx * dx + dy * dy);
        if (distance < bestDistance) {
          bestDistance = distance;
          best = i;
        }
      }
      if (bestDistance <= 26) {
        const now = Date.now();
        const isEdge = best === 0 || best === points.length - 1;
        if (this.lastTapPoint === best && now - this.lastTapPointTime < 400 && !isEdge) {
          points.splice(best, 1);
          this.lastTapPoint = -1;
          this.activeCurvePoint = -1;
          this.drawCurveEditor();
          this.scheduleRender(false);
          return;
        }
        this.activeCurvePoint = best;
        this.lastTapPoint = best;
        this.lastTapPointTime = now;
        this.drawCurveEditor();
        return;
      }
      // 空白处点击：加一个控制点
      let index = points.length;
      for (let i = 0; i < points.length; i++) {
        if (points[i].x > px) {
          index = i;
          break;
        }
      }
      points.splice(index, 0, new CurvePt(px, py));
      this.activeCurvePoint = index;
      this.drawCurveEditor();
      this.scheduleRender(true);
      return;
    }

    if (phase === TouchType.Move && this.activeCurvePoint >= 0 && this.activeCurvePoint < points.length) {
      const point = points[this.activeCurvePoint];
      if (this.activeCurvePoint === 0) {
        point.x = 0;
        point.y = py;
      } else if (this.activeCurvePoint === points.length - 1) {
        point.x = 1;
        point.y = py;
      } else {
        const left = points[this.activeCurvePoint - 1].x + 0.01;
        const right = points[this.activeCurvePoint + 1].x - 0.01;
        point.x = Math.min(Math.max(px, left), Math.max(left, right));
        point.y = py;
      }
      this.drawCurveEditor();
      this.scheduleRender(true);
      return;
    }

    if (phase === TouchType.Up || phase === TouchType.Cancel) {
      if (this.activeCurvePoint >= 0) {
        this.scheduleRender(false);
      }
      this.activeCurvePoint = -1;
      this.drawCurveEditor();
    }
  }

  private resetCurve(): void {
    this.curvePoints[this.curveChannel] = [new CurvePt(0, 0), new CurvePt(1, 1)];
    this.drawCurveEditor();
    this.scheduleRender(false);
  }

  // MARK: - 裁剪

  private enterCrop(): void {
    this.cropSaved = this.cropRect.slice();
    this.cropMode = true;
    this.scheduleRender(false);
  }

  private applyCrop(): void {
    this.cropMode = false;
    this.scheduleRender(false);
  }

  private cancelCrop(): void {
    this.cropRect = this.cropSaved;
    this.cropMode = false;
    this.scheduleRender(false);
  }

  private setAspect(ratio: number): void {
    if (ratio <= 0 || this.lastW <= 0 || this.lastH <= 0) {
      return;
    }
    const imageAspect = this.lastW / this.lastH;
    let width = 1;
    let height = 1;
    if (ratio > imageAspect) {
      width = 1;
      height = imageAspect / ratio;
    } else {
      height = 1;
      width = ratio / imageAspect;
    }
    const centerX = this.cropRect[0] + this.cropRect[2] / 2;
    const centerY = this.cropRect[1] + this.cropRect[3] / 2;
    let x = centerX - width / 2;
    let y = centerY - height / 2;
    x = Math.min(Math.max(x, 0), 1 - width);
    y = Math.min(Math.max(y, 0), 1 - height);
    this.cropRect = [x, y, width, height];
    this.drawCropOverlay();
  }

  private drawCropOverlay(): void {
    const ctx = this.cropContext;
    const width = ctx.width;
    const height = ctx.height;
    ctx.clearRect(0, 0, width, height);
    if (width <= 0 || height <= 0 || this.lastW <= 0 || this.lastH <= 0) {
      return;
    }
    const imageAspect = this.lastW / this.lastH;
    const areaAspect = width / height;
    let imageW = width;
    let imageH = height;
    let imageX = 0;
    let imageY = 0;
    if (imageAspect > areaAspect) {
      imageW = width;
      imageH = width / imageAspect;
      imageY = (height - imageH) / 2;
    } else {
      imageH = height;
      imageW = height * imageAspect;
      imageX = (width - imageW) / 2;
    }
    const rx = imageX + this.cropRect[0] * imageW;
    const ry = imageY + this.cropRect[1] * imageH;
    const rw = this.cropRect[2] * imageW;
    const rh = this.cropRect[3] * imageH;

    // 裁掉的部分压暗
    ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
    ctx.fillRect(0, 0, width, ry);
    ctx.fillRect(0, ry + rh, width, height - ry - rh);
    ctx.fillRect(0, ry, rx, rh);
    ctx.fillRect(rx + rw, ry, width - rx - rw, rh);

    // 边框 + 三分线
    ctx.strokeStyle = '#FFFFFF';
    ctx.lineWidth = 1.5;
    ctx.strokeRect(rx, ry, rw, rh);
    ctx.strokeStyle = 'rgba(255,255,255,0.35)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 3; i++) {
      ctx.beginPath();
      ctx.moveTo(rx + (rw * i) / 3, ry);
      ctx.lineTo(rx + (rw * i) / 3, ry + rh);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(rx, ry + (rh * i) / 3);
      ctx.lineTo(rx + rw, ry + (rh * i) / 3);
      ctx.stroke();
    }

    // 四角把手
    const corners: number[][] = [
      [rx, ry], [rx + rw, ry], [rx, ry + rh], [rx + rw, ry + rh]
    ];
    ctx.fillStyle = '#FFFFFF';
    for (let i = 0; i < corners.length; i++) {
      ctx.fillRect(corners[i][0] - 6, corners[i][1] - 6, 12, 12);
    }
  }

  private handleCropTouch(x: number, y: number, phase: number): void {
    const ctx = this.cropContext;
    const width = ctx.width;
    const height = ctx.height;
    if (width <= 0 || height <= 0 || this.lastW <= 0 || this.lastH <= 0) {
      return;
    }
    const imageAspect = this.lastW / this.lastH;
    const areaAspect = width / height;
    let imageW = width;
    let imageH = height;
    let imageX = 0;
    let imageY = 0;
    if (imageAspect > areaAspect) {
      imageW = width;
      imageH = width / imageAspect;
      imageY = (height - imageH) / 2;
    } else {
      imageH = height;
      imageW = height * imageAspect;
      imageX = (width - imageW) / 2;
    }
    const nx = Math.min(Math.max((x - imageX) / imageW, 0), 1);
    const ny = Math.min(Math.max((y - imageY) / imageH, 0), 1);
    const rx = imageX + this.cropRect[0] * imageW;
    const ry = imageY + this.cropRect[1] * imageH;
    const rw = this.cropRect[2] * imageW;
    const rh = this.cropRect[3] * imageH;

    if (phase === TouchType.Down) {
      const corners: number[][] = [
        [rx, ry], [rx + rw, ry], [rx, ry + rh], [rx + rw, ry + rh]
      ];
      this.cropHandle = -1;
      for (let i = 0; i < corners.length; i++) {
        const dx = x - corners[i][0];
        const dy = y - corners[i][1];
        if (Math.sqrt(dx * dx + dy * dy) <= 30) {
          this.cropHandle = i;
          break;
        }
      }
      if (this.cropHandle < 0 && x >= rx && x <= rx + rw && y >= ry && y <= ry + rh) {
        this.cropHandle = 4;
      }
      this.cropLastX = nx;
      this.cropLastY = ny;
      return;
    }

    if (phase === TouchType.Move && this.cropHandle >= 0) {
      const rect = this.cropRect.slice();
      const dx = nx - this.cropLastX;
      const dy = ny - this.cropLastY;
      const minimum = 0.06;
      if (this.cropHandle === 4) {
        rect[0] = Math.min(Math.max(rect[0] + dx, 0), 1 - rect[2]);
        rect[1] = Math.min(Math.max(rect[1] + dy, 0), 1 - rect[3]);
      } else {
        let left = rect[0];
        let top = rect[1];
        let right = rect[0] + rect[2];
        let bottom = rect[1] + rect[3];
        if (this.cropHandle === 0) {
          left = Math.min(Math.max(left + dx, 0), right - minimum);
          top = Math.min(Math.max(top + dy, 0), bottom - minimum);
        } else if (this.cropHandle === 1) {
          right = Math.max(Math.min(right + dx, 1), left + minimum);
          top = Math.min(Math.max(top + dy, 0), bottom - minimum);
        } else if (this.cropHandle === 2) {
          left = Math.min(Math.max(left + dx, 0), right - minimum);
          bottom = Math.max(Math.min(bottom + dy, 1), top + minimum);
        } else {
          right = Math.max(Math.min(right + dx, 1), left + minimum);
          bottom = Math.max(Math.min(bottom + dy, 1), top + minimum);
        }
        rect[0] = left;
        rect[1] = top;
        rect[2] = right - left;
        rect[3] = bottom - top;
      }
      this.cropRect = rect;
      this.cropLastX = nx;
      this.cropLastY = ny;
      this.drawCropOverlay();
      return;
    }

    if (phase === TouchType.Up || phase === TouchType.Cancel) {
      this.cropHandle = -1;
      this.scheduleRender(false);
    }
  }

  // MARK: - 导出

  /** 导出当前参数下的照片（先落应用缓存目录，相册写入用安全控件，下一阶段接）。 */
  private async exportPhoto(): Promise<void> {
    this.statusText = '导出中…';
    try {
      const buffer = await this.decode(this.photoUri, 4096);
      if (buffer === null) {
        this.statusText = '解码失败';
        return;
      }
      const result = NativeBridge.render(buffer.data, buffer.width, buffer.height, this.paramsJson());
      const options: image.InitializationOptions = {
        size: { width: result.width, height: result.height },
        srcPixelFormat: image.PixelMapFormat.RGBA_8888,
        pixelFormat: image.PixelMapFormat.RGBA_8888,
        editable: false
      };
      const pixelMap = await image.createPixelMap(result.data, options);
      const packer = image.createImagePacker();
      const packed = await packer.packing(pixelMap, { format: 'image/jpeg', quality: 95 });
      pixelMap.release();
      packer.release();

      const context = getContext(this) as common.UIAbilityContext;

      // 优先写系统相册（需要 WRITE_IMAGEVIDEO 授权），失败则落到应用缓存目录。
      let savedToGallery = false;
      try {
        const manager = abilityAccessCtrl.createAtManager();
        const result = await manager.requestPermissionsFromUser(context, ['ohos.permission.WRITE_IMAGEVIDEO']);
        if (result.authResults.length > 0 && result.authResults[0] === 0) {
          const helper = photoAccessHelper.getPhotoAccessHelper(context);
          const uri = await helper.createAsset(photoAccessHelper.PhotoType.IMAGE, 'jpg');
          const file = fs.openSync(uri, fs.OpenMode.READ_WRITE);
          fs.writeSync(file.fd, packed);
          fs.closeSync(file);
          savedToGallery = true;
        }
      } catch (error) {
        savedToGallery = false;
      }

      if (savedToGallery) {
        this.statusText = '已保存到相册 ' + result.width + '×' + result.height;
        promptAction.showToast({ message: '已保存到相册' });
      } else {
        const path = context.cacheDir + '/fdxmp_export_' + Date.now() + '.jpg';
        const file = fs.openSync(path, fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE);
        fs.writeSync(file.fd, packed);
        fs.closeSync(file);
        this.statusText = '已导出 ' + result.width + '×' + result.height;
        promptAction.showToast({ message: '已导出到缓存：' + path });
      }
    } catch (error) {
      this.statusText = '导出失败';
      promptAction.showToast({ message: '导出失败：' + NativeBridge.lastError() });
    }
  }

  private pointColorsPayload(): PointColorPayload[] {
    const result: PointColorPayload[] = [];
    const count = Math.min(this.points.length, 4);
    for (let i = 0; i < count; i++) {
      const point = this.points[i];
      const payload: PointColorPayload = {
        lum: point.lum,
        chroma: point.chroma,
        hue: point.hue,
        hue_shift: point.hueShift,
        sat_shift: point.satShift,
        lum_shift: point.lumShift,
        variance: 0,
        range: 50,
        hue_range: point.hueRange,
        sat_range: point.satRange,
        lum_range: point.lumRange
      };
      result.push(payload);
    }
    return result;
  }

  // MARK: - 点颜色取色

  /** 预览上点击取色：把屏幕坐标换算到图像坐标，取 480px 缓冲里的原图颜色。 */
  private sampleAt(x: number, y: number): void {
    const buffer = this.fast;
    if (buffer === null || this.areaW <= 0 || this.areaH <= 0 || this.lastW <= 0 || this.lastH <= 0) {
      return;
    }
    const imageAspect = this.lastW / this.lastH;
    const areaAspect = this.areaW / this.areaH;
    let imageW = this.areaW;
    let imageH = this.areaH;
    let imageX = 0;
    let imageY = 0;
    if (imageAspect > areaAspect) {
      imageW = this.areaW;
      imageH = this.areaW / imageAspect;
      imageY = (this.areaH - imageH) / 2;
    } else {
      imageH = this.areaH;
      imageW = this.areaH * imageAspect;
      imageX = (this.areaW - imageW) / 2;
    }
    const nx = (x - imageX) / imageW;
    const ny = (y - imageY) / imageH;
    if (nx < 0 || nx > 1 || ny < 0 || ny > 1) {
      return;
    }
    // 预览可能已按 crop 裁过：把显示坐标映射回原图坐标再取样。
    const crop = this.cropMode ? [0, 0, 1, 1] : this.cropRect;
    const originX = crop[0] + nx * crop[2];
    const originY = crop[1] + ny * crop[3];
    const px = Math.min(buffer.width - 1, Math.max(0, Math.round(originX * buffer.width)));
    const py = Math.min(buffer.height - 1, Math.max(0, Math.round(originY * buffer.height)));
    const pixels = new Uint8Array(buffer.data);
    const offset = (py * buffer.width + px) * 4;
    const r = pixels[offset];
    const g = pixels[offset + 1];
    const b = pixels[offset + 2];
    const lch = NativeBridge.oklch(r, g, b);
    if (lch.length < 3) {
      return;
    }
    const sample = new PointSample();
    sample.r = r;
    sample.g = g;
    sample.b = b;
    sample.lum = lch[0];
    sample.chroma = lch[1];
    sample.hue = (lch[2] * 180) / Math.PI;
    this.points = this.points.concat([sample]);
    this.selectedPoint = this.points.length - 1;
    this.picking = false;
    this.scheduleRender(false);
    promptAction.showToast({ message: '已取色（' + r + ', ' + g + ', ' + b + '）' });
  }

  private updatePoint(index: number, mutate: (point: PointSample) => void): void {
    if (index < 0 || index >= this.points.length) {
      return;
    }
    const next = this.points.slice();
    const copy = new PointSample();
    const source = next[index];
    copy.r = source.r;
    copy.g = source.g;
    copy.b = source.b;
    copy.lum = source.lum;
    copy.chroma = source.chroma;
    copy.hue = source.hue;
    copy.hueShift = source.hueShift;
    copy.satShift = source.satShift;
    copy.lumShift = source.lumShift;
    copy.hueRange = source.hueRange;
    copy.satRange = source.satRange;
    copy.lumRange = source.lumRange;
    mutate(copy);
    next[index] = copy;
    this.points = next;
    this.scheduleRender(true);
  }

  private removePoint(index: number): void {
    if (index < 0 || index >= this.points.length) {
      return;
    }
    const next: PointSample[] = [];
    for (let i = 0; i < this.points.length; i++) {
      if (i !== index) {
        next.push(this.points[i]);
      }
    }
    this.points = next;
    this.selectedPoint = Math.max(0, Math.min(this.selectedPoint, next.length - 1));
    this.scheduleRender(false);
  }

  // MARK: - 预设

  private async initPresets(): Promise<void> {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      this.presetStore = await preferences.getPreferences(context, 'fdxmp_presets');
      const raw = await this.presetStore.get('names', '[]') as string;
      this.presetNames = JSON.parse(raw) as string[];
    } catch (error) {
      this.presetNames = [];
    }
  }

  private async savePreset(): Promise<void> {
    const store = this.presetStore;
    if (store === null) {
      return;
    }
    const name = '预设 ' + (this.presetNames.length + 1);
    try {
      await store.put('preset_' + name, this.paramsJson());
      const names = this.presetNames.concat([name]);
      await store.put('names', JSON.stringify(names));
      await store.flush();
      this.presetNames = names;
      promptAction.showToast({ message: '已保存 ' + name });
    } catch (error) {
      promptAction.showToast({ message: '保存失败' });
    }
  }

  private async applyPreset(name: string): Promise<void> {
    const store = this.presetStore;
    if (store === null) {
      return;
    }
    try {
      const raw = await store.get('preset_' + name, '') as string;
      if (raw.length === 0) {
        return;
      }
      const payload = JSON.parse(raw) as ParamsPayload;
      this.applyPayload(payload);
      this.showPresets = false;
      this.drawGraph();
      this.drawCurveEditor();
      this.scheduleRender(false);
      promptAction.showToast({ message: '已应用 ' + name });
    } catch (error) {
      promptAction.showToast({ message: '应用失败' });
    }
  }

  private async deletePreset(name: string): Promise<void> {
    const store = this.presetStore;
    if (store === null) {
      return;
    }
    try {
      await store.delete('preset_' + name);
      const names: string[] = [];
      for (let i = 0; i < this.presetNames.length; i++) {
        if (this.presetNames[i] !== name) {
          names.push(this.presetNames[i]);
        }
      }
      await store.put('names', JSON.stringify(names));
      await store.flush();
      this.presetNames = names;
    } catch (error) {
      promptAction.showToast({ message: '删除失败' });
    }
  }

  /** 把一份参数 JSON 灌回全部 UI 状态。 */
  private applyPayload(payload: ParamsPayload): void {
    const next = this.nums.slice();
    next[I_TEMPERATURE] = payload.color.temperature;
    next[I_TINT] = payload.color.tint;
    next[I_VIBRANCE] = payload.color.vibrance;
    next[I_SATURATION] = payload.color.saturation;
    next[I_EQ_THRESHOLD] = payload.equalizer.threshold;
    next[I_EQ_SMOOTHING] = payload.equalizer.smoothing_hue;
    next[I_EQ_CONTRAST] = payload.equalizer.contrast;
    next[I_CAL_RED_HUE] = payload.calibration.red_hue;
    next[I_CAL_RED_SAT] = payload.calibration.red_sat;
    next[I_CAL_GREEN_HUE] = payload.calibration.green_hue;
    next[I_CAL_GREEN_SAT] = payload.calibration.green_sat;
    next[I_CAL_BLUE_HUE] = payload.calibration.blue_hue;
    next[I_CAL_BLUE_SAT] = payload.calibration.blue_sat;
    next[I_CAL_SHADOW_TINT] = payload.calibration.shadows_tint;
    next[I_CA] = payload.lens.chromatic_aberration;
    next[I_LENS_VIGNETTE] = payload.lens.vignette;
    next[I_DISTORTION] = payload.lens.geometry.distortion;
    next[I_VERTICAL] = payload.lens.geometry.vertical;
    next[I_HORIZONTAL] = payload.lens.geometry.horizontal;
    next[I_ROTATE] = payload.lens.geometry.rotate;
    next[I_SCALE] = payload.lens.geometry.scale;
    next[I_TEXTURE] = payload.effects.texture;
    next[I_CLARITY] = payload.effects.clarity;
    next[I_DEHAZE] = payload.effects.dehaze;
    next[I_VIGNETTE] = payload.effects.vignette;
    next[I_VIGNETTE_MID] = payload.effects.vignette_midpoint;
    next[I_VIGNETTE_FEATHER] = payload.effects.vignette_feather;
    next[I_GRAIN] = payload.effects.grain;
    next[I_GRAIN_SIZE] = payload.effects.grain_size;
    next[I_GRAIN_ROUGH] = payload.effects.grain_roughness;
    next[I_SHARPEN_AMOUNT] = payload.detail.sharpen.amount;
    next[I_SHARPEN_RADIUS] = payload.detail.sharpen.radius;
    next[I_SHARPEN_DETAIL] = payload.detail.sharpen.detail;
    next[I_SHARPEN_MASK] = payload.detail.sharpen.masking;
    next[I_NOISE_LUM] = payload.detail.noise.luminance;
    next[I_NOISE_LUM_DETAIL] = payload.detail.noise.luminance_detail;
    next[I_NOISE_LUM_CONTRAST] = payload.detail.noise.luminance_contrast;
    next[I_NOISE_COLOR] = payload.detail.noise.color;
    next[I_NOISE_COLOR_DETAIL] = payload.detail.noise.color_detail;
    next[I_NOISE_COLOR_SMOOTH] = payload.detail.noise.color_smoothness;
    this.nums = next;
    this.blackWhite = payload.color.black_white;
    this.lensProfile = payload.lens.profile_correction;
    this.hslHue = [payload.hsl.red.hue, payload.hsl.orange.hue, payload.hsl.yellow.hue,
      payload.hsl.green.hue, payload.hsl.aqua.hue, payload.hsl.blue.hue,
      payload.hsl.purple.hue, payload.hsl.magenta.hue];
    this.hslSat = [payload.hsl.red.saturation, payload.hsl.orange.saturation,
      payload.hsl.yellow.saturation, payload.hsl.green.saturation, payload.hsl.aqua.saturation,
      payload.hsl.blue.saturation, payload.hsl.purple.saturation, payload.hsl.magenta.saturation];
    this.hslLum = [payload.hsl.red.luminance, payload.hsl.orange.luminance,
      payload.hsl.yellow.luminance, payload.hsl.green.luminance, payload.hsl.aqua.luminance,
      payload.hsl.blue.luminance, payload.hsl.purple.luminance, payload.hsl.magenta.luminance];
    this.gradingHue = [payload.grading.shadows.hue, payload.grading.midtones.hue,
      payload.grading.highlights.hue, payload.grading.global.hue];
    this.gradingSat = [payload.grading.shadows.saturation, payload.grading.midtones.saturation,
      payload.grading.highlights.saturation, payload.grading.global.saturation];
    this.gradingLum = [payload.grading.shadows.luminance, payload.grading.midtones.luminance,
      payload.grading.highlights.luminance, payload.grading.global.luminance];
    this.gradingBlending = payload.grading.blending;
    this.gradingBalance = payload.grading.balance;
    this.bwMix = [payload.bw_mix.red, payload.bw_mix.orange, payload.bw_mix.yellow,
      payload.bw_mix.green, payload.bw_mix.aqua, payload.bw_mix.blue, payload.bw_mix.purple,
      payload.bw_mix.magenta];
    this.eqHue = payload.equalizer.hue;
    this.eqSat = payload.equalizer.saturation;
    this.eqBri = payload.equalizer.brightness;
    this.curvePoints = [
      this.pointsFrom(payload.curves.rgb),
      this.pointsFrom(payload.curves.red),
      this.pointsFrom(payload.curves.green),
      this.pointsFrom(payload.curves.blue)
    ];
  }

  private pointsFrom(curve: ToneCurvePayload): CurvePt[] {
    const points: CurvePt[] = [];
    for (let i = 0; i < curve.points.length; i++) {
      points.push(new CurvePt(curve.points[i].x, curve.points[i].y));
    }
    if (points.length < 2) {
      points.push(new CurvePt(0, 0));
      points.push(new CurvePt(1, 1));
    }
    return points;
  }

  // MARK: - 滑块

  /** 标量滑块（按 SLIDERS 下标取值）。 */
  @Builder
  sliderRow(index: number) {
    Row({ space: 8 }) {
      Text(SLIDERS[index].label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(72)
      Slider({
        value: this.num(index),
        min: SLIDERS[index].min,
        max: SLIDERS[index].max,
        step: SLIDERS[index].step
      })
        .layoutWeight(1)
        .blockColor('#E8E8E8')
        .trackColor('#3A3A3A')
        .selectedColor('#4A9EFF')
        .onChange((changed: number) => {
          this.applyScalar(index, changed);
        })
      Text(this.formatValue(this.num(index), SLIDERS[index].step))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(42)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }

  /** 色带滑块（HSL / 分级）。 */
  @Builder
  bandSliderRow(label: string, value: number, kind: number) {
    Row({ space: 8 }) {
      Text(label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(64)
      Slider({ value: value, min: -100, max: 100, step: 1 })
        .layoutWeight(1)
        .blockColor('#E8E8E8')
        .trackColor('#3A3A3A')
        .selectedColor('#4A9EFF')
        .onChange((changed: number) => {
          this.applyBand(kind, changed);
        })
      Text(this.formatValue(value, 1))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(42)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }

  @Builder
  plainSliderRow(label: string, value: number, min: number, max: number, step: number,
    kind: number) {
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
          this.applyBand(kind, changed);
        })
      Text(this.formatValue(value, step))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(42)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }

  @Builder
  bwSliderRow(label: string, value: number, index: number) {
    Row({ space: 8 }) {
      Text(label)
        .fontSize(11)
        .fontColor('#AAAAAA')
        .width(52)
      Slider({ value: value, min: -100, max: 100, step: 1 })
        .layoutWeight(1)
        .blockColor('#E8E8E8')
        .trackColor('#3A3A3A')
        .selectedColor('#4A9EFF')
        .onChange((changed: number) => {
          const next = this.bwMix.slice();
          next[index] = changed;
          this.bwMix = next;
          this.scheduleRender(true);
        })
      Text(this.formatValue(value, 1))
        .fontSize(11)
        .fontColor('#CCCCCC')
        .width(42)
        .textAlign(TextAlign.End)
    }
    .width('100%')
  }

  private formatValue(value: number, step: number): string {
    return value.toFixed(step < 0.1 ? 2 : 0);
  }

  private applyScalar(index: number, value: number): void {
    const next = this.nums.slice();
    next[index] = value;
    this.nums = next;
    if (index === I_EQ_THRESHOLD || index === I_EQ_SMOOTHING || index === I_EQ_CONTRAST) {
      this.drawGraph();
    }
    this.scheduleRender(true);
  }

  private applyBand(kind: number, value: number): void {
    if (kind === 100) {
      const next = this.hslHue.slice();
      next[this.hslBand] = value;
      this.hslHue = next;
    } else if (kind === 101) {
      const next = this.hslSat.slice();
      next[this.hslBand] = value;
      this.hslSat = next;
    } else if (kind === 102) {
      const next = this.hslLum.slice();
      next[this.hslBand] = value;
      this.hslLum = next;
    } else if (kind === 103) {
      const next = this.gradingHue.slice();
      next[this.gradingZone] = value;
      this.gradingHue = next;
    } else if (kind === 104) {
      const next = this.gradingSat.slice();
      next[this.gradingZone] = value;
      this.gradingSat = next;
    } else if (kind === 105) {
      const next = this.gradingLum.slice();
      next[this.gradingZone] = value;
      this.gradingLum = next;
    } else if (kind === 106) {
      this.gradingBlending = value;
    } else if (kind === 107) {
      this.gradingBalance = value;
    }
    this.scheduleRender(true);
  }
}
