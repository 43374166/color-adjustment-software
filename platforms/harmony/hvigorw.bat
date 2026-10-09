@echo off
rem fdxmp HarmonyOS command-line build entry (no DevEco Studio GUI required).
rem Requires DEVECO_STUDIO_HOME to point at the DevEco Studio install dir,
rem default: E:\installs\DevEco Studio (or run packaging\harmony\dev-env.ps1).
setlocal

if not defined DEVECO_STUDIO_HOME set "DEVECO_STUDIO_HOME=E:\installs\DevEco Studio"
if not exist "%DEVECO_STUDIO_HOME%\tools\hvigor\bin\hvigorw.js" (
  echo [hvigorw] ERROR: "%DEVECO_STUDIO_HOME%\tools\hvigor\bin\hvigorw.js" not found.
  echo [hvigorw] Set DEVECO_STUDIO_HOME or run: . .\packaging\harmony\dev-env.ps1
  exit /b 1
)

set "NODE_EXE=node.exe"
if defined NODE_HOME if exist "%NODE_HOME%\node.exe" set "NODE_EXE=%NODE_HOME%\node.exe"

if not defined DEVECO_SDK_HOME set "DEVECO_SDK_HOME=%DEVECO_STUDIO_HOME%\sdk"

"%NODE_EXE%" "%DEVECO_STUDIO_HOME%\tools\hvigor\bin\hvigorw.js" %*
exit /b %ERRORLEVEL%
