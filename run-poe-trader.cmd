@echo off
setlocal

rem Start the overlay and keep its log.
rem
rem This file exists because a Smart App Control block leaves no trace at all.
rem The process never starts, so it writes no log, and the user is left with an
rem overlay that "does nothing" and an empty log file. That is exactly what
rem happened the first time this tool was run against a live game.

cd /d "%~dp0"

set GAME=%1
if "%GAME%"=="" set GAME=poe2

set DATA=data-%GAME%
if not exist "%DATA%\stats.ndjson" set DATA=data

echo Starting poe-trader for %GAME%, data from %DATA%
echo Log: %CD%\poe-trader.log
echo.

poe-trader.exe --game %GAME% --data-dir "%DATA%" > poe-trader.log 2>&1
set CODE=%ERRORLEVEL%

rem A blocked binary never runs, so the log is empty and the exit code is not
rem one the program itself can produce.
for %%A in (poe-trader.log) do set SIZE=%%~zA

if "%SIZE%"=="0" (
    echo.
    echo The overlay produced no output at all. It almost certainly never started.
    echo.
    echo On Windows 11 this is nearly always Smart App Control. It blocks
    echo programs that are neither signed by a publisher Microsoft trusts nor
    echo already known to its reputation service. Every fresh build is unknown
    echo again, so a rebuild that worked yesterday can be refused today.
    echo.
    echo Check with:
    echo   reg query "HKLM\SYSTEM\CurrentControlSet\Control\CI\Policy" /v VerifiedAndReputablePolicyState
    echo   0 = off, 1 = on, 2 = evaluation
    echo.
    echo Three ways forward, all of them yours to choose:
    echo   1. Turn Smart App Control off. Windows Security, App and browser
    echo      control, Smart App Control. THIS CANNOT BE UNDONE without
    echo      reinstalling Windows.
    echo   2. Sign the binary with a certificate from a CA in the Microsoft
    echo      Trusted Root Program. A self signed one will NOT work. Sectigo
    echo      sells one to individuals for about 220 dollars a year.
    echo   3. Run it on a machine or VM without Smart App Control.
    echo.
    goto :end
)

if not "%CODE%"=="0" (
    echo.
    echo The overlay exited with code %CODE%. The reason is in poe-trader.log:
    echo.
    type poe-trader.log
)

:end
endlocal
pause
