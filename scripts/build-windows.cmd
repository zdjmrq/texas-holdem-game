@echo off
setlocal
rem Windows x64 portable build entry point.
rem Usage: build-windows.cmd [-SkipBuild] [-Clean]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*
if errorlevel 1 (
    echo.
    echo BUILD FAILED - see the error above.
    exit /b 1
)
echo.
echo BUILD OK - see dist\texas-holdem-rs-current\. No zip is generated.
exit /b 0
