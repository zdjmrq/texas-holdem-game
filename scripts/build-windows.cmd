@echo off
setlocal
rem Windows x64 portable build entry point.
rem Usage: build-windows.cmd [-SkipBuild] [-NoZip] [-Clean]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*
if errorlevel 1 (
    echo.
    echo BUILD FAILED - see dist\build-windows.log
    exit /b 1
)
echo.
echo BUILD OK - see dist\ for the portable directory and zip.
exit /b 0
