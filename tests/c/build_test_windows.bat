@echo off
rem Build + run the C FFI test harness against the built cdylib.
rem Portable: resolves vcvars via vswhere (works on local Build Tools
rem installs and GitHub Actions windows-latest runners alike).
cd /d "%~dp0..\.."
for /f "usebackq tokens=*" %%i in (`"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSDIR=%%i"
if not defined VSDIR (
    echo vswhere: no Visual Studio installation with VC tools found
    exit /b 1
)
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b 1
cl /nologo /W3 /I include tests\c\test_geo_repair.c /link target\release\geo_repair.dll.lib /OUT:target\release\gr_test.exe
if errorlevel 1 exit /b 1
target\release\gr_test.exe
exit /b %errorlevel%
