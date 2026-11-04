## Prime Files 0.2.0

Explorador de arquivos para Windows, rápido, escrito em Rust.

### Novidades

- **Atualização automática**: o Prime Files procura uma versão nova ao abrir, baixa em segundo plano, confere o instalador pelo `SHA256SUMS.txt` e instala quando você fecha o programa. Em Configurações › Sobre dá para procurar na hora, instalar na hora (o programa reabre sozinho) ou desligar.
- **Ícone próprio** no programa, na barra de tarefas, nos atalhos, no instalador e na lista de aplicativos instalados.
- O Windows mostra "Prime Files" como nome e versão do programa (Gerenciador de Tarefas, Propriedades).

Quem tem a 0.1.0 precisa instalar esta à mão uma vez (a 0.1.0 ainda não se atualizava); daqui para frente as versões chegam sozinhas.

### Instalar

Baixe **PrimeFiles-Setup-0.2.0.exe** abaixo e execute. A instalação é só para o seu usuário e não pede administrador. O instalador baixa o que faltar:

- o **Visual C++ Runtime** da Microsoft, se o computador não tiver;
- o **7-Zip**, se você marcar a opção (para ver .rar, .7z e .iso no visualizador);
- no Windows edição "N", ativa o **Media Feature Pack**, sem o qual o programa não abre.

O instalador não é assinado digitalmente, então o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. O `SHA256SUMS.txt` traz o hash para conferir o arquivo.

Requer Windows 10 (versão 1903) ou mais novo, 64 bits.
