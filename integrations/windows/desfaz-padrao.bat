@echo off
chcp 65001 >nul
setlocal
rem Devolve pastas e drives ao Explorador do Windows, apagando so o que
rem tornar-padrao.bat criou.

for %%K in (Directory Drive) do (
  reg delete "HKCU\Software\Classes\%%K\shell\nohrs" /f >nul 2>nul
  for /f "tokens=3" %%V in ('reg query "HKCU\Software\Classes\%%K\shell" /ve 2^>nul ^| find "REG_SZ"') do (
    if /i "%%V"=="nohrs" reg delete "HKCU\Software\Classes\%%K\shell" /ve /f >nul
  )
)

echo Pronto: pastas e drives voltam a abrir no Explorador.
pause
