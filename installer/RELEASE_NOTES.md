## Prime Files 0.3.1

Explorador de arquivos para Windows, rápido, escrito em Rust.

### Novidades

- **Atalhos configuráveis**: em Configurações › Atalhos, clique na tecla de qualquer comando e aperte a combinação nova (Esc desiste). Dá para remover um atalho, voltar um ou todos ao padrão, e um atalho que já era de outro comando sai dele com aviso. Vale na hora, e os menus mostram as teclas novas.
- **Abrir a pasta em outro programa**: botão na barra de cima (e no menu do botão direito) para abrir a pasta no Prompt de Comando, no PowerShell, no Windows Terminal (se instalado) ou no Explorador do Windows. Cada um pode ganhar um atalho.
- **Menu de zoom ao usar Ctrl + roda**: o menu com o controle deslizante aparece enquanto você muda o zoom e some sozinho logo depois.
- **Progresso do som tocado pelo mouse**: a forma de onda do arquivo vai ganhando a cor de destaque conforme o som avança, como na linha do tempo do visualizador; nas vistas sem forma de onda, uma linha embaixo do item mostra o mesmo.

### Novidades da 0.3.0

- **Busca em todo o computador**, no estilo do Everything: clique na lupa da barra de baixo (ou Ctrl+E) e digite qualquer parte do nome. Os nomes de todas as unidades fixas ficam num índice guardado no próprio computador, então a resposta vem enquanto você digita. Vários termos filtram juntos, `*` e `?` funcionam, e um termo com `\` procura no caminho. A primeira indexação de um disco grande leva alguns minutos (dá para buscar enquanto ela anda); depois o índice abre na hora e se atualiza sozinho em segundo plano, no máximo uma vez por hora, ou em Configurações › Busca › Reindexar agora.
- **Ouvir sons passando o mouse**: deixe o mouse parado sobre um arquivo de áudio na lista e ele toca; tirar o mouse para. Liga/desliga e o tempo de espera ficam em Configurações › Mídia.
- **Armazenamento das unidades**: Este Computador mostra cada unidade com uma barra do espaço usado e "livres de total" (vermelha acima de 90%), e a barra lateral mostra uma barrinha ao lado de C:, D: e das outras unidades.
- **Configurações redesenhadas**: seções com ícone e descrição, campo para buscar qualquer configuração, interruptores no lugar de "Sim/Não", botão para voltar cada opção ao padrão, e uma tela que se reorganiza quando a janela é estreita.
- **Mais idiomas**: além de português, inglês e espanhol, agora francês, alemão, italiano, russo, japonês e chinês simplificado. "Automático" segue o formato regional do Windows.
- **Terminal embutido** (Ctrl+' ou o ícone na barra de baixo), com várias abas; enquanto você não digita nada, ele acompanha a pasta aberta.
- **Forma de onda nos ícones** de arquivos de áudio (opcional), também na grade do visualizador.
- **Este Computador abre dentro do Prime Files**, e a barra de cima ganhou o botão de subir (↑, ou Alt+↑) ao lado de voltar, avançar e atualizar.
- **Tocar ao abrir no visualizador** (opcional) e integração opcional com o **Listary**.

### Correções da 0.3.0

- A roda do mouse rolava a lista por baixo das barras do modo compacto (barra lateral, visualizador) em vez da própria barra, também com a tela dividida.

### Correções da 0.2.1

- O Prime Files não abre mais uma janela de console (com o log) ao lado da janela do programa.
- No visualizador, o resumo das pastas e as mensagens não saem mais uma letra por linha; imagens e vídeos cabem no painel a 100%, e arrastar com o botão da roda move o que está ampliado.

### Novidades da 0.2.0

- **Atualização automática**: o Prime Files procura uma versão nova ao abrir, baixa em segundo plano, confere o instalador pelo `SHA256SUMS.txt` e instala quando você fecha o programa. Em Configurações › Sobre dá para procurar na hora, instalar na hora (o programa reabre sozinho) ou desligar.
- **Ícone próprio** no programa, na barra de tarefas, nos atalhos, no instalador e na lista de aplicativos instalados.
- O Windows mostra "Prime Files" como nome e versão do programa (Gerenciador de Tarefas, Propriedades).

Quem tem a 0.2.0 ou mais nova recebe esta sozinho. Quem tem a 0.1.0 precisa instalar esta à mão uma vez (a 0.1.0 ainda não se atualizava); daqui para frente as versões chegam sozinhas.

### Instalar

Baixe **PrimeFiles-Setup-0.3.1.exe** abaixo e execute. A instalação é só para o seu usuário e não pede administrador. O instalador baixa o que faltar:

- o **Visual C++ Runtime** da Microsoft, se o computador não tiver;
- o **7-Zip**, se você marcar a opção (para ver .rar, .7z e .iso no visualizador);
- no Windows edição "N", ativa o **Media Feature Pack**, sem o qual o programa não abre.

O instalador não é assinado digitalmente, então o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. O `SHA256SUMS.txt` traz o hash para conferir o arquivo.

Requer Windows 10 (versão 1903) ou mais novo, 64 bits.
