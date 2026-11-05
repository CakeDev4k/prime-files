## Prime Files 0.2.2

Explorador de arquivos para Windows, rápido, escrito em Rust.

### Correções

- A forma de onda dos arquivos de áudio voltou a aparecer no visualizador.

### Correções da 0.2.1

- O Prime Files não abre mais uma janela de console (com o log) ao lado da janela do programa.
- No visualizador, o resumo das pastas e as mensagens não saem mais uma letra por linha; imagens e vídeos cabem no painel a 100%, e arrastar com o botão da roda move o que está ampliado.

### Novidades da 0.2.0

- **Atualização automática**: o Prime Files procura uma versão nova ao abrir, baixa em segundo plano, confere o instalador pelo `SHA256SUMS.txt` e instala quando você fecha o programa. Em Configurações › Sobre dá para procurar na hora, instalar na hora (o programa reabre sozinho) ou desligar.
- **Ícone próprio** no programa, na barra de tarefas, nos atalhos, no instalador e na lista de aplicativos instalados.
- O Windows mostra "Prime Files" como nome e versão do programa (Gerenciador de Tarefas, Propriedades).

Quem tem a 0.2.0 ou a 0.2.1 recebe esta sozinho. Quem tem a 0.1.0 precisa instalar esta à mão uma vez (a 0.1.0 ainda não se atualizava); daqui para frente as versões chegam sozinhas.

### Instalar

Baixe **PrimeFiles-Setup-0.2.2.exe** abaixo e execute. A instalação é só para o seu usuário e não pede administrador. O instalador baixa o que faltar:

- o **Visual C++ Runtime** da Microsoft, se o computador não tiver;
- o **7-Zip**, se você marcar a opção (para ver .rar, .7z e .iso no visualizador);
- no Windows edição "N", ativa o **Media Feature Pack**, sem o qual o programa não abre.

O instalador não é assinado digitalmente, então o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. O `SHA256SUMS.txt` traz o hash para conferir o arquivo.

Requer Windows 10 (versão 1903) ou mais novo, 64 bits.
