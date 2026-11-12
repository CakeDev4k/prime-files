# Desempenho do Prime Files (fork Windows)

Registro das passadas de desempenho no formato de
[`OTIMIZACAO.md`](../../OTIMIZACAO.md) §11. Os números são da máquina do dono
(12 núcleos lógicos, 15 GB, Premiere aberto e ~1 GB livre), salvo onde dito.

---

### 1. Listagem, ordem, miniaturas e desenho da lista (04/11/2026)

**Sintoma.** Nenhum relatado; passada pedida pelo dono para aplicar o guia de
otimização ao Prime Files: "renderização de arquivos, memória, CPU, escalável". Por
isso cada achado abaixo começa pela medida ou pela contagem, não pela queixa.

**Base.** `crates/prime-files-services/examples/listing_ab.rs` (listagem inteira,
mediana de 7 rodadas alternadas, release) e o teste `sort_timing` de
`explorer/entries.rs` (35.000 nomes, `--release --ignored`). Antes de mudar,
o A/B rodou com os dois lados iguais e deu números iguais: o instrumento não
cria diferença sozinho.

#### Uma leitura do disco por arquivo que já tinha sido lido

`list_dir_impl` chamava `fs::symlink_metadata(path)` para cada entrada. No
Windows isso abre o arquivo de novo pelo caminho; o `DirEntry::metadata` do
`read_dir` é o que o `FindNextFileW` já devolveu junto com o nome, e, como o
`symlink_metadata`, descreve o link e não o alvo. Trocado; o exemplo confere
entrada por entrada (nome, caminho, tipo, tamanho, data) que as duas listagens
são idênticas antes de medir.

| Pasta | Itens | Antes (tudo) | Depois (tudo) |
|---|---:|---:|---:|
| `C:\Windows\WinSxS\Manifests` | 35.628 | 2.628 ms | 95 ms |
| pasta temporária | 20.000 | 1.161 ms | 23 ms |
| `C:\Windows\System32` | 5.218 | 174 ms | 5,5 ms |
| `I:\Meu Drive` (Google Drive) | 69 | 18 ms | 0,9 ms |

#### Pastas acima de 1.000 itens perdiam arquivos em silêncio

`DIR_LISTING_LIMIT = 1000` e ninguém pedia a página seguinte: System32 mostrava
os primeiros 1.000 nomes de 5.218, sem aviso. Com a listagem barata, o explorador
lê a pasta inteira (`usize::MAX`). Ler 1.000 custava 38 ms em System32; ler
todos custa 5,5.

#### A leitura saiu da thread da interface (§2.12, §3.10, §5.4)

`reload()` listava na thread do gpui. Agora a leitura roda numa thread própria
e a interface espera no máximo `LISTING_WAIT` (30 ms): pasta local chega no
mesmo quadro do clique, como antes; pasta de rede, Drive ou disco acordando
segue em segundo plano. A lista anterior fica na tela por `LISTING_HOLD`
(220 ms) e só então dá lugar a "Reading…" na barra de status. Toda leitura leva
um número de geração; resposta de geração velha é descartada, e as tarefas da
leitura anterior são canceladas. Thread própria, não o executor: uma leitura
presa num compartilhamento morto não pode levar uma thread do pool junto.
`reveal` (abrir já selecionando um arquivo) espera a leitura chegar.

#### Ordem do Explorador, com chave calculada uma vez

A comparação fazia `to_lowercase()` dos dois nomes a cada comparação (~34 por
entrada em 35 mil), e a ordem não era a do Explorador (`f10` antes de `f2`,
§7.5). Primeira tentativa: `StrCmpLogicalW` no comparador — ordem certa, mas
**140 ms** contra 46 do antigo (≈0,8 µs por chamada). Ficou a chave de
ordenação do próprio Windows (`LCMapStringW` com `LCMAP_SORTKEY |
NORM_IGNORECASE | SORT_DIGITSASNUMBERS`), uma por nome, comparada byte a byte.
A pasta também era ordenada duas vezes por navegação (`entries` e depois a
lista visível); `entries` não é mais ordenada, ninguém lê a ordem dela.

| 35.000 nomes, release | Antes | `StrCmpLogicalW` | Chave |
|---|---:|---:|---:|
| ordenar por nome | 45–46 ms | 140 ms | 39–40 ms |

#### Miniaturas e ícones

- **Busca por caminho primeiro.** O cache era um `HashMap<(caminho, tipo,
  tamanho)>`, e cada tile ainda carregando varria o mapa inteiro atrás de um
  tamanho menor do mesmo arquivo, a cada quadro. Agora é caminho → poucos slots.
- **O que saiu da tela é cancelado (§5.3).** Cada quadro do painel marca o que
  pediu; o que ficou dois quadros sem ser pedido e ainda espera um worker é
  descartado (fecha o canal, o worker pula). Antes, rolar uma pasta de vídeos
  no Drive deixava os workers lentos gerando, 2–4 s cada, tiles que já tinham
  passado.
- **Teto de memória (§6.8, lei 8).** Nada era liberado enquanto se estava na
  mesma pasta: 10.000 fotos roladas de ponta a ponta no tamanho 256 seriam
  2,6 GB de RGBA, mais a cópia no atlas da GPU. Teto de 192 MB por painel; acima
  dele saem as fora da tela há mais tempo, até 3/4 do teto (histerese), com
  `drop_image` para liberar a GPU também.
- **Ícone por classe (§6.11).** Ícone de `.txt`, `.wav`, `.pdf` é o mesmo para
  todos os arquivos da extensão: o worker pede ao Shell uma vez por extensão e
  tamanho e todas as linhas dividem a mesma imagem (uma textura). Por arquivo
  continuam pastas, `.exe`, `.lnk`, `.url`, `.ico` e afins, e toda extensão
  cujo `DefaultIcon` é `%1` ou que tem `IconHandler` registrado.

#### Desenho da lista e da grade

A cada quadro (e cada miniatura que chega é um quadro) a lista refazia dois
vetores do tamanho da pasta e a grade quebrava os nomes de **todos** os itens
em linhas, alocando uma `String` por linha. Acima de 50.000 nomes o cache de
layout era limpo a cada quadro e todos os nomes eram moldados de novo. Agora:
linhas e alturas guardadas com uma chave (geração das linhas, quantidade,
colunas, larguras, rem), refeitas só quando ela muda; contar linhas não aloca;
o teto do cache de layouts acompanha a pasta (2× o número de itens) e ele é
limpo ao trocar de pasta.

#### Filtro por nome

O padrão do filtro era passado para minúsculas e dividido em caracteres uma vez
por nome, em cada tecla. Agora `NameMatcher` prepara o padrão uma vez por
filtragem (e uma vez por busca em subpastas).

**Cobertura.**
- `listing_ab` recusa medir se as listagens antiga e nova diferirem.
- `names_sort_as_strcmplogicalw_does`: cada par vizinho da ordem por chave é
  conferido com o `StrCmpLogicalW`, em nomes difíceis e nos ~41 mil nomes reais
  de System32 e WinSxS\Manifests. `names_sort_as_explorer_sorts_them`,
  `apply_order_matches_a_plain_sort_for_many_shapes`.
- `what_left_the_screen_is_cancelled_and_the_oldest_thumbnails_go_past_the_cap`,
  `icons_are_shared_only_where_the_registry_says_so`.
- `a_slow_folder_is_read_in_the_background_and_an_overtaken_read_is_dropped`.
- Mutação: sem o cancelamento de tiles fora da tela, e sem
  `SORT_DIGITSASNUMBERS`, três testes reprovam.

**Descartado.** `StrCmpLogicalW` no comparador (140 ms, acima). Trocar
`FileEntryDto.kind` (`String`) por enum e guardar a lista filtrada como índices
em vez de cópias: economizaria memória por item (§6.12), mas toca quase todo o
explorador e os testes; fica para uma passada própria, se a memória por item
aparecer numa medida.

**Não conferido.**
- Nada foi visto na tela: o Prime Files estava aberto pelo dono durante a passada e o
  `.exe` de `target\release` não pôde ser substituído. O release compilou em
  `target\ipc\release`.
- O caminho em segundo plano não foi visto com uma pasta de rede real lenta;
  o teste o exercita com `listing_wait = 0`.
- O ganho do cache de layout da grade e da lista não foi cronometrado (é
  estrutural: O(pasta) por quadro → O(1) quando nada mudou).
- O compartilhamento de ícone por classe não foi conferido visualmente contra
  o Explorador para tipos com ícone por arquivo não listados.
- A pasta aberta não é vigiada (mudança feita por outro programa só aparece
  com F5) — fora do escopo, registrado aqui.

**Lição (passada 1).** No Windows, `DirEntry::metadata` é de graça e
`fs::symlink_metadata(entry.path())` reabre o arquivo: 28× numa pasta de 35 mil.
E a comparação "lógica" do Windows custa ~0,8 µs por chamada; ordenar com ela
precisa de chave pré-calculada (`LCMapStringW` + `SORT_DIGITSASNUMBERS`).

---

### 2. Mover no mesmo disco era copiar e apagar (04/11/2026)

**Sintoma.** Nenhum relatado; saiu da leitura do código pedida pelo dono
("listar, deletar, copiar, renomear, preview"). Recortar e colar uma pasta
dentro do mesmo disco escrevia todos os bytes de novo.

**Base.** `crates/prime-files-services/examples/move_ab.rs` (release, mediana de 5
rodadas alternadas, Premiere aberto, ~700 MB livres). O lado "antes" é a
literal do que o Windows fazia: `copy_path_no_replace` + `delete_permanent`.
Rodado com os dois lados iguais antes da mudança, deu 8,1 s contra 6,1 s nos
2.000 arquivos e 1,55 contra 1,2 s no de 512 MB: **o ruído desta máquina é de
até 35%**, então só diferença de ordem de grandeza conta.

#### `rename_no_replace` devolvia `None` no Windows

`move_path_no_replace` (o que o colar de um recorte, o arrastar, os passos de
"substituir" e a restauração da lixeira usam) tenta primeiro um rename que
recusa destino ocupado. Só havia um para Linux e macOS; no Windows a função
devolvia `None` e **todo** mover caía no fallback de copiar e apagar, mesmo
com origem e destino no mesmo disco. Agora é `MoveFileExW` sem
`MOVEFILE_REPLACE_EXISTING`: o próprio sistema recusa nome ocupado
(`ERROR_ALREADY_EXISTS`), para arquivo e pasta, e outro disco responde
`ERROR_NOT_SAME_DEVICE`, que manda para a cópia como antes. Sem
`MOVEFILE_COPY_ALLOWED`, porque a cópia do Windows entre discos não reivindica
cada entrada, e é essa garantia que o fallback dá.

Cuidado que falha em silêncio: o `windows::core::Error` carrega um HRESULT
(`0x8007xxxx`, negativo como `i32`), e o `From` do próprio crate põe o HRESULT
inteiro como "erro do SO". Sem desembrulhar o código do Win32, o
`is_cross_device` nunca reconheceria o 17 e mover para outro disco viraria
falha em vez de cópia. A primeira versão desta passada errou isso
(`u32::try_from` de um `i32` negativo); o teste de códigos pegou.

| Mover no mesmo disco | Antes | Depois |
|---|---:|---:|
| C:, 2.000 arquivos de 64 KB em 20 pastas | 3.222 ms | 3,9 ms |
| C:, 1 arquivo de 512 MB | 2.307 ms | 2,5 ms |
| D:, 2.000 arquivos de 64 KB em 20 pastas | 2.160 ms | 1,8 ms |
| D:, 1 arquivo de 512 MB | 1.385 ms | 0,5 ms |
| C:, 1 arquivo de 1 KB | 3,3 ms | 2,3 ms |

O tempo antigo crescia com o tamanho; o novo, não. Além do tempo: o disco não
precisa mais de espaço para uma segunda cópia, e o arquivo movido mantém data
de modificação e criação (a cópia por `std::io::copy` gravava "agora").

**Efeito de comportamento.** O fallback recusa links no Windows
(`symlink_no_replace`), então uma pasta com link ou junção dentro
(`node_modules`, pasta de mods) não podia ser movida nem no mesmo disco. No
mesmo disco agora pode; entre discos continua recusada.

**Cobertura.**
- `a_move_within_one_drive_is_a_rename_and_not_a_copy` (data de modificação
  antiga sobrevive; `MoveKind::Rename`),
  `a_folder_holding_a_junction_moves_within_one_drive`,
  `the_no_replace_rename_reports_windows_codes_as_plain_io_errors` (183 e
  `NotFound` crus).
- Mutação: voltar a `None` reprova os três; passar
  `MOVEFILE_REPLACE_EXISTING` reprova `move_path_no_replace_refuses_an_occupied_destination`
  e o de códigos. O de pasta ocupada passa com esse mutante, e com razão: o
  Windows não substitui pasta nem com o flag.
- `move_ab` recusa medir se algum lado devolver uma árvore diferente (caminho
  relativo, tamanho, primeiros e últimos 4 KB de cada arquivo).
- 68 testes do `prime-files-services` e 163 do `prime-files-pages` verdes.

**Não conferido.**
- Nada visto na tela: o Prime Files do dono estava aberto em `target\release`; a
  build foi em `target\ipc`.
- Mover para outro disco (o caminho que continua copiando) não foi
  cronometrado, nem pasta de rede ou o Google Drive (`I:`, FAT32 virtual).
- Pasta-mãe inexistente agora é erro (como no Linux), e o fallback a criava.
  Colar e arrastar sempre têm destino existente; a restauração pelo ledger só
  roda onde não há índice da lixeira do sistema, que não é o Windows.

**Encontrado e não corrigido (fora do escopo).** No Windows,
`entry_identity`/`handle_identity` devolvem `None`, então
`discard_partial_destination` nunca apaga nada: um colar entre discos que
falha no meio deixa a cópia pela metade no destino.

**Lição.** Fallback que funciona esconde a ausência do caminho principal
(OTIMIZACAO.md lei 9): nenhum teste via que o Windows nunca renomeava, porque
copiar e apagar dá o mesmo resultado visível. O teste novo olha o que só o
caminho principal preserva (a data do arquivo).

---

### 3. Lixeira em lote (04/11/2026)

**Sintoma.** Nenhum relatado; item 3 da leitura de 04/11. Excluir uma
seleção grande levava segundos por centena de arquivos.

**Base.** `crates/prime-files-services/examples/trash_ab.rs` (release, mediana de 3
rodadas alternadas, Lixeira real, Premiere aberto). O lado "antes" é a literal
do `trash_selection` no Windows: um `ops::trash_path(item, None)` por item. A
linha de 1 arquivo é o controle: os dois lados fazem o mesmo trabalho e deram
o mesmo tempo (29,8 contra 28,4 ms).

#### Uma operação do Shell e uma thread por arquivo

Cada `trash_path` abre uma thread própria (por causa do pânico de COM da
rodada 20), entra num apartamento COM, cria um `IFileOperation`, executa e
fecha. Agora `ops::trash_paths` entrega a seleção inteira ao
`trash::delete_all`: uma thread, um `IFileOperation`.

Os cuidados, porque o `delete_all` do crate é tudo-ou-nada no que **diz**, não
no que **faz**:
- um caminho que não abre (apagado por outro programa) derruba o lote inteiro
  antes de começar, então caminho que já não existe fica fora do lote e é
  tentado sozinho, para sair com a mensagem do sistema como antes;
- se um item falha no meio, os outros vão e o crate só diz "algumas operações
  foram abortadas"; depois de um lote que falhou, o que **ainda está no disco**
  é tentado um a um, o que dá nome e motivo à falha. O que o lote já levou não
  é tentado de novo.
- Onde o ledger é obrigatório (macOS), continua item por item, porque cada
  item é capturado e registrado antes de sair.

| Para a Lixeira | Antes | Depois |
|---|---:|---:|
| 1 arquivo (controle) | 29,8 ms | 28,4 ms |
| 10 arquivos | 300 ms | 103 ms |
| 200 arquivos | 7,0 s | 1,9 s |
| 1.000 arquivos | 35,8 s | 9,9 s |

3,6× nas seleções grandes. O que sobra, ~10 ms por item, é trabalho da
própria Lixeira do Windows (os arquivos `$I`/`$R` de cada item) e não mudou.

**Cobertura.**
- O plano é função pura (`trash_in_one_batch`) com as chamadas do sistema
  trocadas por contadores: `a_selection_goes_to_the_trash_in_one_call` (1
  chamada de lote, 0 avulsas),
  `a_failed_batch_retries_only_what_is_left_and_names_the_file_that_failed`,
  `a_path_already_gone_stays_out_of_the_batch_and_is_still_reported`,
  `nothing_on_disk_makes_no_batch_call`. E um real:
  `trash_paths_moves_real_files_and_reports_the_missing_one` (deixa 4 itens na
  Lixeira a cada execução, como o teste de COM já deixava).
- Mutação: trocar o lote por laço reprova três; tentar de novo também o que o
  lote já levou reprova um.
- O fio entre o explorador e `trash_paths` (uma linha em `file_ops.rs`) não
  tem teste próprio; está provado pelo benchmark, que chama a mesma função.
- `trash_ab` confere que todos os arquivos saíram e que nenhum lado relatou
  falha antes de aceitar o tempo, e no fim tira da Lixeira só os itens cuja
  origem é a pasta da rodada (7.266 = 3 × 1.211 × 2).

**Não conferido.**
- Não visto na tela (o Prime Files do dono aberto em `target\release`).
- Falha real no meio do lote (arquivo em uso) só no teste com contadores; não
  provocada contra o Shell.
- O `noh rm` da linha de comando continua item por item (`prime-files-cli/src/rm.rs`).
- Excluir permanentemente (Shift+Delete) não foi tocado: é `remove_dir_all`
  por item, sem Shell.

---

### 4. Colar com `CopyFileExW` (04/11/2026)

**Sintoma.** Nenhum relatado; item 2 da leitura de 04/11. Colar uma pasta de
fotos apagava a data de todas: cada cópia saía com a hora do colar.

**Base.** `crates/prime-files-services/examples/copy_ab.rs` (release, mediana de 5
rodadas alternadas, origem no C:, Premiere aberto). O lado "antes" é a literal
do `copy_path_no_replace` anterior: `create_new` + `std::io::copy` +
`set_permissions`. Com os dois lados iguais, antes da mudança, o ruído chegou a
**2,4×** (2.000 arquivos para o D:, 7,2 contra 3,0 s) e a 5% no arquivo grande
do C:.

#### A cópia sem substituir não usava a cópia do sistema

O `copy_path` (que substitui) já usava `fs::copy`, que no Windows é
`CopyFileExW`. O que o colar usa, sem substituir, reivindicava o nome com
`create_new` e copiava com `std::io::copy`: os bytes passavam pelo programa em
blocos, a data de modificação virava "agora" e as streams alternativas ficavam
para trás. Agora, no Windows, é `CopyFileExW` com `COPY_FILE_FAIL_IF_EXISTS`,
que cria o destino com `CREATE_NEW` dentro da própria chamada: é a mesma
reivindicação, e nada que pegou o nome antes é sobrescrito.

O que muda no relato de falha (`ClaimFailure`): "nome ocupado" volta como
`Nothing`, porque é respondido antes de qualquer escrita. Outra falha volta como
`Nothing` se não há nada no destino (o `CopyFileExW` apaga o que escreveu ao
desistir), e como `PartialDestination` sem identidade se há algo lá: a limpeza
de quem chamou não apaga nada (como já era no Windows) e o relato diz onde está.

| Colar (copiar sem substituir) | Antes | Depois | 2ª rodada: antes | depois |
|---|---:|---:|---:|---:|
| 2.000 × 64 KB, C: → C: | 2.429 ms | 1.521 ms | 2.776 ms | 1.704 ms |
| 2.000 × 64 KB, C: → D: | 1.755 ms | 861 ms | 2.776 ms | 1.301 ms |
| 512 MB, C: → C: | 835 ms | 252 ms | 721 ms | 180 ms |
| 512 MB, C: → D: | 1.316 ms | 1.026 ms | 1.327 ms | 1.424 ms |
| data de modificação mantida | não | **sim** | não | **sim** |

O que os números sustentam: o arquivo grande no mesmo disco ficou ~4× mais
rápido (acima do ruído de 5% desse caso); os arquivos pequenos ficaram 1,6–2×
mais rápidos nas duas rodadas e nos dois destinos, mas o ruído desse caso é do
mesmo tamanho; o arquivo grande para o D: não mudou (o disco é o limite). O
resultado firme é a data.

**Cobertura.**
- `a_pasted_copy_keeps_the_modification_time` (arquivo solto e dentro de
  pasta), `a_pasted_copy_keeps_a_read_only_file_read_only`,
  `a_source_another_program_holds_leaves_nothing_at_the_destination` (origem
  aberta sem compartilhamento: `Nothing`, destino ausente, código 32 cru).
- Mutação: voltar ao `io::copy` reprova os dois de atributo; tirar o
  `FAIL_IF_EXISTS` reprova `the_fallback_move_claims_the_destination_before_writing_to_it`;
  marcar toda falha como parcial reprova o da origem travada.
- `copy_ab` recusa medir se a cópia de algum lado diferir da origem.

**Não conferido.**
- Não visto na tela (o Prime Files do dono aberto em `target\release`).
- Pasta de rede (onde o `CopyFileExW` deixa o servidor copiar) e o Google
  Drive não foram medidos.
- Falha no meio de uma cópia grande (disco cheio) não foi provocada; o ramo
  `PartialDestination` não tem teste no Windows.
- Ainda sem progresso nem cancelamento na interface: o `CopyFileExW` aceita
  rotina de progresso e sinal de cancelar, e é por aí que a barra "só aparece
  quando demora" (OTIMIZACAO.md §3.10) entraria.
- Pastas copiadas continuam com a data do colar (só arquivos mantêm).

---

### 5. Recarregar não é reabrir (04/11/2026)

**Sintoma.** Nenhum relatado; item 4 da leitura de 04/11. Excluir, colar,
renomear ou soltar um arquivo relia a pasta como se fosse a primeira visita:
fechava todas as pastas abertas na vista Árvore, fechava a pré-visualização de
texto e de imagem (mas não a de vídeo, PDF e 3D, que ficavam mostrando um
arquivo que podia nem existir mais), jogava fora os nomes já moldados da grade
e, numa pasta lenta, trocava as linhas por "Reading…" depois de 220 ms.

**Base.** Estrutural, por leitura de `navigation.rs` (`reload`,
`apply_listing`) e dos seis lugares que chamam `reload` depois de uma operação
(`file_ops.rs`, `drag.rs`, `context_menu.rs`). Nada cronometrado.

#### O `reload` passa a saber se é a mesma pasta

Em vez de mudar cada um dos seis chamadores, o painel guarda de que pasta vêm
as linhas na tela (`listed_cwd`). Uma leitura da mesma pasta é **refresh**
(OTIMIZACAO.md §3.9, o `RefreshDirectory` do Salamander):
- as pastas abertas na Árvore continuam abertas e são **relidas junto**, na
  mesma thread da listagem, para que um arquivo excluído dentro delas saia da
  tela; pasta aberta que sumiu fecha;
- o visualizador fica, a menos que o arquivo dele tenha sumido — e então fecha
  **inteiro**, inclusive vídeo, PDF e modelo (`clear_preview`, agora a única
  lista do que compõe o visualizador, usada também por `load_preview`). Saber
  se o arquivo sumiu é uma ida ao disco, então é perguntado na thread da
  leitura, não na da interface (§2.12);
- os nomes moldados da grade ficam (a chave é o nome);
- numa pasta lenta, as linhas velhas ficam até as novas chegarem.

Entrar em outra pasta continua como antes. Uma listagem que falha zera
`listed_cwd`, para a próxima leitura ser visita.

E o renomear: a seleção é por caminho, e o caminho muda; a linha renomeada
saía da seleção. Agora seleção, âncora e linha ativa seguem o nome novo, como
no Explorador.

**Cobertura.**
- `a_refresh_keeps_tree_folders_open_and_reads_them_again` (arquivo excluído
  dentro da pasta aberta sai; pasta aberta excluída fecha),
  `a_visit_to_another_folder_starts_with_the_tree_closed`,
  `a_refresh_keeps_the_preview_while_its_file_is_there`,
  `a_renamed_row_stays_selected`.
- Mutação: tratar todo `reload` como visita reprova dois; manter as pastas
  abertas sem relê-las reprova um; tirar o ajuste da seleção reprova um.
- 167 testes do `prime-files-pages` verdes.

**Não conferido.**
- Não visto na tela (o Prime Files do dono aberto em `target\release`).
- Ganho de tempo não medido: deixar de moldar de novo os nomes de uma grade
  grande a cada exclusão é estrutural.
- Renomear o próprio arquivo do visualizador fecha o visualizador (o caminho
  antigo sumiu); o Explorador mostraria o arquivo com o nome novo.
- O refresh ainda relê a pasta inteira. Corrigir a lista na memória para uma
  exclusão ou renomeação de um item só (sem reler 35 mil) é a próxima peça,
  e ainda não foi feita.

---

### 6. O visualizador segue as setas (04/11/2026)

**Sintoma.** Pedido do dono depois da leitura de 04/11. A leitura dizia que
"cada seta lê o arquivo inteiro" — **errado**: as setas (`move_active`) só
moviam a seleção, e o visualizador ficava no último item clicado. O código
mostrou isso antes de qualquer mudança (OTIMIZACAO.md §1.2: leitura diz o que
deveria acontecer). Seguir as setas, como o painel de visualização do
Explorador, foi decisão do dono; e é ao segui-las que o atraso e o
cancelamento passam a ser necessários.

**Base.** Nada a medir antes: o visualizador não seguia as setas.

#### Seguir a linha ativa, só quando ela para

`move_active_following` move a linha e chama `follow_active_in_preview`, que
espera `PREVIEW_FOLLOW_DELAY` (80 ms) antes de abrir o visualizador da linha
ativa. Cada tecla substitui a espera anterior (o `Task` guardado em
`preview_follow`), então segurar a seta para baixo lê só o arquivo onde ela
para (§5.4). 80 ms é pouco mais que a repetição do teclado (~30 ms) e fica
abaixo dos 100 ms em que o visualizador pareceria atrasado (§3.10).

Os cuidados:
- **o clique ganha**: quando a espera termina, ela só abre se a linha ativa
  ainda é a dela; um clique no meio já abriu o próprio visualizador;
- **visualizador escondido não lê nada**;
- só pastas e arquivos, como o clique (`row.rs`);
- **o carregamento do visualizador não é mais solto** (`detach`): fica em
  `preview_load`, e o seguinte o substitui, o que descarta a leitura de um
  arquivo que ainda não começou a ser lida. Uma leitura já em andamento
  (listar pasta, `7z.exe l`) não pode ser interrompida; termina e é
  descartada pela conferência de caminho que já existia. É o atraso que
  impede de começar leituras à toa.

**Cobertura.**
- `the_preview_follows_the_arrow_keys_once_they_stop` (com o relógio de teste:
  nada lido durante as teclas, cada tecla recomeça a espera, a última é lida),
  `a_hidden_preview_does_not_follow_the_arrow_keys`,
  `a_click_during_the_wait_is_not_undone_by_it`.
- Mutação: abrir na hora reprova o primeiro; não conferir a linha ativa
  reprova o do clique; ignorar o visualizador escondido reprova o dele.
- 170 testes do `prime-files-pages` verdes.
- **Visto na tela**, numa segunda cópia com `LOCALAPPDATA` próprio (setas por
  `PostMessage`, captura por `PrintWindow`), sem tocar a janela do dono: uma
  seta → o visualizador mostra `b.txt`; duas setas seguidas → 30 ms depois a
  seleção está em `c.txt` e o visualizador ainda em `b.txt`; 600 ms depois,
  `c.txt`.

**Não conferido.**
- Pasta grande com arquivos pesados (PDF, vídeo, `.rar`) segurando a seta: o
  ganho de não abrir 7-Zip ou o player a cada linha é estrutural, não
  cronometrado.
- A lista antiga do gpui-component (`ListEvent::Select` em `list_setup.rs`)
  continua abrindo na hora; não é a lista desenhada hoje.

---

### 7. Imagem ajustada vem do Shell, não da decodificação inteira (04/11/2026)

**Sintoma.** Nenhum relatado; item 10 da leitura de 04/11. O visualizador
desenhava imagens com `img(PathBuf)`, que decodifica o arquivo inteiro em
resolução cheia, mesmo quando o painel mostra umas centenas de pixels dele.

**Base.** Roteiro `measure-image.ps1` (no scratchpad da sessão): uma cópia do
Prime Files com `LOCALAPPDATA` próprio abre uma pasta com uma foto de 8000×6000
(48 MP, JPEG de 0,9 MB), um clique por `PostMessage` na linha da foto, e
capturas por `PrintWindow` a cada ~50 ms até a foto aparecer no painel; a
memória é a `PrivateMemorySize64` do processo. Release, Premiere aberto, 3
rodadas alternadas antes/depois.

| Foto de 48 MP no visualizador | Antes | Depois |
|---|---:|---:|
| Memória privada ao abrir | 122 → **677 MB** (+555) | 122 → **124 MB** (+2) |
| Até a imagem aparecer | 336 / 376 / 405 ms | 75 / 105 / 108 ms |
| Depois de zoom acima de 100% | 493 MB | 676 MB |

+555 MB é quase o triplo dos 192 MB da imagem em RGBA (cópias do
decodificador, a conversão para BGRA e a do atlas da GPU), e ficava enquanto a
foto estivesse no visualizador. A linha do zoom não é regressão: acima de
100% a imagem é decodificada inteira de propósito, nos dois lados.

#### O que mudou

No ajuste (zoom ≤ 100%) a imagem é a miniatura do Shell no tamanho do painel
(`fitted_picture` em `view/preview.rs`): o lado maior do painel em pixels de
tela, arredondado para 512, 1024 ou 2048 (`fitted_edge`), pelo mesmo cache e
pelas mesmas filas das miniaturas da grade, que já cancelam o que sai da tela
e respeitam o teto de memória (passada 1). Enquanto ela é feita, a miniatura
menor do mesmo arquivo (a da grade) aparece no lugar; sem nenhuma, um quadro
vazio, e não a decodificação inteira, que custaria a memória que isto evita
para ser trocada logo depois.

A decodificação inteira continua onde ela é o que se precisa:
- zoom acima de 100% — e, ao voltar para o ajuste, ela é solta
  (`gif_cache = None`);
- GIF animado (a miniatura do Shell é um quadro só);
- formato de que o Shell não tem miniatura (um codec que o Windows não tem).

Para saber esse último caso, a cache de miniaturas passou a guardar se a
resposta é a imagem de verdade ou o ícone do tipo no lugar dela (`settled`):
depois das novas tentativas o ícone fica `Ready` para sempre, e `retry_at` não
diz mais nada. `Thumbnails::picture` responde `Shown`, `Waiting` ou
`Unavailable`, e o ícone nunca aparece como se fosse a foto.

**Cobertura.**
- `a_fitted_image_is_not_decoded_whole` (ajustada: miniatura; zoom: arquivo;
  GIF: arquivo), `a_fitted_image_is_asked_for_in_steps_that_cover_the_pane`,
  `the_preview_takes_the_picture_and_never_the_stand_in`.
- Mutação: sempre decodificar inteira e ignorar o zoom reprovam o primeiro;
  aceitar o ícone como foto e mostrar qualquer tamanho menor enquanto espera
  reprovam o terceiro.
- 173 testes do `prime-files-pages` verdes.
- **Visto na tela** (capturas da cópia isolada): a foto ajustada com o mesmo
  enquadramento de antes, e o zoom a 173% pela decodificação inteira.

**Não conferido.**
- Foto real (a de teste é de faixas de cor): nitidez da miniatura do Shell e a
  orientação EXIF, que o Shell aplica e a decodificação inteira do gpui talvez
  não.
- PNG com transparência: se a miniatura do Shell mantém o alfa no visualizador.
- `.webp` sem a extensão de imagem da Microsoft: o caminho `Unavailable` →
  decodificação inteira está testado com contadores, não com o arquivo real.
- Painel maior que 2048 pixels de tela (4K com escala alta): a imagem
  ajustada sai de 2048 e fica um pouco mais macia.
- A memória depois de voltar do zoom para o ajuste não foi medida.

**Lição.** No caminho de uma ação que se repete (clicar de foto em foto), o
custo de memória vem do tamanho do arquivo, não do que a tela mostra; pedir o
tamanho mostrado (OTIMIZACAO.md §3.5) vale também para o visualizador, e não
só para a grade.

---

### 8. Os itens menores da leitura de 04/11 (04/11/2026)

Seis mudanças pequenas, cada uma com teste e mutante próprios. Nenhuma vista
na tela.

#### Renomear só a caixa numerava o arquivo (defeito)

`foto.jpg` → `Foto.jpg` saía `Foto (1).jpg`: no NTFS `Foto.jpg` está ocupado —
pelo próprio arquivo — e `would_conflict` dizia que sim. Agora
`ops::would_collide_with_another(src, dst)` só conta colisão com **outra**
entrada: no Windows as duas são comparadas por `canonicalize`, que devolve o
caminho final com a caixa gravada; um link em `dst` conta como colisão mesmo
apontando para `src`, porque renomear por cima o destruiria.
Testes: `a_rename_that_only_changes_case_collides_with_nothing` (ops) e
`a_rename_that_only_changes_case_is_not_numbered` (explorador).

#### Renomear fora da thread da interface (§2.12)

`commit_rename` perguntava ao disco na thread do gpui três vezes (o nome está
ocupado? qual número está livre? e o próprio `rename`): num compartilhamento
que parou de responder, a janela congelava. Agora roda em `background_spawn`, e
`finish_rename` aplica a resposta (seleção segue o nome novo, refresh,
mensagem). Teste: `a_rename_runs_off_the_ui_thread` (o executor de teste só roda
trabalho de fundo quando deixado: logo depois do `commit_rename` o arquivo
ainda tem o nome antigo). `rename_to_invalid_name_reports_error` ganhou um
`run_until_parked` antes de ler a mensagem — a conferência é a mesma; a
resposta é que passou a chegar depois.

#### A listagem não ordena o que vai ser ordenado de novo

`list_dir_impl` ordenava a pasta inteira por `to_lowercase`, e o explorador e a
Árvore ordenam de novo pela ordem escolhida. Medido com
`examples/listing_sort_cost.rs` (release, 3 rodadas): WinSxS\Manifests, 35.628
itens, **8,7–10,4 ms de 82–101 ms** da listagem; System32, 0,5 de 4,8 ms.
Agora só ordena quando há página (`offset > 0 || limit < total`), que é quando
a ordem é contrato: sem ela o cursor da página seguinte não continuaria de
onde a anterior parou. O teste `lists_files_and_dirs_sorted_case_insensitively`
virou `lists_files_and_dirs` (o contrato mudou, de propósito) e
`pages_are_sorted_case_insensitively` guarda o que ficou. O `listing_ab` da
passada 1 compara as listagens sem olhar a ordem.

Armadilha que o mutante pegou: a primeira versão do teste de páginas passava
**sem** a ordenação, porque o NTFS já devolve os nomes na ordem dele, sem
caixa. Só com um `_` no nome (0x5F: depois das letras para o NTFS, que compara
em maiúsculas; antes delas para o `to_lowercase`) as duas ordens divergem e o
mutante reprova.

#### Miniatura nova quando o arquivo muda (§6.1)

A cache de miniaturas é por caminho e só esvaziava ao trocar de pasta: um
arquivo sobrescrito por "colar e substituir" ou salvo por outro programa
mantinha a imagem velha. No refresh (passada 5), cada item cujo tamanho ou
data mudou desde a leitura anterior é esquecido (`Thumbnails::forget`) e
pedido de novo; os outros mantêm a sua. Miniatura solta a textura; ícone só
perde o lugar, porque um ícone pode ser uma textura dividida pelo tipo inteiro.
Teste: `a_refresh_forgets_the_picture_of_a_file_that_changed`. Não pega:
sobrescrita no mesmo segundo com o mesmo tamanho (a data da listagem é em
segundos), nem mudança sem refresh (a pasta não é vigiada).

#### O visualizador não lê 2 MB de binário

Arquivo desconhecido era lido inteiro (até 2 MB) para o `from_utf8` falhar;
num drive na nuvem, isso é baixar o arquivo. Agora `read_unless_binary` lê os
primeiros 8 KB e para num byte zero, que nenhum texto UTF-8 mostrável tem.
Texto em UTF-16 já caía em "não disponível" antes e continua. Teste:
`a_binary_file_is_not_read_past_its_first_bytes` (inclusive texto maior que os
8 KB chegando inteiro).

#### A pasta no visualizador na ordem da lista (§7.5)

`read_folder` comparava com `to_lowercase` dos dois nomes a cada comparação e
punha `f10` antes de `f2`, ao contrário da lista principal. Agora usa a mesma
chave (`entries::SortName`, o `LCMapStringW` + `SORT_DIGITSASNUMBERS` da
passada 1), uma vez por nome; os arquivos compactados também. Teste:
`a_folder_in_the_preview_is_in_the_listing_order`.

**Cobertura.** Mutação, um por item: comparar colisão sem olhar a entrada,
renomear na thread da interface, nunca ordenar páginas, não esquecer a
miniatura, não farejar binário, voltar ao `to_lowercase` — cada um reprova o
seu teste (o de renomear na thread também reprova
`rename_collision_falls_back_to_numbering`). 78 testes do `prime-files-services` e
178 do `prime-files-pages` verdes.

**Não conferido.** Nada visto na tela. A vantagem de renomear em segundo plano
num compartilhamento travado não foi provocada; o renomear de caixa não foi
testado em FAT32/exFAT nem no Google Drive (`I:`).

---

### 9. A busca do Everything a cada tecla (07/11/2026)

**Sintoma.** Nenhum relatado; saiu da leitura de 07/11, pedida pelo dono
("procure pontos que falha na otimização do Prime Files"). A busca global
(`explorer/everything.rs`, a do Ctrl+T e a da barra) entrou em 05/11 e nenhuma
passada a tinha medido. Três itens: ordenar todos os acertos para ler 5.000,
montar um caminho por entrada nos filtros de caminho e a busca aproximada
alocando por nome.

**Base.** `explorer/everything_pass9.rs`, que guarda o código de antes
**literal** (`old_gather`, `old_search`, `old_rank`, `old_subsequence`; só os
nomes mudaram) para o benchmark rodar o código antigo e não uma reescrita
(OTIMIZACAO.md §1.3):
- `pass_9_speed`: a busca inteira, 18 consultas (larga, estreita, cada filtro,
  termos de caminho, `em:`, sigma final, a aproximada), mediana de 5 rodadas
  alternadas, release, num índice sintético de 2,5 milhões de nomes com a
  profundidade de um disco real (`C:\Pasta\Users\editor\Documents\Projetos
  2026\subNN\arquivo`). Recusa medir se os dois lados derem resultados
  diferentes. `PRIME_FILES_PASS9_INDEX=<everything.idx>` roda no índice salvo; não
  rodou: o do dono tem 346 MB e a máquina tinha 0,4–3,6 GB livres.
- `pass_9_parts_speed`: os itens 2 e 3 sozinhos, numa thread.

Controle A/A antes de mudar (os dois lados com o código antigo): mesmos tempos,
com até ~20% de ruído numa linha (`em:` 42 contra 51 ms). Entre rodadas o
estado da máquina mudou o "antes" da mesma consulta de 155 para 43 ms (o
Premiere e outra sessão disputando memória), então só a comparação **dentro**
de uma rodada vale.

#### Ordenar só o que vai ser lido

`gather` ordenava todos os acertos e lia os 5.000 primeiros (a paleta mostra
60). Uma letra casa quase todo nome do disco. Agora `in_order_while` separa os
primeiros com `select_nth_unstable` (linear) e ordena só esse bloco; se a
deduplicação (o mesmo arquivo na tabela do disco e na do vigia) pedir mais, o
bloco seguinte, com o dobro do tamanho, sai do que sobrou. Cada acerto é único
(a fonte e a posição fazem parte dele), então a ordem é exatamente a de
ordenar tudo.

Armadilha que o benchmark pegou: no passo da busca aproximada, a primeira
versão passava um `keep` que sempre dizia "continue", e o laço acabava
ordenando tudo em blocos dobrados: `rendr` ficou **mais lento** (25 → 35 ms)
nas quatro rodadas até a correção. Nenhum teste de igualdade veria isso; o
resultado era o mesmo.

#### Caminho do pai montado uma vez (§7.6)

Termo de caminho (`em:`, `pasta\`, `-node_modules\`) montava o caminho de cada
entrada cujo nome passou: subir pelos pais, juntar, passar para minúsculas,
duas alocações por entrada. Sem palavra de nome ao lado, isso vale para todas.
Agora cada worker guarda o caminho do último pai (`LowerPaths`): a varredura
adiciona os filhos de uma pasta juntos, então o próximo nome quase sempre tem o
mesmo pai, e o caminho dele é o do pai, um `\` e o nome, num buffer
reaproveitado.

Minúsculas parte por parte dão o mesmo texto que minúsculas do caminho inteiro:
a única regra do `to_lowercase` que olha os vizinhos (o sigma final) não
atravessa o `\`. Há uma consulta com sigma no fim de pasta (`σας\ kick`) entre
as de igualdade.

Este item quase foi descartado por uma medida errada: a primeira consulta de
caminho do benchmark não achava nada (`projeto\sub0`, numa árvore onde a pasta
se chamava `Projetos 2026`) e media 15 ms dos dois lados. Uma consulta que
**acha** (`2026\sub0`, 500 mil nomes) mostrou o custo real.

#### Busca aproximada sem copiar o nome

`subsequence` roda em todo nome que a passada aproximada olha (quando a exata
dá menos de 20) e copiava o termo e o nome em `Vec<char>`. Agora percorre os
dois no lugar, com iteradores.

| Isolado, uma thread, 2,5 milhões de nomes | Antes | Depois |
|---|---:|---:|
| caminho em minúsculas de cada nome (item 2) | 495 ms | 19 ms |
| teste aproximado de `rendr` em cada nome (item 3) | 470 ms | 57 ms |

| Busca inteira, mesma rodada, 2,5 milhões | Antes | Depois |
|---|---:|---:|
| `a` (casa quase tudo) | 142 ms | 63 ms |
| `ext:wav` | 46 ms | 25 ms |
| `ext:wav -kick` | 45 ms | 26 ms |
| `2026\sub0` (só caminho) | 44 ms | 26 ms |
| `rendr` (exata vazia, entra a aproximada) | 21 ms | 15 ms |
| `-sub1\ glow` | 21 ms | 14 ms |
| `re` / `render` | 22 ms | 14 ms |
| `bonk render`, `kick 0001`, `em:… wav` | 5–11 ms | iguais |

O que os números sustentam: as consultas largas e as de caminho ficaram ~2×
mais rápidas dentro de cada rodada, em todas as rodadas; as estreitas, que já
eram baratas, não mudaram. Na busca inteira os itens 2 e 3 aparecem diluídos:
são 12 threads, e o filtro de letras (`mask`) barra a maioria dos nomes antes
deles.

**Cobertura.**
- `the_search_finds_what_it_found_before_pass_9`: as 18 consultas, código novo
  contra o antigo literal, resultado e ordem idênticos; recusa consulta que
  não acha nada (três delas não achavam e foram trocadas).
- `the_hits_come_in_the_order_of_sorting_them_all` (tamanhos de 0 a 20 mil,
  paradas sorteadas), `reading_the_first_hits_does_not_sort_them_all` (conta
  as comparações: ler 5.000 de um milhão fica abaixo de 6 milhões; ordenar tudo
  são ~20 milhões).
- `a_folder_path_is_built_once_for_all_its_children` (cada caminho igual ao
  antigo, e no máximo dois pais montados por pasta).
- `the_near_miss_test_answers_as_before`: 200 mil pares sorteados (acentos,
  sigma, separadores), nova contra antiga.
- Mutantes: ver abaixo.

**Não conferido.**
- O índice real do dono (346 MB) não rodou: faltava memória livre.
- Nada visto na tela; a busca é a mesma função, mas a tecla no Ctrl+T não foi
  cronometrada.
- O desvio do `keep` da aproximada não tem teste que o reprove; só o benchmark
  o vê.
- A varredura em si (os ~60 ms que sobram em `a`) e o "digitar estreita"
  (§7.2) ficaram de fora: a busca aproximada não é monotônica, e o estreitamento
  precisa da mesma exceção do PrimeSearch.

**Lição.** Consulta de benchmark que não acha nada mede o caminho barato: aqui
ela quase descartou um ganho de 26×. A §1.3 já diz "rápido porque não fez
nada"; vale também para "barato porque não casou".

---

### 10. Abrir o app: um dispositivo Direct3D e a lista de fontes do cache (07/11/2026)

**Sintoma.** Pedido do dono: "otimizar o tempo pra abrir a tela do Prime
Files", com base no OTIMIZACAO.md, no Open Salamander e no EdenExplorer.

**Base.** A linha do tempo das aberturas reais, tirada do log do próprio app
(`~/.local/state/prime-files/logs`; t0 = "interface language detected"), em cinco
aberturas de 07–08/11: GPU escolhida em 11–28 ms, "Created device" em
260–470 ms, "Use Segoe UI" em 420–810 ms, primeira `fs.list_dir` em
490–1.290 ms. Os dois maiores pedaços antes das fontes foram cronometrados em
sondas (C++, no scratchpad da sessão) que fazem as mesmas chamadas do gpui num
processo novo, sem janela, 5 rodadas alternadas, Premiere fechado.

#### O gpui criava o dispositivo Direct3D duas vezes

`get_adapter` criava um dispositivo D3D11 completo em cada placa só para ver
se dava, soltava, e `DirectXDevices::new` criava de novo. Agora o dispositivo
feito ao escolher a placa é o que fica (`third_party/gpui/.../directx_devices.rs`,
marcado `PRIME FILES`). A placa que falha continua passando para a próxima, como
antes.

#### O DirectWrite conferia fontes novas a cada abertura

`GetSystemFontCollection(.., checkForUpdates = true)` faz o DirectWrite
procurar fontes instaladas desde a última leitura do cache. A abertura agora
pede `false`; `update_system_font_collection`, chamada quando uma fonte não é
achada, continua pedindo `true`.

| Sonda (mediana de 5, processo novo) | Antes | Depois |
|---|---:|---:|
| Dispositivo D3D11 na RTX 5050 | 317 ms | 181 ms |
| Lista de fontes do sistema (1.530 instaladas) | 134 ms | 1,8 ms |

Mesmas 388 famílias de fontes nos dois casos.

#### Marcas da abertura no log

`prime_files_pages::startup` grava, no alvo `prime_files::startup`, os milissegundos
desde a **criação do processo** (`GetProcessTimes`, não o `main`: o que o
carregador faz antes do `main` também é espera) em cada etapa: `main`,
`logging`, `config`, `gpui` (Direct3D e DirectWrite prontos), `services`,
`window`, `first-render`, `first-frame` (o quadro seguinte ao primeiro
desenho) e `first-listing`. As marcas feitas antes de o log existir ficam
guardadas e saem quando ele sobe. É a base que faltava para a próxima
passada: hoje ninguém sabe o que acontece antes do t0 nem nos 30–470 ms entre
as fontes e a primeira lista.

**Descartado.** O filtro por letra recalcula a chave de ordenação
(`LCMapStringW`) de cada linha visível em `sort_entries_with`. A leitura de
07/11 estimou 35–40 ms por letra numa pasta de 35 mil, tirando o número do
`sort_timing` da passada 1 — que mede a ordenação inteira, não só as chaves.
Medido (35 mil linhas já em ordem, mediana de 7 alternadas, 3 rodadas): fazer
as chaves 5,2–6,1 ms; um cache de chave por nome, 3,4–3,7 ms (o `HashMap` de
`String` custa quase o mesmo que o `LCMapStringW`). ~2 ms por letra não se
sentem e o cache custava memória por nome; a mudança foi desfeita. Se o filtro
de uma pasta grande parecer lento, o custo está em outro lugar (as cópias de
`FileEntryDto` em `apply_filter`, o desenho), e é lá que se mede.

Também medidos e sem custo de abertura: o syntect que cada aba
carrega (`SyntaxService::new`, 1–2 ms; o campo nem é usado), abrir o índice
tantivy e o escritor de 50 MB (1,2–1,5 ms, numa cópia do índice), o tamanho
do exe (38 MB nasce em ~40 ms, um exe mínimo em ~23).

**Cobertura.** Nenhum teste: é inicialização da plataforma, e os testes do
gpui não criam dispositivo. A prova é o log da próxima abertura: "Created
device" e "Use Segoe UI" devem chegar ~265 ms mais cedo.

**Não conferido.**
- O app não foi aberto com a mudança (o dono pediu para não testar na tela
  dele); os números são das sondas, não do app.
- Máquina com duas placas onde a primeira falha: o caminho de passar para a
  próxima não foi exercitado.
- Fonte instalada com o app aberto: depende de `update_system_font_collection`
  ser chamada; não provado.
- O que vem antes do t0 (processo, log, config) e os 30–470 ms entre as fontes
  e a primeira lista continuam sem medida.

### 11. Memória: o índice do Everything ficava para sempre (09/11/2026)

**Queixa.** "O consumo de RAM ainda está alto para um projeto em Rust."

**Medido** no app do dono aberto havia 4 h 40 (0.3.9 instalada), sem tocar
nele: `VirtualQueryEx` região por região e uma amostra do conteúdo de cada
alocação (`ReadProcessMemory`, 48 páginas por região).

| | MB |
|---|---|
| Memória privada do processo | 1.122 |
| Pico de memória paginada | 1.694 |
| Tabela de C: (3.326.827 nomes): `entries` | 160 (127 em uso) |
| Tabela de C:: `names` e `lower` | 152 + 152 (111 + 111 em uso) |
| Tabela de D: (270 mil nomes) | 47 |
| **Índice do Everything, ao todo** | **~511** |
| Resto (heap, quadros do gpui, miniaturas) | ~610, 300 deles páginas nunca escritas |

Quase metade da memória privada era o índice, e ele só era lido na primeira
busca (modo Everything ou Ctrl+T) e nunca mais saía: o app inicia com o
Windows e fica aberto o dia inteiro. Três causas, da maior para a menor:

- **Nunca era solto.** Agora quem pode buscar segura um `InUse` (o painel
  no modo, a caixa do Ctrl+T aberta); sem ninguém por `RELEASE_AFTER`
  (5 min) a tabela sai da memória e a próxima busca a lê do arquivo de cache.
  Uma varredura ou leitura em curso adia.
- **Folga de capacidade.** As três partes de cada tabela crescem dobrando,
  nome a nome: 464 MB reservados para 349 de conteúdo em C:. `Segment::shrink`
  ao fim de cada disco (MFT e varredura).
- **Pico ao ler.** `load` lia o arquivo inteiro (354 MB) e copiava os nomes
  de dentro dele: arquivo e tabela na memória ao mesmo tempo. Agora lê cada
  parte direto no lugar onde ela fica, e recusa um cabeçalho que promete
  mais do que o arquivo tem antes de reservar.

**Cobertura.** `a_table_filled_name_by_name_gives_back_what_it_did_not_use`,
`a_header_promising_more_than_the_file_holds_is_refused_before_reserving`,
`the_index_is_let_go_only_with_nobody_searching_for_a_while` (só a regra; o
temporizador não tem teste) e os de leitura que já existiam, que passam pelo
`load` novo.

**Não conferido.**
- O app não foi aberto com a mudança: os números de depois (esperado ~120 MB
  parado, ~470 com o índice carregado) são conta, não medida.
- Quanto demora a primeira busca depois de o índice ter sido solto (ler
  354 MB e refazer as máscaras de 3,6 mi de nomes).
- Uma caixa do Ctrl+T que fique aberta e escondida seguraria o índice.

- **Minúsculas repetidas** (feito depois, a pedido do dono). `lower` repetia
  `names` byte a byte em todo nome que já é minúsculo (quase todo o WinSxS).
  Agora o bit `SAME` em `flags` diz "o minúsculo é o próprio nome": `lower`
  aponta para dentro de `names` e só os nomes com maiúscula guardam a
  segunda forma. O arquivo virou PFEVRY04 (o bit é salvo); o 03 e o 02 são
  lidos e compartilhados na leitura (`share_lower`). Uma versão anterior do
  app recusa o 04 e indexa de novo. Contado no índice real do dono (3,62 mi
  de nomes): `lower` cai de 118,7 para 27,0 MB, na memória e no arquivo.
  Testes: `a_name_already_lowercase_is_kept_once` e o de tabela antiga.

**Ficou de fora** (mexe na estrutura):
- `Entry` tem 40 bytes (37 + alinhamento); `size` e `mask` em 64 bits são
  metade disso.
- Durante uma atualização o índice velho e o novo coexistem (2×), de 30 em
  30 min de uso com o indexador da MFT.
- Os ~610 MB que não são o índice não foram atribuídos item a item: há
  blocos de 14–32 MB quase todos zerados (listas de primitivas de quadro do
  gpui, que crescem até o pior quadro e não encolhem) e o teto de 192 MB de
  miniaturas por painel.

### 12. A barra lateral lia o disco a cada quadro (11/11/2026)

**Queixa** (X, 11/11): "de umas atualizações pra cá, demora um bom tempo
depois de clicar até executar a ação. Mas quando vou pras configurações roda
liso."

**Achado.** Amostrando a pilha da thread de interface do app do dono aberto
(0.3.10 instalada; 400 amostras, `SuspendThread` + `StackWalk64`, sem tocar no
app), parte do tempo ocupado era `CreateFileW` + `GetFileInformationByHandle`
+ `CloseHandle` — um `fs::metadata` do Rust — dentro do desenho. A origem:
`sidebar.rs::row` chamava `ExplorerPane::place_address` em toda linha, a cada
quadro, para decidir se ligava o clique do meio (issue #14, 0.3.8), e
`place_address` perguntava `Path::is_file()` para não abrir aba de favorito
que é arquivo. As Configurações não desenham a barra lateral: por isso "rodam
liso".

**Medido** (`Path::is_file`, binário de release à parte, mediana de 50):

| Onde está o item da barra | Por chamada |
|---|---:|
| C: (SSD, em cache) | 0,022 ms |
| G:/I: (Google Drive) | 0,13–0,18 ms |
| Os 16 itens do Acesso rápido do dono, somados | 0,83 ms por quadro |
| Compartilhamento que não responde (`\192.0.2.1\share\pasta`) | **21.051 ms**, toda vez |

Num disco saudável o custo cabe no quadro. Basta um item fixado num
compartilhamento desligado, numa VPN caída ou num HD que dormiu para cada
quadro da lista esperar o disco — e o Windows não guarda a falha, então a
espera se repete no quadro seguinte.

**Mudança.** `place_address` não lê mais o disco: caminho volta como
endereço. A pergunta "é arquivo?" passou para `open_place_in_new_tab`, feita
só no clique do meio e numa thread de fundo (§2.12 do `OTIMIZACAO.md` do
hub). O comportamento do #14 fica o mesmo: favorito que é arquivo não abre
aba, pasta abre.

**Cobertura.** `a_places_address_is_answered_without_reading_the_disk`
(arquivo existente responde com o caminho: só uma leitura do disco o
distinguiria de pasta, então o teste reprova se ela voltar) e
`the_middle_click_on_a_place_opens_folders_and_skips_files`.

**Não conferido.**
- A máquina de quem reclamou: não sabemos se ela tem item fixado em rede ou
  HD externo. A causa encontrada explica exatamente o sintoma relatado, mas
  pode não ser a única.
- O app do dono, parado e visível, gasta ~10% de um núcleo na thread de
  interface (90% das amostras em `GetMessage`, o resto desenhando quadros
  seguidos). A cópia isolada com a configuração padrão fica em 0,8%. Quem
  pede esses quadros não foi achado: a amostra do binário publicado não tem
  símbolos.
- `open_place` (clique esquerdo num lugar) ainda pergunta `is_file()` na
  thread de interface — uma vez por clique, não por quadro, mas num
  compartilhamento morto ainda congela 21 s. Feito na passada 13.

### 13. Nenhuma ida ao disco na thread de interface (11/11/2026)

**Origem.** A comparação com o Open Salamander (§2.12 do `OTIMIZACAO.md`
do hub): lá, testar um caminho de rede roda numa thread descartável, e até
fechar um handle tem thread própria (`salamdr5.cpp`, `snooper.cpp`). Depois
da passada 12, a varredura do código achou estes pontos ainda perguntando ao
disco, com caminho do usuário, na thread de interface:

| Onde | Quando |
|---|---|
| `folder_watch.rs`, `watch_listed_folders`: `is_dir()` de cada pasta na tela, abrir o vigia, `GetVolumeInformationW`, fechar o vigia anterior | depois de **toda** listagem |
| `palette.rs`, `palette_changed`: `exists()` + `is_dir()` do texto digitado | a cada tecla no Ctrl+T, quando o texto é um caminho (`\\s`, `\\se`, … já vão à rede) |
| `places.rs`, `open_place`: `is_file()` | clique num lugar da barra lateral |
| `commands.rs`, `folder_for_programs`: `is_dir()` | "Abrir em" |

**Medido** (sonda `watch_on_a_dead_share_holds_the_ui_for`, ignorada por
padrão, com a pasta da tela em `\\192.0.2.1\share`):

| | Thread de interface presa |
|---|---:|
| Antes | **21.046 ms** |
| Depois (duas rodadas) | 0,0 ms |

**Mudança.**
- O vigia é montado no fundo (`start_watching`) e se instala quando termina;
  a interface só filtra o que não é caminho (`is_absolute`, sem disco). O
  vigia velho é solto numa thread própria (`impl Drop for FolderWatch`):
  fechar handle de compartilhamento morto também prende.
- Ctrl+T: a pergunta vai para o fundo; a linha do caminho digitado aparece
  com a resposta, e a da tecla anterior fica até lá, como os resultados.
  A seleção que as setas escolheram não pula (`place_typed_row`).
- `open_place`: arquivo ou pasta é decidido no fundo; o clique segue para o
  programa ou para a pasta quando a resposta chega.
- "Abrir em": a pasta é decidida pelo endereço (`is_absolute`, e não é um
  servidor sozinho), não pelo disco.

**Cobertura.** Cada teste reprova com a versão síncrona antiga (mutante
conferido nos três): `the_watch_is_opened_off_the_ui_thread` (registra, só em
teste, onde o disco foi tocado — conferir o campo `watcher` não bastava: o
primeiro rascunho do teste passava com o mutante),
`a_typed_path_is_looked_up_off_the_ui_thread`,
`a_click_on_a_place_asks_the_disk_off_the_ui_thread`. O teste antigo do vigia
(`the_folder_on_screen_follows_what_other_programs_do`) continua verde.

**Ficou de fora.**
- `context_menu.rs` (`apply_preview_choice`, `open_item_location`) e
  `page.rs` (`terminal_directory`) também perguntam ao disco na interface;
  outra sessão editava esses arquivos ao mesmo tempo.
- `commit_path_edit` já pulava a rede (`\\`); unidade mapeada desconectada
  (`Z:`) ainda pergunta na interface, uma vez por Enter.
- `preview_folder` pergunta `is_dir()` a cada atalho na prévia, mas só de
  uma pasta que acabou de ser listada.
- Uma pergunta presa ocupa uma thread do executor do gpui pelos 21 s; o
  Salamander põe teto (30 threads descartáveis). Aqui não há teto.
- Clicar num lugar de compartilhamento morto agora não congela, mas também
  não mostra nada durante os 21 s até a listagem começar.
- Não visto no app: só testes e a sonda.

### 14. O player parado redesenhava a tela dez vezes por segundo (11/11/2026)

**Sintoma.** O app do dono, parado e visível, gastava ~10% de um núcleo na
thread de interface (passada 12, "Não conferido"). A captura da janela
mostrou o estado: vista em grade com um `.avi` selecionado e o player da
prévia aberto, parado em 0:00.

**Causa.** A thread do player (`media.rs`, `platform::run`) mandava
`Update::State` a cada 100 ms, mudasse algo ou não, e cada um faz
`cx.notify()` no painel inteiro (lista, barra lateral, prévia). É o mesmo
erro que o #53 corrigiu no `root.rs`.

**Medido** na cópia isolada (pasta com 13 `.avi`, janela fora da tela,
clique por `PostMessage`; `player-idle.ps1`), A/B alternado:

| | Sem seleção | Vídeo na prévia, parado |
|---|---:|---:|
| 0.3.10 instalada, rodada 1 | 1,2% | 11,2% |
| Com a correção, rodada 1 | 1,7% | 1,7% |
| 0.3.10 instalada, rodada 2 | 0,5% | 9,1% |
| Com a correção, rodada 2 | 0,8% | 1,2% |

(% de um núcleo, 10 s cada. A build da correção é o HEAD 2475d8f + só esta
mudança, numa worktree limpa.)

**Mudança.** `report_if_changed`: o estado só vai à interface quando muda.
Parado, a posição não anda e nada é enviado; tocando, a posição muda e o
relatório segue a cada 100 ms. Conferido na tela, mudo: o relógio andou de
0:03 para 0:09.

**Cobertura.** `the_player_reports_only_what_changed` (só a regra; que o laço
do player a use não tem teste — o motor de mídia do Windows não roda em
teste).

**Visto e não mexido.**
- Tocando, o app gasta ~23% de um núcleo: cada quadro de vídeo também
  redesenha o painel inteiro, porque o player mora dentro dele. Um player
  como view própria redesenharia só a si mesmo.
- O `.avi` (xvid) aparece preto na prévia parada em 0:00, no app do dono e
  na cópia; tocando, a imagem aparece. Provavelmente é o primeiro quadro do
  episódio; não investigado.

### 15. O vigia calado durante as operações do próprio app, e no ritmo do custo (11/11/2026)

**Origem.** Item 3 da comparação com o Open Salamander. Lá, enquanto o
próprio programa copia, move ou apaga, o vigia entra em "modo suspenso": as
pastas mudadas vão para uma lista, e no fim cada painel recebe **um**
refresh (`snooper.cpp`, `BeginSuspendMode`). E o intervalo entre refreshes
em segundo plano é 20× o custo do último, entre 2 e 10 s (`fileswnb.cpp`).
Aqui (`folder_watch.rs`), durante uma cópia nossa para a pasta aberta, o
vigia relia a pasta inteira a cada 0,8 s, e `run_fs_op_with` relia mais uma
vez no fim. A sondagem das unidades sem aviso (Google Drive) lia tudo a cada
3 s fixos, qualquer que fosse o tamanho da pasta.

**Medido** (`reloads_during_an_own_copy`: 60 arquivos escritos em ~3 s por
uma thread, na pasta aberta, com o relógio do teste andando junto do real;
conta `listing_generation`):

| | Releituras |
|---|---:|
| Cópia nossa, antes (duas rodadas) | 6 e 6 |
| Cópia nossa, depois | **1** |
| Outro programa escrevendo, depois | 3 |

O número de antes cresce com a duração: uma cópia de 10 minutos daria ~750
releituras da pasta. O de outro programa caiu porque o intervalo agora
conta do **fim** da leitura; nesta pasta pequena o ritmo pelo custo fica no
piso de 0,8 s.

**Mudança.**
- `begin_own_operation` → `OwnOperation`: `run_fs_op_with` (por onde passam
  todas as operações) segura o vigia calado e o solta logo antes da releitura
  que faz no fim.
- Cada aviso leva a hora em que chegou (relógio lógico, §5.4 do
  `OTIMIZACAO.md`): o que chegou antes do fim da operação já é lido pela
  releitura dela e é descartado; o que chega depois gera refresh normal.
- Ritmo pelo custo (`paced`): o próximo refresh espera 20× o que a última
  leitura custou, entre 0,8 e 10 s; a sondagem, entre 3 e 30 s.

**Cobertura.** `an_own_copy_reads_the_folder_once` e
`a_file_operation_holds_the_watch_quiet_until_done` (mutantes conferidos: sem
a espera calada, 3 releituras; sem a guarda em `run_fs_op_with`, reprova),
`the_watch_waits_twenty_times_what_a_read_cost`, e o teste antigo
`the_folder_on_screen_follows_what_other_programs_do` (arquivo salvo por
outro programa continua aparecendo).

**Ficou de fora.**
- Um aviso da nossa operação que chegue depois do fim dela (atraso do
  sistema) ainda gera um refresh a mais. O Salamander marca o refresh até 1 s
  depois do fim como "provavelmente inútil" e compara as listagens; aqui não.
- Outra aba na mesma pasta não fica calada (o modo é por painel).
- As operações do celular (`run_device_op`, sem commit, de outra sessão) não
  passam por `run_fs_op_with`.
- O custo da leitura é medido de 25 em 25 ms (`listing_pending`); abaixo
  disso dá o piso.
- Janela em segundo plano trabalha igual (item 4).
- Não visto no app: só testes.

### 16. Atrás de outra janela, o vigia anda mais devagar (12/11/2026)

**Origem.** Item 4 da comparação com o Open Salamander. Lá, com a janela em
segundo plano, o intervalo entre refreshes de pasta tem piso de 2 s
(`MIN_DELAY_BETWEENINACTIVEREFRESHES`, `fileswnb.cpp`); em foco, só a pausa
mínima. Aqui (`folder_watch.rs`) o vigia e a sondagem das unidades sem aviso
andavam igual na frente e atrás: uma pasta do Google Drive aberta atrás do
navegador era lida inteira a cada 3 s o dia todo.

**Mudança.**
- O painel guarda se a janela está na frente (`in_front`), atualizado pelo
  observador de ativação que já fechava os menus (issue #15).
- Atrás, o piso entre refreshes sobe de 0,8 s para 2 s (o teto de 10 s e os
  20× o custo continuam), e a sondagem passa de 3–30 s para 10–60 s.
- As duas esperas andam em passos (100 ms o vigia, 500 ms a sondagem): ao
  voltar para a frente, o que estava esperando o ritmo de trás sai na hora.
- Miniaturas não mudaram: já são pedidas só para as linhas desenhadas, sem
  vizinhança, então atrás já se lê só o visível (o `InactWinOptimizedReading`
  do Salamander).

**Em números** (conta pelas regras, não medida): pasta rápida com outro
programa gravando sem parar, de até 75 para até 30 refreshes por minuto
atrás; pasta pequena do Google Drive, de 20 para 6 leituras por minuto atrás.

**Cobertura.** `behind_another_window_the_watch_keeps_a_slower_pace` (as
regras), `behind_another_window_refreshes_wait_longer_until_it_comes_to_the_front`
(atrás, o segundo save não aparece em 1,2 s; ao voltar, aparece em 0,4 s),
`behind_another_window_a_polled_folder_is_looked_at_less` (atrás, nenhuma
leitura em 4 s; ao voltar, uma em 0,8 s) e
`the_pane_knows_when_its_window_goes_behind` (a ligação com a ativação da
janela). Mutantes conferidos: sem o piso de trás, sem as esperas em passos
(vigia e sondagem), sondagem sem olhar a janela e observador sem gravar o
estado; cada um reprova o teste dele.

**Achado, não mexido.** O custo que dá o ritmo (passada 15) é cronometrado
na thread de interface, de 25 em 25 ms, até `listing_pending` cair. Com a
interface ocupada, uma leitura de 1 ms conta como cara e empurra o próximo
refresh para até 10 s. Nos testes isso aparece forte: o relógio do teste anda
mais devagar que o real, a leitura de uma pasta com poucas dezenas de
arquivos teve "custo" de 1,13 s e o vigia parou de seguir por 10 s (só 37–40 dos 120 na tela 3 s depois de outro
programa terminar de gravar). Por isso não há medida de releituras desta
passada. O certo é a própria leitura (`PendingListing`, em `navigation.rs`)
devolver quanto levou.

**Ficou de fora.**
- Janela minimizada é tratada como atrás, não parada.
- Não visto no app: só testes.

### 17. O ritmo do vigia pelo custo da leitura, não da espera (12/11/2026)

**Origem.** Achado da passada 16. O custo que dá o ritmo (`paced`, passada
15) era cronometrado na thread de interface: o vigia olhava de 25 em 25 ms
se `listing_pending` tinha caído. Interface ocupada fazia uma leitura de
1 ms parecer cara, e o próximo refresh esperava o teto de 10 s.

**Medido** (`another_program_writes`: outro programa grava 120 arquivos em
~6 s na pasta aberta; conta refreshes e quantos arquivos estão na tela 3 s
depois do último):

| | Refreshes | Na tela |
|---|---:|---:|
| Antes (duas rodadas, na frente) | 2 e 2 | 38 e 42 de 120 |
| Depois, na frente (cinco rodadas) | 8–9 | 120 |
| Depois, atrás de outra janela (duas rodadas) | 4 e 4 | 120 |

Nos testes o "antes" é exagerado pelo relógio do teste, que anda mais
devagar que o real; no app o efeito aparece quando a interface está ocupada
(miniaturas, prévia, rolagem) no momento em que a leitura chega.

**Mudança.** A thread da leitura (`PendingListing`) cronometra a si mesma e
devolve `took` no `FolderRead`; `apply_listing` guarda em `last_read_cost`, e
o vigia usa esse número. O laço de 25 ms ficou só para saber quando a
leitura terminou. De quebra, o custo agora conta também a parte síncrona da
leitura (até 30 ms dentro de `reload`), que antes ficava de fora.

**Cobertura.** `the_watch_keeps_up_with_another_program_writing` (reprovava
no código antigo: 38 e 42 de 120) e `a_read_is_timed_by_its_own_thread` (com a
interface ocupada 300 ms, o custo fica abaixo de 300 ms e acima de zero;
mutante sem guardar o custo reprova). A medida da passada 16 agora existe:
`measure_reloads_in_front_and_behind` (ignorado), 8–9 refreshes na frente
contra 4 atrás.

**Ficou de fora.**
- Nenhum teste prova que o vigia usa `last_read_cost` para pastas lentas:
  um mutante que trocasse o custo por zero passaria (pastas de teste leem em
  ~1 ms, o piso de qualquer jeito).
- Não visto no app: só testes.

### 18. Memória por item: a grade guardava 3,4 KB por nome (12/11/2026)

**Origem.** Item 5 da comparação com o Open Salamander. Em 11/11, abrir a
WinSxS (23,5 mil itens) somava ~40 MB ao app, ~1,7 KB por item; a suspeita
era o `FileEntryDto` (3 `String`) e `filtered_entries` ser cópia inteira de
`entries`.

**Medido** com um alocador que conta os bytes vivos (só nos testes,
`heap_count` em `prime_files_pages.rs`), listando `C:\Windows\WinSxS`
(23.518 itens) num painel de verdade e soltando cada parte:

| Parte | MB | B/item |
|---|---:|---:|
| `entries` | 7,7 | 343 |
| `filtered_entries` (cópia) | 7,7 | 343 |
| o resto da listagem | 0,2 | 7 |
| `name_layouts` da grade, antes | **76,3** | **3.401** |
| `name_layouts` da grade, depois | 20,5 | 913 |

(Bytes pedidos; o heap do Windows soma ~16 B por alocação. O sistema de
texto dos testes faz um glifo por caractere, como o DirectWrite com nomes
latinos.)

O maior custo não era a listagem: era a grade. Para saber a altura de cada
linha antes de desenhar, ela quebra **todos** os nomes, e guardava para
cada um o `LineLayout` inteiro do gpui — id, posição, índice e marca de
emoji de cada glifo (24 B), num vetor que cresce dobrando (um nome de 70
glifos reserva 128). Só na visão em grade; lista e detalhes não guardam.

**Mudança.** `NameWidths` guarda de cada glifo só o que a quebra lê — o byte
no nome e o x —, 8 B, numa fatia do tamanho exato, sem `Arc`;
`index_for_x`/`x_for_index` dão a mesma resposta do gpui. 76,3 → 20,5 MB na
WinSxS em grade (−73%).

**Cobertura.** `name_widths_answer_as_the_shaped_line_does` (duas corridas,
caracteres de 1 a 4 bytes, avanços desiguais, linha vazia; mutantes `<=`→`<`
e `>=`→`>` reprovam), `a_name_keeps_eight_bytes_a_glyph`, e as medidas
ignoradas `measure_grid_name_memory` e `measure_listing_memory`.

**Ficou de fora.**
- A cópia em `filtered_entries` (7,7 MB na WinSxS) continua. Tirar exige
  trocar `name`/`path` para algo de clone barato (`Arc<str>`): 31 lugares
  constroem `FileEntryDto` e 86 clonam esses campos, vários em arquivos com
  trabalho de outra sessão sem commit (inclusive `device.rs`, ainda fora do
  git).
- `kind` é `String` ("file"/"dir"): uma alocação por item que poderia ser
  `&'static str`; mesma troca de tipo.
- A grade ainda guarda o nome duas vezes (chave do mapa e a entrada) e o
  índice do glifo, que na maioria dos nomes é só o de cada caractere
  (guardar só o x daria ~4 B por glifo).
- Os ~1,7 KB/item de 11/11 vinham do app aberto, com o heap do Windows e o
  gpui; esta passada não mediu no app.

**De quebra.** `the_watch_keeps_up_with_another_program_writing` (passada 17)
reprovou uma vez em ~13 com a máquina ocupada: dava só 3 s para o último
refresh. Agora espera até 4,5 s, parando quando os 120 aparecem; com o
código antigo continua reprovando (56 e 39 de 120).

### 19. O resto do disco na interface (12/11/2026)

**Origem.** O "Ficou de fora" da passada 13, e um caso pior achado no
caminho: `terminal_directory` (`page.rs`) era chamado a cada render do
painel do terminal (`follow_tab_folder`) e fazia `is_dir()` — com o terminal
aberto numa aba de compartilhamento morto, 21 s por quadro, o mesmo defeito
da barra lateral (passada 12).

**Mudança.**
- `terminal_directory` decide pelo endereço (`is_disk_folder_address`):
  pasta de unidade ou de compartilhamento é pasta; Este Computador, tags,
  celular, servidor sozinho e `\.\` não. A aba só guarda pastas que listou.
- Campo de caminho (`commit_path_edit`): `is_dir`/`is_file` numa thread de
  fundo; o campo fica aberto até a resposta e a resposta só vale se o texto
  ainda for o mesmo. `Z:` desconectado não segura mais a janela a cada Enter.
- "Abrir local do arquivo" (`open_item_location`): ler o atalho e o
  `exists()` no fundo.
- "Abrir em nova aba" do menu da prévia: `is_dir()` no fundo.
- `run_device_op` segura o vigia calado como `run_fs_op_with` (commit
  próprio, bf2764f): a cópia do celular para a pasta aberta relia a cada
  800 ms.

**Cobertura.** `the_terminal_folder_is_told_by_the_address_alone` (cronometra
o compartilhamento morto: < 50 ms),
`path_field_shows_the_path_and_goes_where_it_is_told` (o campo não fecha
antes da resposta; com a versão síncrona reprova),
`a_device_operation_holds_the_watch_quiet_until_done` (mutante sem o guarda
reprova).

**Ficou de fora.**
- "Abrir local do arquivo" e "Abrir em nova aba" da prévia sem teste.
- Teto de threads presas numa pergunta ao disco (o Salamander usa 30) e
  algum sinal na tela durante os 21 s de um compartilhamento morto.

### 20. O quadro do vídeo no tamanho em que aparece (12/11/2026)

**Origem.** A prévia tocando gastava ~23% de um núcleo (passada 14). Cada
quadro era copiado em até 1280×720 qualquer que fosse o painel — num painel
de 400 px, ~6 vezes os pixels mostrados —, copiado duas vezes na CPU
(`TransferVideoFrame` no bitmap WIC, linha a linha para o `Vec`) e enviado
de novo à placa.

**Mudança.** A view mede a caixa do quadro, zoom incluído, num `canvas`
(`media::set_frame_room`, arredondado a 32 px para não recriar o bitmap a
cada pixel ao arrastar a borda); o player copia em `frame_size(nativo,
caixa)`, nunca acima do vídeo nem de 1280×720. Vale para a prévia e para a
busca do Ctrl+T.

**Cobertura.** `a_frame_is_copied_at_the_size_it_is_shown`.

**Ficou de fora.**
- Não medido no app: a CPU tocando não foi medida de novo.
- Cada quadro ainda redesenha o painel inteiro. Redesenhar só o player
  exigiria lista e barra lateral como views com `.cached()`: o gpui marca
  sujos os ancestrais de quem chama `notify`, e nada no app é cacheado hoje.

### 21. Outra aba na mesma pasta também espera (12/11/2026)

**Origem.** "Ficou de fora" da passada 15: o silêncio do vigia era do
painel que começou a operação; outra aba na mesma pasta relia a cada 800 ms.

**Mudança.** `BUSY` (thread local da janela): pastas ocupadas por uma
operação nossa e quantas. `OwnOperation` registra as pastas do painel e
solta no `Drop`; o vigia de qualquer aba que olhe uma delas espera o fim e
relê uma vez (o painel de origem relê sozinho). Aba em outra pasta não
espera.

**Cobertura.** `another_tab_on_the_folder_waits_for_our_copy` (a outra aba
relê 1 vez; a aba em outra pasta continua seguindo o disco); mutante sem
`any_busy` reprova.

**Ficou de fora.** O aviso que chega depois do fim da operação ainda gera
uma releitura. O Salamander a marca como "provavelmente inútil" e compara as
listagens; aqui a releitura seria feita do mesmo jeito e o ganho seria só o
redesenho, contra o risco de engolir uma mudança de outro programa no
mesmo segundo.

### 22. Um teste para o ritmo em pasta lenta (12/11/2026)

**Origem.** "Ficou de fora" da passada 17: nenhum teste provava que o vigia
usa `last_read_cost`; pastas temporárias leem em ~1 ms, o piso de qualquer
jeito.

**Mudança.** Gancho só de teste `make_read_slow` (por caminho, para não
atrasar os testes ao lado). `a_slow_folder_is_read_at_the_pace_of_its_cost`:
pasta de 500 ms, ~8 s de escritas de outro programa, leitura no fundo
(`listing_wait` 0). Com o custo: 1 releitura (duas rodadas); com o custo
zerado: 3 (mutante, duas rodadas). Limite em 2. Com 150 ms a diferença era
3 contra 4 — margem curta demais, trocada.

### 23. Uma terceira cópia da listagem, a cada quadro (12/11/2026)

**Achado.** `ensure_list_initialized` rodava em todo render da listagem e,
depois da primeira vez, clonava `filtered_entries` inteira para um
`FileListDelegate` numa `ListState` que nunca é desenhada nem lida (resto do
upstream). Na WinSxS (27.385 itens, debug): 10,2 ms por quadro só nessa
cópia, e 8,5 MB guardados numa terceira cópia da listagem. A passada 18 não
viu porque `measure_listing_memory` não desenha.

**Mudança.** A `ListState`, o delegate e a função saíram; a medida da cópia
entrou em `measure_listing_memory`.

**Ficou de fora.** `entries` e `filtered_entries` continuam duas cópias
(8,5 MB cada na WinSxS). Compartilhar por `Arc` troca o tipo em ~230 usos;
com a terceira cópia fora, o ganho fica nas pastas de dezenas de milhares
de itens.

**De quebra.** `the_watch_keeps_up_with_another_program_writing` reprovou
uma vez de novo (94 de 120) numa rodada da suíte que levou 25 s, contra
13 s das outras; sozinho passou 3 de 3, a suíte 2 de 2.
