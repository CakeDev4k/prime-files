@echo off
chcp 65001 >nul
setlocal
cd /d "%~dp0"

rem Tema: escuro por padrao. Passe "claro" ou "sistema" para trocar,
rem por exemplo:  iniciar-nohrs.bat claro
set "TEMA=dark"
if /i "%~1"=="claro" set "TEMA=light"
if /i "%~1"=="sistema" set "TEMA=system"

where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust nao encontrado. Instale em https://rustup.rs e abra este arquivo de novo.
  pause
  exit /b 1
)

rem Release porque a build de debug fica visivelmente lenta para rolar pastas
rem grandes. A primeira vez leva alguns minutos; depois so recompila o que mudou.
echo Compilando o Nohrs (release)...
cargo build --release -p nohrs
if errorlevel 1 (
  echo.
  echo A compilacao falhou. A mensagem de erro esta acima.
  pause
  exit /b 1
)

echo Abrindo o Nohrs com tema %TEMA%...
start "" "%~dp0target\release\nohrs.exe" --theme %TEMA%
endlocal
