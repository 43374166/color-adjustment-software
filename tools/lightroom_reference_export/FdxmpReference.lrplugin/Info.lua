return {
    LrSdkVersion = 5.0,
    LrSdkMinimumVersion = 5.0,
    LrToolkitIdentifier = 'com.fdxmp.reference',
    LrPluginName = "FDXMP Reference Export",
    LrInitPlugin = "PluginInit.lua",
    LrForceInitPlugin = true,
    LrShutdownPlugin = "PluginShutdown.lua",
    LrShutdownApp = "AppShutdown.lua",
    LrExportMenuItems = {
        { title = "FDXMP: Probe", file = "MenuActions.lua" },
    },
    VERSION = { major=1, minor=0, revision=0, build=1 },
}
