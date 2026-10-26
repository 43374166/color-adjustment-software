# Lightroom reference export

用 Lightroom Classic 自动导出“同参数参考图”，供 `tools/compare_lr_basic.py`
或后续曲线 / 颜色对照使用。

## 安装（只需一次）

1. 打开 Lightroom Classic；
2. `文件 → 增效工具管理器`（快捷键 `Ctrl+Alt+Shift+,`）；
3. `添加`，选择 `FdxmpReference.lrplugin` 文件夹；
4. 确认状态为“已安装并正在运行”，点“完成”。

Lightroom 不会自动扫描 `Modules` 目录，必须走 Plug-in Manager 添加。

## 跑一组参考

1. 关闭 Lightroom；
2. 在仓库根目录写 `.tmp/lr-export-job.lua`（**UTF-8 无 BOM**，否则 Lua 会报
   `unexpected symbol near '?'`）：

```lua
return {
    expected_catalog = [[D:\Pictures\Lightroom\Lightroom Catalog.lrcat]],
    output_root = [[D:\color-adjustment-software\.tmp\lr-ref-basic]],
    images = {
        { path = [[D:\PS\2022-11.11\1.jpg]], key = 'landscape' },
        { path = [[D:\PS\2023-8-16\heng.jpg]], key = 'portrait' },
    },
    controls = {
        { name = 'base', settings = {} },
        { name = 'contrast_p100', settings = {
            Exposure2012 = 0, Contrast2012 = 100,
            Highlights2012 = 0, Shadows2012 = 0,
            Whites2012 = 0, Blacks2012 = 0,
        } },
    },
    auto_shutdown = false,
}
```

3. 启动 Lightroom；插件读到 job 后会自动 apply → export → 恢复原设置；
4. 进度看 `.tmp/lr-reference-export.log`，出现 `job done` 即完成；
5. **删掉 job 文件**，避免下次启动重复导出。

## 注意

- 插件会保存并恢复每张照片的 develop 设置，且原设置做了深拷贝；
  仍建议在 **catalog 副本** 上跑，避免参考导出影响正在编辑的 catalog；
- 需要故意把设置留在照片上时，job 里写 `restore_settings = false`；
- 导出设置固定为 JPEG 100 / sRGB / 关闭输出锐化 / 不缩放；需要缩放时在比较端做，
  保证 LR 与 fdxmp 使用同一尺寸；
- `settings` 里应显式写全同一组滑块（例如六个基本滑块都写 0），
  避免依赖 Lightroom 的增量 merge 行为；
- 参考图很大（24MP JPEG 约 7MB/张），批量导出前确认磁盘空间；
- 插件本身不包含任何 Adobe 代码，只用公开 Lightroom SDK API。
