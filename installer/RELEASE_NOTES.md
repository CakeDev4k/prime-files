## Prime Files 0.3.5

Explorador de arquivos para Windows, rápido, escrito em Rust.

### Novidades da 0.3.5

- **Busca no Ctrl+T, como no Zen**: Ctrl+T (ou o botão "+") abre uma caixa no topo da janela. Ela lista as abas abertas, para ir direto a uma delas, e encontra qualquer arquivo ou pasta do computador pelo nome. Enter ou clique abre o resultado numa aba nova (um arquivo abre a pasta dele, já selecionado), e dá para arrastar o resultado para outro programa. Com a caixa vazia, Enter abre uma aba normal.
- **Menu do botão direito do seu jeito**: em Configurações › Menu do botão direito, escolha os ícones do topo e os itens do menu, mude a ordem com as setas ou comece por um estilo (Minimalista, Padrão, Completo). Os itens do Windows podem sair do menu de uma vez, ou um por um com o botão direito em cima deles.
- **Copiar como caminho**: ícone no topo do menu, item na lista e Ctrl+Shift+C.
- **Botão do meio abre nova aba**: numa pasta, num arquivo ou num lugar da barra lateral. A aba abre ao fundo, como num navegador (dá para mudar em Configurações › Geral). "Abrir em nova aba" também está no menu.
- **Desfazer exclusão**: Ctrl+Z, ou "Desfazer exclusão" no menu da pasta, devolve da Lixeira o que foi excluído por último.
- **Árabe** entre os idiomas, com o texto na ordem certa, da direita para a esquerda.
- **Barra lateral em cima ou embaixo, reorganizada**: os lugares ficam em colunas por grupo, e a roda do mouse rola a faixa de lado.
- **Propriedades** pode ir para o menu ou para os ícones do topo.

### Correções da 0.3.5

- Pastas criadas com `mklink` (links de pasta e junções) abriam no Explorador do Windows. Agora abrem no Prime Files como qualquer pasta.
- Com muitas abas, elas passavam por baixo dos botões de minimizar, maximizar e fechar. Agora a faixa de abas encolhe, rola e mostra sempre a aba atual.
- O volume definido no Mixer de Volume do Windows voltava a 100% a cada abertura. Agora o Prime Files lembra dele, e o volume do visualizador é o mesmo em todas as abas.
- O ícone do Explorador fixado na barra de tarefas ainda abria o Explorador com o Prime Files como explorador padrão. Agora abre o Prime Files.
- Nomes em árabe apareciam com as letras invertidas ou como quadradinhos.
- Uma linha já selecionada pode ser arrastada por qualquer coluna, não só pelo nome.

### Novidades da 0.3.3

- **Fotos nítidas no visualizador**: a imagem agora é lida do próprio arquivo, no tamanho do painel e na orientação em que a foto foi tirada. Antes vinha da miniatura guardada pelo Windows, que deixava fotos nítidas borradas e em blocos.
- **Painéis em qualquer lado**: a barra lateral, o visualizador e o terminal podem ficar à esquerda, à direita, em cima ou embaixo da lista (Configurações › Geral › Layout). Arraste a borda para mudar o tamanho; puxada até o fim, ela esconde o painel, e a borda de um painel escondido o traz de volta.
- **Escolher o que abre na borda**: puxe qualquer borda livre da lista e aparece uma bolha para cada coisa que pode abrir ali (nova divisão, barra lateral, visualizador, terminal). Deslize ao longo da borda para escolher e solte; Esc cancela.
- **Abas verticais** (opcional), numa coluna que vira só ícones quando fica estreita.
- **Tags**: marque arquivos e pastas pelo menu do botão direito (Tags). As tags aparecem na barra lateral, e um clique lista tudo que tem aquela tag; renomear, mudar a cor e excluir pelo botão direito.
- **Rede dentro do Prime Files**: Rede mostra os computadores, as unidades mapeadas e os locais adicionados; `\\servidor` lista os compartilhamentos, e um compartilhamento que pede login abre a janela de credenciais do Windows. "Adicionar local de rede" aceita caminhos `\\servidor\pasta`, `smb://` e WebDAV.
- **Mais formatos no visualizador**: Word, OpenDocument e RTF formatados; .doc, .ppt e .xls pelo leitor do próprio Windows; Photoshop (PSD/PSB); Illustrator; miniatura do Affinity; resumo de projetos do Premiere e do After Effects, com a mídia que está faltando; SVG.
- **Editar Markdown**: arquivos `.md` podem ser editados no visualizador e salvos com Ctrl+S.

### Correções da 0.3.4

- Uma aba nova podia abrir na pasta de onde o Prime Files tinha sido iniciado. Agora abre sempre na sua pasta de usuário.

### Correções da 0.3.3

- No visualizador de PDF, sobrava um vão enorme entre as páginas. Agora cada página ocupa só a própria altura.

### Novidades da 0.3.2

- **Tela de boas-vindas**: no primeiro uso (e quando você quiser, em Configurações › Sobre), quatro passos para escolher idioma e tema, deixar o Prime Files como explorador padrão, iniciar com o Windows e ligar as integrações.
- **Arquivo em destaque**: quando outro programa pede para mostrar um arquivo (Premiere em "Revelar no Explorador", o navegador em "Mostrar na pasta" depois de baixar, o menu Iniciar em "Abrir local do arquivo"), a pasta abre no Prime Files com o arquivo selecionado e piscando na cor de destaque.
- **Iniciar com o Windows**: abre minimizado ao entrar no Windows, para que esses pedidos caiam no Prime Files mesmo antes de você abri-lo.
- **"Mostrar na pasta" pode ser desligado** em Configurações › Integração com o Windows, se você preferir que esses programas abram o Explorador.
- **Espaço abre a visualização rápida**: com um arquivo ou pasta selecionado, aperte Espaço para vê-lo grande por cima da janela, no modo normal e no compacto. As setas passam para o próximo, Enter abre, e Espaço, Esc ou um clique fora fecham.
- **Botões laterais do mouse** voltam e avançam de pasta, como no Explorador e no navegador.
- **Busca em todo o computador com filtros**: `ext:wav`, `tipo:som` (também imagem, vídeo, documento, compactado, código, programa, projeto, pasta), `tamanho:>10mb` (ou `<1gb`, `10mb..1gb`, `vazio`, `grande`), `data:hoje` (ou `ontem`, `semana`, `<7d`, `>2a`, `2026-10`, `05/11/2026`), `em:.` (a pasta aberta), `em:C:\Projetos` ou `em:sfx`, e `pasta:` / `arquivo:`. Um `-` na frente exclui (`-ext:tmp`, `-backup`) e aspas buscam a frase inteira. A barra mostra o que foi entendido, quanto a busca levou e a idade do índice.
- **Resultados melhores**: nome igual e começo de nome primeiro, depois palavra dentro do nome; entre os iguais, os modificados há pouco sobem e as pastas do sistema (Windows, Arquivos de Programas, AppData) descem. Se quase nada bate, aparecem até 50 nomes parecidos, com as letras na ordem.
- **Arquivos novos aparecem na busca na hora**: o Prime Files acompanha o que é criado ou renomeado nos discos enquanto está aberto, então a busca no computador acha um arquivo salvo um segundo antes. A varredura completa passou a rodar no máximo a cada 6 horas.
- A busca ficou mais rápida (cerca de duas vezes numa tabela de 2 milhões de nomes), e a lista não pisca mais em branco enquanto você digita.
- **Indexação instantânea (opcional)**: em Configurações › Busca › Indexação instantânea › Ativar, o Prime Files instala um pequeno serviço do Windows (pede permissão de administrador uma vez) que lê a tabela dos discos NTFS como o Everything faz. O índice do computador fica pronto em segundos em vez de minutos, também depois de reiniciar, e é relido a cada 30 minutos. O serviço só lê, só responde ao seu usuário e mostra as pastas de outros usuários do PC apenas pelo nome. Desativar remove o serviço, e desinstalar o Prime Files também.
- **A primeira indexação não recomeça do zero**: ela é salva a cada 30 segundos e a cada disco terminado. Se o Prime Files fechar no meio (uma atualização, um reinício), na próxima abertura a busca já responde com o que foi indexado e a indexação continua por trás. A leitura das pastas também ficou mais rápida, com mais pastas lidas ao mesmo tempo.

- **Tela de boas-vindas mais completa**: agora em cinco passos, com a cor de destaque, um passo novo para como você vê os arquivos (itens ocultos, modo compacto, sons ao passar o mouse, forma de onda nos ícones), atualizações automáticas, indexação instantânea e um resumo de todas as escolhas.

### Correções da 0.3.2

- **A pasta aberta agora acompanha o disco**: um arquivo salvo, baixado, apagado ou renomeado por outro programa aparece (ou some) na hora, sem precisar apertar F5; tamanho e data de arquivos alterados também se atualizam. Vale também para as pastas abertas na vista Árvore.
- Arquivos de texto e Markdown (`.md`) apareciam em branco no visualizador lateral. Agora aparecem, com o Markdown formatado também quando vem de uma busca.

### Novidades da 0.3.1

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

Baixe **PrimeFiles-Setup-0.3.3.exe** abaixo e execute. A instalação é só para o seu usuário e não pede administrador. O instalador baixa o que faltar:

- o **Visual C++ Runtime** da Microsoft, se o computador não tiver;
- o **7-Zip**, se você marcar a opção (para ver .rar, .7z e .iso no visualizador);
- no Windows edição "N", ativa o **Media Feature Pack**, sem o qual o programa não abre.

O instalador não é assinado digitalmente, então o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. O `SHA256SUMS.txt` traz o hash para conferir o arquivo.

Requer Windows 10 (versão 1903) ou mais novo, 64 bits.
