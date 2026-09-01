@echo off
set /p _observation=

if "%~1"=="--decision-only" goto decision_only

set "_present=0"
if defined %~1 set "_present=1"
set "_path=0"
if defined PATH set "_path=1"
echo {"orders":[],"reasoning":"var=%_present% path=%_path%"}
exit /b 0

:decision_only
echo {"orders":[]}
