import UIAbility from '@ohos.app.ability.UIAbility';
import window from '@ohos.window';
import display from '@ohos.display';
import hilog from '@ohos.hilog';

export default class EntryAbility extends UIAbility {
  onCreate(): void {
    hilog.info(0x0000, 'fdxmp', 'EntryAbility onCreate');
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(0x0000, 'fdxmp', 'loadContent failed: %{public}s', JSON.stringify(err));
      }
    });

    try {
      const mainWindow = windowStage.getMainWindowSync();
      // 安全区兜底色（页面没铺满时也不出现纯黑条）。
      mainWindow.setWindowBackgroundColor('#141414');

      // 全屏布局：让照片 / 面板一直画到屏幕底边（含手势条区域）。
      mainWindow.setWindowLayoutFullScreen(true).then(() => {
        const density = display.getDefaultDisplaySync().densityPixels;
        const top = mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM);
        const bottom = mainWindow.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR);
        const safeTop = density > 0 ? top.topRect.height / density : 0;
        const safeBottom = density > 0 ? bottom.bottomRect.height / density : 0;
        AppStorage.setOrCreate('safeTop', safeTop);
        AppStorage.setOrCreate('safeBottom', safeBottom);
        hilog.info(0x0000, 'fdxmp', 'safe area top=%{public}f bottom=%{public}f', safeTop, safeBottom);
      }).catch((error: Error) => {
        hilog.warn(0x0000, 'fdxmp', 'setWindowLayoutFullScreen failed: %{public}s', error.message);
      });
    } catch (error) {
      hilog.warn(0x0000, 'fdxmp', 'window setup failed');
    }
  }

  onDestroy(): void {
    hilog.info(0x0000, 'fdxmp', 'EntryAbility onDestroy');
  }
}
