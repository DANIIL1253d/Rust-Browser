@echo off
cd /d "%~dp0"
start "Rust Browser" "%~dp0app\RustBrowser.exe" %*
