@echo off
chcp 65001 >nul
setlocal
cd /d "%~dp0Listary.FileAppPlugin.PrimeFiles"

rem Compila o plugin e copia para a pasta de plugins do Listary, que fica em
rem Program Files: por isso o Windows pede permissão de administrador na cópia.

set "DESTINO=%ProgramFiles%\Listary\FileAppPlugins\PrimeFiles"

if not exist "%ProgramFiles%\Listary\Listary.exe" (
  echo Listary nao encontrado em "%ProgramFiles%\Listary".
  pause
  exit /b 1
)

where dotnet >nul 2>nul
if errorlevel 1 (
  echo O SDK do .NET nao esta instalado: https://dotnet.microsoft.com/download
  pause
  exit /b 1
)

echo Compilando o plugin...
dotnet build -c Release --nologo -v q
if errorlevel 1 (
  echo.
  echo A compilacao falhou. A mensagem de erro esta acima.
  pause
  exit /b 1
)

echo Copiando para "%DESTINO%" (o Windows vai pedir permissao)...
powershell -NoProfile -Command ^
  "Start-Process -Verb RunAs -Wait -FilePath robocopy -ArgumentList '\"%CD%\bin\Release\net48\" \"%DESTINO%\" /MIR /NJH /NJS /NP'"

if not exist "%DESTINO%\Listary.FileAppPlugin.PrimeFiles.dll" (
  echo A copia nao aconteceu ^(permissao negada?^).
  pause
  exit /b 1
)

echo.
echo Plugin instalado. Agora:
echo   1. Feche o Listary pela bandeja e abra de novo.
echo   2. Listary ^> Opcoes ^> Integracao: confira que "Prime Files" esta ligado.
echo   3. Deixe o Prime Files aberto: o Quick Switch passa a enxergar a pasta dele.
pause
endlocal
