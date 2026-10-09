// 相册中的一张照片（对应 photoAccessHelper.PhotoAsset 的轻量快照）。
export class PhotoItem {
  uri: string;
  name: string;
  sizeBytes: number;
  dateAdded: number;
  width: number;
  height: number;

  constructor(
    uri: string,
    name: string,
    sizeBytes: number,
    dateAdded: number,
    width: number,
    height: number,
  ) {
    this.uri = uri;
    this.name = name;
    this.sizeBytes = sizeBytes;
    this.dateAdded = dateAdded;
    this.width = width;
    this.height = height;
  }

  isRaw(): boolean {
    const lower = this.name.toLowerCase();
    const extensions: string[] = [
      '.dng', '.cr2', '.cr3', '.nef', '.nrw', '.arw', '.raf', '.rw2',
      '.orf', '.pef', '.srw', '.raw', '.rwl', '.x3f', '.3fr',
    ];
    for (let i = 0; i < extensions.length; i++) {
      if (lower.endsWith(extensions[i])) {
        return true;
      }
    }
    return false;
  }

  displaySize(): string {
    if (this.sizeBytes >= 1024 * 1024) {
      return (this.sizeBytes / 1024 / 1024).toFixed(1) + ' MB';
    }
    if (this.sizeBytes >= 1024) {
      return (this.sizeBytes / 1024).toFixed(0) + ' KB';
    }
    return this.sizeBytes.toString() + ' B';
  }

  pixelSize(): string {
    if (this.width > 0 && this.height > 0) {
      return this.width.toString() + ' × ' + this.height.toString();
    }
    return '尺寸未知';
  }
}
