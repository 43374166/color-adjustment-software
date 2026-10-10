; fdxmp Windows 安装向导（NSIS 3.x / Unicode）
; 生成：用 NSIS 的 makensis 编译本脚本 → dist\fdxmp-setup.exe
Unicode true

!define APP_NAME "fdxmp"
!define APP_VERSION "1.0.1"
!define APP_PUBLISHER "fdxmp"
!define APP_DESC "fdxmp 专业图像处理（对标 Lightroom）"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "..\..\dist\fdxmp-setup.exe"
; 用户级安装：无需管理员 / UAC；目录页可改到任意盘（例如 F:\fdxmp）。
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "主程序" SecMain
    SetOutPath "$INSTDIR"
    ; 主程序 + MinGW / libheif 运行时
    File "..\..\dist\windows\fdxmp-win-ui.exe"
    File "..\..\dist\windows\*.dll"
    File /nonfatal "..\..\dist\windows\README.txt"

    WriteUninstaller "$INSTDIR\uninstall.exe"

    CreateShortCut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\fdxmp-win-ui.exe"
    CreateDirectory "$SMPROGRAMS\${APP_NAME}"
    CreateShortCut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\fdxmp-win-ui.exe"
    CreateShortCut "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"

    WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${APP_PUBLISHER}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\fdxmp-win-ui.exe"
    WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
    WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINST_KEY}" "Comments" "${APP_DESC}"
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
    WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" 90000
SectionEnd

Section "Uninstall"
    Delete "$DESKTOP\${APP_NAME}.lnk"
    Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
    Delete "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk"
    RMDir "$SMPROGRAMS\${APP_NAME}"
    DeleteRegKey HKCU "${UNINST_KEY}"
    DeleteRegKey HKCU "Software\${APP_NAME}"
    RMDir /r "$INSTDIR"
SectionEnd
