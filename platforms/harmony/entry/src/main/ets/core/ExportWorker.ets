// 导出工作线程：解码 → 分块渲染 → 编码全部在 TaskPool 里完成。
//
// UI 线程只负责显示进度和写相册。之前这些步骤都在 UI 线程跑，
// 32MP 的 readPixelsToBuffer 单次就 4 秒以上，直接触发 THREAD_BLOCK_6S。
import { image } from '@kit.ImageKit';
import { fileIo as fs } from '@kit.CoreFileKit';
import nativeModule from 'libfdxmp_ohos_bridge.so';

interface RenderResult {
  width: number;
  height: number;
  data: ArrayBuffer;
}

interface FdxmpApi {
  renderTiledRgba(
    rgba: ArrayBuffer,
    width: number,
    height: number,
    paramsJson: string,
    stripPixels: number
  ): RenderResult;
  lastError(): string;
}

/**
 * 导出编码（在 TaskPool 线程执行）。
 *
 * @param uri 照片 uri
 * @param paramsJson 调色参数
 * @param mime image/jpeg 或 image/png
 * @param quality 0..100
 * @param maxEdge 解码最长边；<= 0 表示原图尺寸
 * @returns 编码后的文件字节
 */
@Concurrent
export async function encodeExport(
  uri: string,
  paramsJson: string,
  mime: string,
  quality: number,
  maxEdge: number
): Promise<ArrayBuffer> {
  const api: FdxmpApi = nativeModule as FdxmpApi;
  const file = fs.openSync(uri, fs.OpenMode.READ_ONLY);
  try {
    const source = image.createImageSource(file.fd);
    const info = await source.getImageInfo();
    let width = info.size.width;
    let height = info.size.height;
    const longest = Math.max(width, height);
    if (maxEdge > 0 && longest > maxEdge) {
      const scale = maxEdge / longest;
      width = Math.max(1, Math.round(width * scale));
      height = Math.max(1, Math.round(height * scale));
    }
    const pixelMap = await source.createPixelMap({
      desiredSize: { width: width, height: height }
    });
    const buffer = new ArrayBuffer(width * height * 4);
    await pixelMap.readPixelsToBuffer(buffer);
    pixelMap.release();
    source.release();

    const rendered = api.renderTiledRgba(buffer, width, height, paramsJson, 2_000_000);
    if (rendered.width <= 0 || rendered.height <= 0 || rendered.data.byteLength <= 0) {
      throw new Error('渲染失败：' + api.lastError());
    }

    const outMap = await image.createPixelMap(rendered.data, {
      size: { width: rendered.width, height: rendered.height },
      srcPixelFormat: image.PixelMapFormat.RGBA_8888,
      pixelFormat: image.PixelMapFormat.RGBA_8888,
      editable: false
    });
    const packer = image.createImagePacker();
    const packed = await packer.packing(outMap, { format: mime, quality: quality });
    outMap.release();
    packer.release();
    return packed;
  } finally {
    fs.closeSync(file);
  }
}
