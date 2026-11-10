@echo off
chcp 65001 >nul
setlocal
rem Faz pastas e drives abrirem no Prime Files: duplo clique na Area de Trabalho,
rem "Abrir pasta" de outros programas, atalhos para pastas. Vale so para o seu
rem usuario (HKCU), nao pede administrador, e desfaz-padrao.bat volta tudo.
rem
rem O Windows chama "prime-files.exe <pasta>"; se o Prime Files ja estiver aberto, a pasta
rem abre numa aba nova da mesma janela.
rem
rem Nao cobre: Win+E e o botao do Explorador na barra de tarefas, que abrem o
rem explorer.exe direto, e pastas especiais (Este Computador, Lixeira).

for %%I in ("%~dp0..\..\target\release\prime-files.exe") do set "EXE=%%~fI"
if not exist "%EXE%" (
  echo Nao achei %EXE%.
  echo Rode iniciar-prime-files.bat uma vez para compilar.
  pause
  exit /b 1
)

call :registrar Directory || goto falhou
call :registrar Drive || goto falhou

echo Pronto: pastas e drives abrem no Prime Files.
echo Para voltar ao Explorador: desfaz-padrao.bat
pause
exit /b 0

:registrar
set "CHAVE=HKCU\Software\Classes\%~1\shell"
reg add "%CHAVE%\prime-files" /ve /d "Abrir no Prime Files" /f >nul || exit /b 1
reg add "%CHAVE%\prime-files" /v Icon /d "\"%EXE%\"" /f >nul || exit /b 1
reg add "%CHAVE%\prime-files\command" /ve /d "\"%EXE%\" \"%%1\"" /f >nul || exit /b 1
rem O valor padrao de "shell" escolhe o verbo do duplo clique.
reg add "%CHAVE%" /ve /d "prime-files" /f >nul || exit /b 1
exit /b 0

:falhou
echo Nao consegui gravar no registro.
echo Rode desfaz-padrao.bat para limpar o que tiver ficado.
pause
exit /b 1
