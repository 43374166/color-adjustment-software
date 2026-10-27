import UIAbility from '@ohos.app.ability.UIAbility';
import window from '@ohos.window';
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
    // 底部安全区（手势条）用工具条同色填充，避免出现一条黑边。
    try {
      const mainWindow = windowStage.getMainWindowSync();
      mainWindow.setWindowBackgroundColor('#141414');
    } catch (error) {
      hilog.warn(0x0000, 'fdxmp', 'setWindowBackgroundColor failed');
    }
  }

  onDestroy(): void {
    hilog.info(0x0000, 'fdxmp', 'EntryAbility onDestroy');
  }
}

