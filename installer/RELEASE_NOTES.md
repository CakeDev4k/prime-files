## Prime Files 0.1.0

Explorador de arquivos para Windows, baseado no [Nohrs](https://github.com/noh-rs/nohrs).

### Instalar

Baixe **PrimeFiles-Setup-0.1.0.exe** abaixo e execute. A instalação é só para o seu usuário e não pede administrador. O instalador baixa o que faltar:

- o **Visual C++ Runtime** da Microsoft, se o computador não tiver;
- o **7-Zip**, se você marcar a opção (para ver .rar, .7z e .iso no visualizador);
- no Windows edição "N", ativa o **Media Feature Pack**, sem o qual o programa não abre.

O instalador não é assinado digitalmente, então o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. O `SHA256SUMS.txt` traz o hash para conferir o arquivo.

Requer Windows 10 (versão 1903) ou mais novo, 64 bits. Os ícones de alguns botões usam a fonte Segoe Fluent Icons, que vem no Windows 11.

### O que tem

- Abas, divisão em dois painéis, árvore, lista, detalhes e ícones com zoom contínuo
- Visualizador de imagens, GIF, vídeo e áudio (com forma de onda), PDF, modelos 3D, texto, Markdown, pastas e arquivos compactados
- Menu do botão direito do Windows, arrastar e soltar com o Explorador e outros programas
- Busca nas subpastas (Ctrl+F) e filtro rápido
- Modo compacto no estilo do Zen Browser: abas, barra de rota, barra lateral, visualizador e barra de baixo escondidos até o mouse encostar na borda, por cima da lista ou empurrando-a
- Opção de tornar o Prime Files o explorador padrão do Windows (Configurações › Explorador)
