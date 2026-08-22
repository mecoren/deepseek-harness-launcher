@echo off
:: DeepSeek Harness Launcher - one-click dev runner (ASCII only so any
:: codepage renders it correctly; batch files are codepage-sensitive).
:: Locates the release build and starts it; hints how to build if missing.
setlocal
set "EXE=%~dp0src-tauri\target\x86_64-pc-windows-msvc\release\deepseek_harness_launcher.exe"

if exist "%EXE%" (
    start "" "%EXE%" %*
    exit /b 0
)

echo [DSH] Release build not found. Build it first with:
echo.
echo     cargo build --release --target x86_64-pc-windows-msvc
echo     or: npm run build:bin
echo.
pause
exit /b 1
