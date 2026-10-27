// fdxmp HarmonyOS 原生桥接。
//
// ArkTS → `libfdxmp_ohos_bridge.so`（N-API 薄封装，见 entry/src/main/cpp/napi_init.cpp）
//      → `libfdxmp_ffi.so`（Rust 核心，与 Windows / iOS 同一份调色管线）。
//
// 渲染是同步阻塞调用，必须放到 TaskPool / Worker 里执行，不要在 UI 线程直接调。

import nativeModule from 'libfdxmp_ohos_bridge.so';

/** 渲染结果：行优先 RGBA8。 */
export interface RenderResult {
  /** 输出宽度（裁切 / 几何之后）。 */
  width: number;
  /** 输出高度。 */
  height: number;
  /** 像素数据，长度 = width × height × 4。 */
  data: ArrayBuffer;
}

/** 原生模块的导出签名（与 entry/src/main/cpp/napi_init.cpp 一一对应）。 */
interface FdxmpNativeApi {
  version(): string;
  abiVersion(): number;
  lastError(): string;
  renderMemoryRgba(
    rgba: ArrayBuffer,
    width: number,
    height: number,
    paramsJson: string
  ): RenderResult;
  renderTiledRgba(
    rgba: ArrayBuffer,
    width: number,
    height: number,
    paramsJson: string,
    stripPixels: number
  ): RenderResult;
  equalizerCurve(channel: number, paramsJson: string): ArrayBuffer;
  oklch(r: number, g: number, b: number): ArrayBuffer;
}

const native: FdxmpNativeApi = nativeModule as FdxmpNativeApi;

export class NativeBridge {
  /** 核心版本号。 */
  static version(): string {
    return native.version();
  }

  /** C ABI 版本号。 */
  static abiVersion(): number {
    return native.abiVersion();
  }

  /** 最近一次错误（没有错误时为空串）。 */
  static lastError(): string {
    return native.lastError();
  }

  /**
   * 对 RGBA8 图像应用参数 JSON，返回渲染结果。
   *
   * 阻塞调用：放到 TaskPool 里跑。
   */
  static render(rgba: ArrayBuffer, width: number, height: number, paramsJson: string): RenderResult {
    const result: RenderResult = native.renderMemoryRgba(rgba, width, height, paramsJson);
    const out: RenderResult = {
      width: result.width,
      height: result.height,
      data: result.data
    };
    return out;
  }

  /**
   * 分块渲染整帧（导出用）：原图分辨率，单次内存峰值按 stripPixels 控制。
   * 几何非单位变换时核心内部回退整帧渲染。
   */
  static renderTiled(rgba: ArrayBuffer, width: number, height: number, paramsJson: string,
    stripPixels: number): RenderResult {
    const result: RenderResult = native.renderTiledRgba(rgba, width, height, paramsJson, stripPixels);
    const out: RenderResult = {
      width: result.width,
      height: result.height,
      data: result.data
    };
    return out;
  }

  /**
   * 颜色均衡器曲线（512 点）。
   *
   * channel：0 色相（弧度）/ 1 饱和度增益 / 2 亮度增益。
   */
  static equalizerCurve(channel: number, paramsJson: string): Float32Array {
    return new Float32Array(native.equalizerCurve(channel, paramsJson));
  }

  /** 8bit sRGB → OkLCh（L / C / 色相弧度）。 */
  static oklch(r: number, g: number, b: number): Float32Array {
    return new Float32Array(native.oklch(r, g, b));
  }
}

export default NativeBridge;
