/**
 * fdxmp 原生桥接（libfdxmp_ohos_bridge.so）的类型声明。
 *
 * ArkTS 侧通过 `import fdxmp from 'libfdxmp_ohos_bridge.so'` 使用。
 */

export interface RenderResult {
  /** 输出宽度（裁切 / 几何之后）。 */
  width: number;
  /** 输出高度。 */
  height: number;
  /** 行优先 RGBA8 像素。 */
  data: ArrayBuffer;
}

/** 核心版本号。 */
export const version: () => string;

/** C ABI 版本号。 */
export const abiVersion: () => number;

/** 最近一次错误的文本（没有错误时为空串）。 */
export const lastError: () => string;

/**
 * 对内存中的 RGBA8 图像应用参数 JSON，返回渲染结果。
 *
 * 阻塞调用，必须放到 TaskPool / Worker 里跑。
 */
export const renderMemoryRgba: (
  rgba: ArrayBuffer,
  width: number,
  height: number,
  paramsJson: string
) => RenderResult;

/**
 * 颜色均衡器曲线（512 个 float32，单位见 channel）。
 *
 * channel：0 色相（弧度）/ 1 饱和度增益 / 2 亮度增益；
 * paramsJson 传整份渲染参数（只读其中的 `equalizer`）。
 */
export const equalizerCurve: (channel: number, paramsJson: string) => ArrayBuffer;

/** 8bit sRGB → OkLCh（3 个 float32：L / C / 色相弧度）。 */
export const oklch: (r: number, g: number, b: number) => ArrayBuffer;
