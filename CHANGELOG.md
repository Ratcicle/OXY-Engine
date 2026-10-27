# Histórico de versões

Resumo de cada versão. Os relatórios completos, com testes, capturas e medições, ficam em [`docs/`](docs/); as capturas de versões anteriores continuam acessíveis pelas tags do repositório.

## 0.7.0 — (data a definir no lançamento)

- **Scripts em Luau:** a lógica do jogo passa a ser escrita em Luau, em arquivos `.luau` de qualquer pasta do projeto, que os objetos anexam. É o caminho principal da lógica para todos os jogos da OXY: quem escreve é um agente de IA, pelo MCP, ou uma pessoa, no VS Code; a OXY anexa, roda, mostra os erros e recarrega. Um script serve a vários objetos, cada um com os seus valores em atributos. O Luau 0.740 roda pelo `mlua` 0.12.2, sem JIT, numa máquina virtual (VM) criada só quando a cena tem um script: um projeto só com nós joga como na 0.6 (a única mudança nos nós é o limite de trabalho por passo, abaixo).
  - **Caixa de areia:** nada de disco, rede, relógio do computador ou sorteio fora da engine. Os globais são só de leitura e recusam um nome desconhecido com o mais parecido, e o módulo fica congelado depois de carregar.
  - **Estado na engine:** nada do jogo fica guardado no Luau. O que dura de uma chamada para outra fica em atributos, na memória do script (`Modulo.state`, declarada no topo, com listas e mapas), nas variáveis do jogo, em temporizadores e em mensagens, todos guardados pela engine. Um `local` do topo alterado dentro de uma função é recusado ao carregar, com a linha. Assim, a recarga, os saves e a rede da 0.8 não perdem nada.
  - **Eventos:** `on_start`, `on_step`, `on_input`, `on_click`, `on_area_enter`, `on_area_exit`, `on_marker`, `on_character`, `on_pause` e `on_message`, na ordem da cena, depois da lógica de nós do objeto e na ordem da lista de scripts dele.
  - **Temporizadores e mensagens:** `self:after`, `self:every`, `self:sequence`, `self:schedule` e `self:cancel` contam o tempo de jogo, que para na pausa, sem `wait()` que deixe o script parado. `obj:send` e `on_message` trocam mensagens entre objetos, na ordem de envio.
  - **Limites:** o trabalho é contado em unidades que dão o mesmo em toda máquina, com uma trava contra laço sem fim e um teto de memória. Um laço sem fim ou um limite passado (trabalho demais numa chamada, temporizadores ou mensagens demais, a memória do script acima de 64 KB) desliga o script naquele objeto, até o arquivo ser salvo de novo ou a cena recomeçar.
  - **Erros com arquivo e linha:** um erro para só aquela chamada, e o resto do jogo segue. Ele vai ao Console uma vez, em português, com o objeto, o arquivo e a linha (`Pista/porta.luau:8`) e um contador quando se repete. É igual no teste do editor, no `oxy_simulate` e no jogo exportado.
  - **Recarga:** durante o teste no editor, salvar um script o recarrega entre dois passos, mesmo com o teste pausado, e o jogo continua de onde estava, com os atributos, a memória e os temporizadores. Um texto com erro fica de fora, e o teste segue com a versão anterior.
  - **Atributos declarados:** `Modulo.attributes = { Velocidade = 90 }` diz o que o script espera, e anexá-lo cria os atributos que faltam, num passo de desfazer, avisando quando o objeto já tem o atributo com outro tipo. `param.object()`, `param.surface()` e `param.vector2()` declaram os tipos que não têm valor literal.
  - **`require`** de outros scripts do projeto com `./`, `../` ou `@project/`, sempre dentro da pasta do projeto; cada módulo roda uma vez e é o mesmo para todos.
  - **Tipos para o VS Code:** a OXY grava os tipos da API em `.oxy/tipos/oxy.d.luau`, um `.luaurc` e as chaves da extensão luau-lsp (conferida na versão 1.70.1) no `.vscode/settings.json`, para o VS Code completar os nomes, mostrar a ajuda em português e acusar nomes e tipos errados.
- **API dos scripts, versão 1:** 175 funções, propriedades e eventos, com nomes em inglês (`scene.find`, `obj.world_position`, `audio.play`), que cobrem tudo o que os nós fazem e algumas coisas que eles não faziam: mensagens entre objetos, achar objetos pelo nome, pai e filhos, texto e cor da interface e o `on_start` das cópias. Os nomes de objetos, atributos, sons e cenas continuam os do projeto.
  - **Áreas:** objetos e cena, transformação, vetores e quatérnios (`vmath`, `quat`), personagem, física (`physics.raycast` e as outras consultas), câmera, animação, interface, som, jogo e progresso salvo, entrada, tempo e sorteios, com sequências nomeadas que dão o mesmo numa repetição.
  - **Referência gerada:** uma tabela só gera a API que o jogo instala, os tipos do VS Code, a referência em [`docs/api-scripts.md`](docs/api-scripts.md) e as páginas da API no Guia. Cada função tem a assinatura, a ajuda, o que faz a chamada parar com erro, os nós equivalentes, onde vai rodar com vários jogadores e um exemplo que um teste roda. Um teste confere que cada uma das 86 operações dos nós tem equivalente na API ou no Luau puro.
  - **Conferência de nomes:** sem rodar o script, os nomes do projeto escritos como texto (sons, variáveis, atributos, cenas, modelos, ações, clipes, objetos e superfícies) e os nomes da própria API (`audio.paly`) são conferidos, com a linha e o nome mais parecido.
  - Os nomes da 0.8 (`players`, `session`, `on_request`…) ficam reservados, com um erro que diz que chegam na 0.8, e `require("@native/…")` fica reservado para os módulos em Rust por jogo.
- **Árvore do projeto:** a pasta do projeto é a Biblioteca.
  - **Pastas reais:** imagens, sons, modelos e scripts ficam em qualquer pasta, juntos como a pessoa quiser (`Pista/` com tudo da pista), e as pastas da Biblioteca são as do disco. Pastas e arquivos que começam com ponto (`.oxy`, `.vscode`, `.git`) ficam fora do projeto.
  - **Nomes legíveis:** cada arquivo tem o nome que aparece na Biblioteca, único por tipo no projeto inteiro (o segundo vira "Música 2"). Renomear e mover na OXY renomeia e move no disco, sem nunca gravar por cima de um arquivo.
  - **Modelos como arquivos:** cada modelo é um arquivo `<Nome>.modelo.json`, com o ID dentro; um modelo movido fora da OXY é achado pelo ID.
  - **Conversão com backup:** um projeto antigo é organizado em pastas no primeiro salvamento, pelo diálogo **Organizar o projeto em pastas**, que mostra o que vai para onde, ou na primeira gravação de um agente: imagens em `Imagens/`, sons em `Sons/`, modelos em `Modelos/`, scripts onde estão, sem mudar nenhum ID. Antes, uma cópia completa do projeto vai para `.oxy/backup/antes-das-pastas-<data>/`, que ainda abre na 0.6, e uma conversão interrompida termina ou se desfaz na abertura seguinte.
  - **Desfazer com os arquivos:** criar, renomear, mover e excluir pastas e arquivos, e importar, são passos de Ctrl+Z que devolvem os arquivos. Excluir manda para a lixeira do projeto (`.oxy/lixeira`), **Descartar alterações** devolve também as pastas ao último salvamento, e um diário em `.oxy/` diz à abertura seguinte onde parou um lote interrompido.
  - **`require` que acompanha:** mover ou renomear um script ou uma pasta reescreve os `require` escritos por inteiro que deixariam de achar o módulo, no mesmo passo de desfazer.
  - Um script anexado que sumiu da pasta não impede abrir nem salvar o projeto; só jogar e exportar esperam até ele voltar.
- **Editor:** a interface aprovada no protótipo, para ver, anexar, ligar e ler os scripts.
  - **Cartão Scripts**, nas Propriedades: os scripts do objeto na ordem em que rodam, com **Ligado** (salvo por objeto e por script), quantos objetos usam cada um, o erro, **Ver código**, **Desanexar** e **Mover para cima** e **para baixo**. **+ Anexar script** lista os scripts por pasta, com busca, e os atributos criados aparecem com a nota **pedido por**. A Hierarquia marca **{ } N** nos objetos com scripts, em vermelho com **!** quando um falha ou falta.
  - **Aba do código:** só leitura, no painel de baixo, aberta por **Ver código** e pelos links do Console, na linha pedida, com cores, números de linha, quem usa o script e os erros embaixo das linhas. Ela lê o arquivo de novo quando ele muda e acompanha o script movido pela OXY.
  - **Console:** as origens **Editor**, **Agente**, **Jogo**, **Lógica** e **Script**, o filtro **Mostrar**, o arquivo e a linha como link para o código e as repetições juntadas com ×N.
  - **Biblioteca:** a coluna **PASTAS**, com as pastas reais e **Todos os recursos**; as fichas **Tudo**, **Imagens**, **Sons**, **Modelos** e **Scripts**; a busca no projeto inteiro; e a lista compacta, com o uso, o estado e a ação de cada item. Arrastar entre pastas (o alvo fica verde, ou vermelho com o motivo), **Mover para…**, **Nova subpasta**, **Renomear** e **Excluir…**, cada um com Ctrl+Z. Um arquivo que sumiu tem **Localizar arquivo…**, com sugestões, e os arquivos que a OXY não usa aparecem numa linha, com **Mostrar no Explorer**.
  - **Diálogos:** **Organizar o projeto em pastas**; excluir um item em uso ou um script anexado (**Tirar dos N e excluir**) e excluir uma pasta, que manda os itens para a raiz; **Descartar alterações**, com a lista do que volta nas pastas; e **Esvaziar lixeira do projeto**, no menu Projeto, que diz quantos passos de desfazer deixam de valer.
  - **Guia:** o Guia de lógica visual passa a se chamar **Guia** e começa pelos scripts (Primeiro script, Onde guardar valores, Esperar, repetir e mensagens, Erros e recarga), seguidos das três receitas em script e da **Referência da API**, uma página por assunto, com o total de funções. Os nós ficam embaixo, em amarelo, em **Nós (legado)**, e a busca acha páginas, receitas, funções e nós.
  - **Nós (legado):** a Lógica mostra o selo amarelo nos objetos com nós. Os nós continuam funcionando e editáveis, mas não ganham nós novos.
  - **Teste fora de foco:** **Continuar o teste quando a OXY perde o foco**, na seção nova **Teste** das Preferências do editor, desligada por padrão, deixa o teste rodando enquanto a pessoa usa o VS Code, para ver a recarga. A barra diz **Jogando · fora de foco**, e o teclado e o mouse só voltam ao jogo com a OXY em foco. Desligada, o **Pausado** da barra explica o motivo.
- **MCP:** as ferramentas passam a servir, antes de tudo, a quem escreve os scripts.
  - **`oxy_scripts`** (nova; são 21 ferramentas): lista os scripts com quem roda cada um, lê com números de linha, confere a sintaxe, a carga e os nomes sem gravar, grava tudo ou nada (com erro, nada é gravado), renomeia, move e exclui, e prepara o VS Code (`vscode`). Ao vivo, a gravação vai para o disco na hora, também durante o teste, que recarrega o script; o Ctrl+Z não a desfaz, e a primeira mudança de cada arquivo numa sessão fica em `.oxy/agente/`.
  - **`oxy_apply`:** `attach_script`, que cria os atributos declarados e diz quais em `attributes_created`, `detach_script`, `set_script_enabled` e `clear_logic`, que tira o grafo de nós de um objeto na troca por um script; `create_folder`, `rename_folder`, `move` e `delete_folder`, `save_model` com `folder` e recursos pelo caminho. Renomear ou excluir um som, uma cena ou uma variável que um script cita avisa com o arquivo e a linha.
  - **Leituras:** o `oxy_catalog` traz a API dos scripts por padrão, e os nós, que são legado, só com `nodes: true`; o `oxy_project` lista as pastas, os scripts e o que cada pasta tem, e conta como uso as linhas de script que citam um recurso; o `oxy_entity` descreve os scripts do objeto (eventos, atributos, memória e erro); o `oxy_import` aceita `folder`; o `oxy_console` aponta a linha do script, e o `oxy_status` resume os scripts e o layout do projeto.
  - **`oxy_simulate`:** logs e erros com `script {path, line}`, `script_errors`, `script_time` (o custo de cada arquivo) e `check_vm_reset`, que roda o jogo ao lado recriando a VM do Luau no meio e diz se algum script guarda estado escondido nela. Opções desconhecidas vêm em `unknown_options`, com a mais parecida.
  - **Conversão:** a primeira gravação de um agente nos arquivos organiza um projeto antigo em pastas, com backup, e a resposta traz `converted`, o que mudou; ao vivo, o `oxy_save` organiza como o primeiro Salvar da pessoa e diz em `note` que o desfazer do editor recomeçou.
  - Nos arquivos, a cópia do projeto de antes da primeira gravação de cada sessão do servidor passa a ficar em `.oxy/agente/project.oxy.json.bak`, fora da Biblioteca; antes, era `project.oxy.json.agente.bak`, ao lado do projeto.
- **OXY Parkour em Luau e receitas:** o parkour foi organizado em pastas (`Pista/`, `Menu/` e `Comum/`) pela própria OXY e reescrito em 16 scripts, com 387 linhas no lugar dos 449 nós, um por comportamento e reaproveitados entre objetos. A versão em nós da 0.6 virou um teste da engine, que joga as duas com os mesmos cliques e o mesmo bot e confere o percurso, os atributos, os saves, as trocas de cena e os sons. As receitas **Porta com chave**, **Carta e energia** e **Ataque por marcador** ganham versões em Luau, em `examples/guia-scripts`, que jogam igual às de nós.
- **Formato do projeto 6:** cada objeto ganha a lista ordenada `scripts`, com o caminho de cada arquivo a partir da pasta do projeto (e `"enabled": false` num script desligado); o projeto ganha a marca `"layout": "Tree"`, e os recursos passam a ter o caminho com o nome que aparece na Biblioteca (`Sons/Pulo.wav`), com cada modelo no seu arquivo `.modelo.json`, fora do arquivo do projeto. Projetos anteriores abrem como antes, convertidos em memória, e o primeiro salvamento grava o formato 6, já organizado em pastas. A 0.6.x e versões anteriores não abrem projetos salvos pela 0.7; a cópia completa em `.oxy/backup/` abre.
- **Pacote e build:** o Luau é C++. No build GNU, a `libstdc++` vai estática, e o `test-portable.ps1` recusa um executável que importe o runtime do GCC, o do Visual C++ ou uma DLL que não seja do Windows. O CI compila o Luau no MSVC com `/EHsc`, conferido arquivo por arquivo, e roda os testes de script no build do ZIP. As licenças do mlua e do Luau (MIT) vão no pacote e nos jogos exportados, e as notas de build estão no novo [`AGENTS.md`](AGENTS.md).
- **O que muda em projetos existentes:**
  - O primeiro salvamento organiza o projeto em pastas e dá aos arquivos o nome que aparece na Biblioteca, depois de guardar uma cópia completa; depois dele, o desfazer recomeça. Um projeto numa pasta com outros arquivos pede uma escolha: organizar numa pasta nova ou incluir esses arquivos no projeto.
  - O limite de trabalho da lógica de nós (1.024 unidades) passa a valer por passo do jogo, e não por quadro do editor ou do player: um projeto que o estourava agora se comporta igual em qualquer taxa de quadros. É a única mudança no comportamento dos nós; o parkour da 0.6 chega ao mesmo estado.
  - O Guia abre em Scripts, e as catorze receitas em nós ficam em **Nós (legado)**.
  - O `oxy_catalog` sem opções traz a API dos scripts; os nós vêm com `nodes: true`.
  - Um projeto novo precisa de uma pasta vazia, porque a pasta do projeto vira a Biblioteca.
  - O pacote não leva mais DLLs ao lado dos executáveis.

## 0.6.0 — 2026-10-17

- **Formato do projeto 5:** o formato sobe uma vez só na 0.6, para que nós existentes possam ganhar parâmetros e entradas. Projetos anteriores abrem como antes, convertidos em memória, e cada nó recebe os parâmetros novos com o valor padrão. O primeiro salvamento grava o formato 5 e guarda o arquivo original num backup (como `project.oxy.json.schema-4.….backup.json`, para um projeto da 0.5), também quando um agente edita os arquivos. A 0.5.x e versões anteriores não abrem projetos salvos pela 0.6.
- **Variáveis do jogo:** valores do projeto inteiro, com tipo e valor inicial, que continuam valendo ao mudar ou reiniciar a cena, até a partida acabar. O botão **Variáveis do jogo**, na barra da Lógica, abre a tabela com nome, tipo, valor inicial, **Salva entre partidas** e onde cada variável é usada. **Ler variável do jogo** e **Alterar variável do jogo** aceitam uma **Chave** opcional, que guarda um valor por chave, como um recorde por cena.
- **Progresso salvo:** **Salvar progresso**, **Apagar progresso** e **Ler progresso salvo** levam as variáveis marcadas de uma partida para outra. O jogo exportado grava em `%APPDATA%\OXY Engine\Jogos\<id do projeto>\progresso.json` (ou na pasta de `OXY_SAVE_DIR`). Um arquivo ilegível é guardado à parte, e um de versão mais nova nunca é sobrescrito. No teste do editor, o progresso fica na memória até abrir outro projeto, no bloco **Progresso do teste**, com uma linha no Console a cada gravação.
- **Pausa do jogo:** o jogo ganha uma pausa própria, separada da pausa da engine, para menus feitos na lógica: o evento **Ao pausar ou retomar** e os nós **Pausar / retomar jogo**, **Ler estado do jogo** e **Sair do jogo**. Nela o mundo para (física, animações, áreas e esperas), e a lógica, as ações de entrada e os cliques na interface continuam.
  - Numa cena com esse evento, Esc abre o menu do jogo, no player e no teste do editor, com o mouse livre, e a barra do teste mostra **Pausa do jogo**.
  - A perda de foco continua acionando a pausa da engine, com a janela **Jogo pausado** por cima do menu, e retomar volta ao estado em que o jogo estava.
  - **Sair do jogo** fecha o player e, no editor, para o teste.
- **Som:** o `oxy_audio` troca o WinMM, que abria um dispositivo por som, por uma saída cpal e um mixador próprio, com loops sem emenda, transições e cache de decodificação.
  - A categoria nova **Som** reúne **Reproduzir WAV**, agora no canal de efeitos, **Tocar em loop**, **Parar som**, **Volume do canal** e **Ler volumes**.
  - A música segue sem recomeçar ao mudar ou reiniciar a cena e cruza com a seguinte. Os loops dos efeitos acabam com a cena, e os volumes valem até o fim da partida.
  - A pausa do jogo congela os efeitos e mantém a música; a pausa da engine congela tudo; **Parar** silencia o teste.
  - Sem dispositivo de som, o jogo segue calado e tenta de novo no próximo **Jogar** ou **Ouvir**.
- **Nós novos:** as categorias **Jogo** (variáveis, progresso, cena atual, pausa, cursor e sair) e **Som** entram no catálogo.
  - **Matemática e texto:** **Função matemática** (piso, teto, arredondar, truncar, valor absoluto, sinal e raiz quadrada), **Limitar entre mínimo e máximo** e **Formatar tempo**, que escreve segundos como `01:23.45`, cortando as casas e com ponto, em seis formatos. **Operação aritmética** ganha resto, mínimo e máximo.
  - **Cenas:** **Cena atual** (Jogo) e **Referência de cena** (Dados) fornecem o identificador e o nome de uma cena, e **Mudar de cena** ganha a entrada **Cena**. Assim, uma lógica de reiniciar ou de seguir para a próxima cena funciona copiada ou colocada a partir de um modelo, sem ID fixo.
  - **Sortear número** e **Definir semente aleatória** (Ações); **Mover / girar / escalar objeto** (Transformação 3D), que define ou soma posição, rotação ou escala de qualquer objeto; **Mostrar / esconder cursor** (Jogo), que solta o mouse sem pausar o jogo.
  - **Mensagem de diagnóstico** ganha a entrada **Valor**, para registrar um valor calculado. A falha de uma ação que nenhum nó lê escreve uma linha no log, uma vez por nó.
- **Faixas de números:** os parâmetros numéricos que o jogo recusa ou limita (a busca do teleporte, volumes, transições, esperas, semente, dano, consultas) declaram mínimo e máximo no catálogo. No editor, o campo só arrasta e aceita números dentro da faixa. O MCP recusa números fora dela, e o `oxy_validate` e a conferência da exportação avisam dos projetos antigos que os têm.
- **Plataformas que giram carregam o personagem:** uma plataforma móvel que gira, pela animação ou pela lógica, leva quem está em cima em volta do eixo dela e o gira junto. A câmera em primeira pessoa acompanha; a de terceira, não. Isso reverte o contrato da 0.3.0, em que qualquer giro soltava o apoio com aviso: agora só um giro de mais de 20° num passo, uma mudança de escala ou um salto soltam. No cartão **Plataforma móvel**, **Seguir animação** passa a se chamar **Seguir animação ou lógica**.
- **Física dos personagens:** o personagem não afunda mais no meio de pisos e caixas girados, nem no eixo de uma plataforma que gira.
  - O contato entre duas cápsulas é exato, e **Teleportar com segurança** não põe mais um personagem dentro de outro: o corpo de outro personagem conta como ocupado quando os grupos de colisão dos dois se aceitam.
  - O teleporte mantém as esperas da lógica que já começou. Antes, uma área que escondia um objeto e o mostrava depois de uma espera deixava de mostrá-lo se quem entrou nela fosse teleportado no meio da espera.
- **Colisores, áreas e plataformas 3D pela lógica:** **Ativar / desativar componente** liga e desliga também o colisor 3D, sólido ou área, e ganha a escolha **Plataforma móvel**. Uma área religada abre outra ativação, e quem está dentro entra de novo.
- **Interface do jogo:** **Alinhamento** (esquerda, centro ou direita) e **Tamanho do texto** (automático ou de 10 a 96 px) no cartão **Interface do jogo**. Uma **Imagem** sem imagem desenha um retângulo na Cor do elemento, com a transparência dela, para fundos de menus; no editor, um contorno tracejado indica a imagem ausente.
- **Fundo da cena:** o bloco **Cena aberta**, nas Propriedades sem seleção, escolhe o fundo: o **Céu da OXY**, o céu de dia estilizado do pacote Skyboxes da Kenney (CC0, licença em `licenses/`), ou uma **Cor**. Vale na vista, no teste, no jogo exportado e nas capturas. As cenas 3D novas começam com o céu; as 2D e as já existentes continuam com o fundo escuro.
- **Exportar jogo:** **Projeto → Exportar jogo…** cria uma pasta com `<Nome do jogo>.exe`, os dados, as licenças e um `LEIA-ME.txt`. A exportação abre e roda o jogo para conferi-lo antes de pô-lo no lugar, e só então substitui uma exportação anterior do mesmo jogo. O título, a janela e a tela cheia ficam no projeto. O player abre com o nome do jogo e o ícone da OXY, alterna a tela cheia com Alt+Enter e se confere sem janela com `--verificar`, que o `test-portable.ps1` também roda.
- **OXY Parkour:** um jogo completo em primeira pessoa, em `examples/parkour`, montado só pelo MCP com os nós e componentes da engine: menu, pista com plataformas móveis e que giram, ladrilhos que caem, cronômetro, pausa, recorde salvo por fase e música gerada por script. Ele abre como cópia pela tela inicial e vai no ZIP, na pasta `parkour`. O estado de cada jogador fica no próprio personagem, pensando no online da 0.7. O Guia ganha as receitas **Menu de pausa** e **Recorde salvo** (catorze no total).
- **MCP:** o parkour foi construído e conferido só pelo MCP, e cada atrito virou uma melhoria genérica.
  - **Ferramentas novas:** `oxy_new_project` cria um projeto como o **Novo projeto**, numa pasta nova ou vazia que a pessoa indicar. `oxy_play` inicia, pausa e para um teste no editor e manda entradas a ele, sem tomar o mouse; a barra mostra **Agente jogando**, e **Assumir** devolve o teste à pessoa. `oxy_save` salva ao vivo só com **Permitir que agentes salvem**, desligada por padrão, guardando antes o `project.oxy.json.agente.bak`. `oxy_export` exporta o jogo para a pasta que a pessoa indicar. São 20 ferramentas.
  - **Nomes em todo lugar:** cenas e objetos pelo nome em todas as ferramentas, nos comandos e nos nós de cena. O resumo da lógica nomeia ações, animações, cenas, variáveis, sons e objetos e distingue nós de mesmo rótulo pelo valor que muda. O `oxy_entity` diz quem aponta para o objeto (`referenced_by`), e as recusas listam as cenas ou os objetos possíveis.
  - **Bot:** o `oxy_simulate` e o `oxy_screenshot` levam um personagem 3D por um caminho de pontos, apertando pular, correr e agachar como uma pessoa. O bot assume quando a cena dele roda (depois de um menu, por exemplo), espera plataformas móveis com `wait_for` e recomeça a cada recarga da cena. Ao desistir, diz se o personagem caiu, ficou bloqueado, foi travado pela lógica ou esperou demais, e onde pisou por último.
  - **Lotes:** `connect` e `disconnect` com várias ligações em `links`, e `add_node` já ligado. A lógica criada sem posição é organizada no fim do lote, e o `arrange_logic` com `split_shared_data` evita fios longos. O `fit_collider` ganha `shape` (caixa, esfera, cápsula, convexo ou malha); `"$N@<ID>"` edita no mesmo lote as cópias de objetos, nós e animações; há `save_model` com `replace`, `rename_asset`, `delete_asset`, filtros de colisão pelo nome dos grupos e comandos para variáveis, fundo da cena e jogo exportado.
  - **Testes e relatórios:** `until.then_seconds` continua o teste depois da condição. O `click` aceita um ponto da vista, e o `ui` diz o que cobre cada elemento (`covered_by`). As capturas desenham a interface do jogo, e a vista tem o tamanho da janela do jogo. Os sons trazem o canal e o volume efetivo; há `sample_every` e `samples_at`; o relatório traz variáveis, progresso, pausa, cursor e o que toca.
  - **Catálogo:** cada nó tem descrição própria, com as unidades das saídas, e o catálogo diz quando os dados são lidos e a faixa de cada número.
  - **Determinismo:** `seed` decide os sorteios, e as cópias de **Criar cópia de objeto** recebem os mesmos IDs a cada execução. O `state_hash` resume o estado no fim, e `check_replay` roda duas cópias do jogo lado a lado e diz onde se separaram. O teste do editor e o jogo exportado começam com uma semente nova; a do teste aparece no `oxy_status`.
  - **Arquivos:** o `project.oxy.json.agente.bak` guarda o projeto de antes da primeira gravação de cada sessão do servidor e fica fora do repositório da OXY, da exportação do jogo e dos pacotes da engine; num projeto seu com Git, ignore-o também. O `oxy_import` aceita vários arquivos de uma vez, e o `oxy_status` diz a versão e a compilação do servidor.
  - Respostas que mudaram de formato estão listadas em [`docs/agentes-mcp.md`](docs/agentes-mcp.md).
- **Testes mais rápidos:** os testes de integração do `oxy_core` viram um binário só, e o perfil de testes guarda só arquivo e linha como informação de depuração. Recompilar os testes caiu de ~106 s para ~26 s, e rodá-los, de ~25 s para ~6 s.
- **O que muda em projetos existentes:**
  - Plataformas que giram passam a carregar quem está em cima (veja acima).
  - **Reproduzir WAV** está na categoria **Som**, com os mesmos parâmetros. Um nó de som sem áudio escolhido diz isso no erro.
  - **Ler tempo da simulação** e as esperas contam o tempo do jogo, que para na pausa do jogo; sem pausa, é igual ao de antes.
  - Uma animação terminada ou pausada deixa de repor a pose a cada passo. **Ler posição e orientação** no Mundo dá o pivô, como as Propriedades; num personagem 3D, continua dando a origem.
  - A câmera **Fixa na vista atual** fica onde a vista está, em vez de saltar para o objeto câmera; o salto antigo é **Fixa no objeto câmera**.
  - Um clique num elemento da interface oculto não chega à lógica, e um clique num botão com o mouse livre não aciona também a ação ligada ao botão do mouse.
  - Numa cena com **Ao pausar ou retomar**, Esc abre o menu do jogo em vez de pausar a engine.
  - Uma **Imagem** sem imagem aparece no jogo como um retângulo de cor, sem o aviso roxo.
  - O perfil de movimento Parkour se chama **Ágil com embalo**; o identificador continua `parkour`.
  - **Parar** silencia os sons do teste, e um novo **Jogar** começa em silêncio.
  - Exportar um jogo passa a ser pelo editor (ou pelo `oxy_export`). O `scripts/package.ps1` fica como empacotador da engine.

## 0.5.1 — 2026-10-12

- **MCP pronto para o parkour:** um agente constrói e confere o próprio trabalho sem adivinhar.
  - **Simulação:** as entradas programadas sobrevivem à troca e ao reinício de cena, e o relatório marca cada recarga. As ações vêm resumidas por padrão (objeto, ação, quantas vezes, quando), e os logs trazem o objeto e o nó, sem IDs. O relatório tem a velocidade, o chão, a postura e o olhar do personagem; a câmera do jogo; os eventos de pulo, aterrissagem, saída do chão, superfície e áreas; e a interface do jogo no fim. `setup` e `spawn` preparam o teste só na cópia, e `until` para o teste quando algo acontece.
  - **Capturas:** colisores e sensores visíveis, câmera livre e vistas de cima e de lado. As vistas do jogo listam a interface do jogo, que a imagem não desenha.
  - **Consultas compactas:** posições, rotações e escalas no mundo e caixas envolventes; projeção de campos; paginação da árvore da cena; catálogo filtrável; números arredondados. O `oxy_find` responde em páginas.
  - **Comandos novos:** ações de entrada (criar, renomear, trocar tecla, excluir), superfícies físicas e nomes de grupos de colisão; salvar e colocar modelos; copiar objetos entre cenas; duplicar em série; criar e editar animações e quadros-chave; aplicar perfis de movimento. Os presets se descrevem no catálogo, e os de personagem escolhem o que fazer com a câmera.
  - **Validação:** os parâmetros dos nós aceitam só as opções válidas, e o `oxy_apply` devolve avisos, por exemplo uma ação de entrada que o projeto não tem. Os marcadores aceitam `"$N.M"`, valem dentro de `patch` e usam `"$$"` como escape.
  - **`oxy_import`:** importa PNG e WAV como o Importar da Biblioteca, conferindo o conteúdo e copiando só para a pasta `assets`. Ao vivo, a importação entra no histórico do editor.
  - **Editor ao vivo:** `oxy_console` lê o Console com a origem de cada linha, e `oxy_status` informa se o jogo está em teste.
  - Respostas que mudaram de formato estão listadas em [`docs/agentes-mcp.md`](docs/agentes-mcp.md).
  - **Ajustes do teste prático:**
    - `fit_collider` ajusta o colisor à forma, como "Ajustar ao objeto", e o `oxy_apply` avisa quando o tamanho muda e o colisor fica para trás.
    - Nós de entrada aceitam a ação pelo nome.
    - Duas ações na mesma tecla geram aviso.
    - Valores simples (`3`, `"texto"`, `true`) valem quando o tipo do parâmetro é conhecido, e os erros dizem qual parâmetro e o formato esperado.
    - Os erros numeram os comandos a partir de 0, como os marcadores.
    - Modelos não repetem nome.
- **Som antes de trocar de cena:** um som pedido no mesmo passo de uma troca ou reinício de cena (por exemplo "tocar som e reiniciar a fase") era descartado e agora toca.
- **Aviso de plataforma:** o aviso de apoio liberado por uma plataforma que girou ou saltou sai uma vez por personagem e plataforma, com o nome da plataforma. Antes ele saía a cada início de cena, mesmo sem ninguém em cima.
- **Mover objetos animados:** mover, girar ou escalar um objeto cujas próprias animações o movem, como uma plataforma móvel, agora leva junto os quadros-chave, fora da aba Animação. Assim a plataforma continua o percurso a partir do lugar novo, em vez de voltar ao antigo durante o jogo.

## 0.5.0 — 2026-10-12

- **Agentes de IA (MCP):** o novo `oxy-mcp.exe` é um servidor MCP para agentes como o Claude. Pelas ferramentas, o agente lê o projeto (resumo, árvore da cena, objeto, busca, catálogo de nós e componentes), edita por lotes atômicos de comandos, valida, roda o jogo sem janela com entradas programadas e captura a vista do editor ou da câmera do jogo. Com o projeto aberto e **Permitir agentes de IA** ligado nas Preferências (desligado por padrão), as mudanças aparecem ao vivo, entram no histórico como **Agente: …**, aparecem no Console com a origem Agente e podem ser desfeitas; o agente não salva. Sem editor aberto, ele edita os arquivos e guarda a versão anterior. Veja [`docs/agentes-mcp.md`](docs/agentes-mcp.md).
- **Camada de comandos:** edições descritas como dados, aplicadas em lote atômico e validadas com as regras de salvar, no `oxy_core`. A regra de quais componentes cabem em cada objeto e as descrições dos nós passaram do editor para o `oxy_core`, compartilhadas pelo painel e pelos comandos.
- **Simulação e captura sem janela:** `oxy_core::headless` roda o jogo por um tempo simulado e relata logs, ações, sons, trocas de cena e objetos observados; `oxy_render::offscreen` desenha sem janela e gera PNG.

- **Janelas restantes no visual novo:** Excluir recurso e Animação em uso viraram diálogos no padrão dos demais, com a lista de usos pelo nome do objeto (sem IDs de nó). Desempenho mostra quadro, cena, memória e lógica em tabelas; Prévia da câmera e Ajustar caixa usam os mesmos controles e botões do editor.
- **Console:** cada linha indica a origem (Editor, Jogo, Lógica) e os avisos ficam destacados. Os traços de Lógica citam objeto e ação, sem o ID do nó.
- **QA nativa mais estável:** o redimensionamento espera o tamanho real da janela, e a medição de repouso ignora eventos reais do sistema (ponteiro, foco).
- **Guia do editor:** tela inicial, Propriedades sem seleção e com vários objetos, Preferências do editor, Console e Desempenho atualizados.
- **Roadmap:** [`docs/roadmap.md`](docs/roadmap.md) registra os jogos-alvo, a ordem das próximas versões, o servidor MCP e o terreno do Shadow Duel.

## 0.4.2 — 2026-10-11

- **Tela inicial no visual novo:** barra superior e barra de status iguais às do editor, cartões para **Novo projeto** e **Abrir projeto**, um bloco **Exemplos** com a explicação de cada exemplo, e **Projetos recentes** num bloco com busca, há quanto tempo cada projeto foi salvo, aviso de arquivo não encontrado e as ações (Abrir, Remover da lista) no "⋯". Em janelas estreitas as colunas se empilham.
- **Novo projeto:** a primeira cena (2D ou 3D) é escolhida em dois cartões lado a lado, com ícone e marca de seleção.
- **Diálogos no mesmo padrão:** cabeçalho com fechar e a ação principal à direita do rodapé em Nova cena, Renomear cena, Excluir cena, Alterações não salvas e Preferências do editor (antes "Escala da interface"). Excluir cena explica por que não pode excluir e lista os nós pelo nome do objeto, sem identificadores; em Alterações não salvas, "Descartar alterações" fica separado de "Salvar e continuar".
- **Propriedades sem seleção e com vários objetos:** sem seleção, um bloco Projeto com nome, arquivo, cena inicial e conteúdo, e uma indicação de como começar; com vários objetos, cabeçalho com os nomes, visibilidade de todos e o bloco Transformar o conjunto em linhas alinhadas.
- **Forma em prévia e Encaixar vértices:** no mesmo bloco das operações em prévia, com Cancelar/Aplicar; o encaixe mostra os passos Origem e Destino. Clicar fora da prévia de forma continua aplicando.
- **Ajuda da modelagem:** tópicos com tabela de atalhos no lugar dos parágrafos.
- **Borda interna sem fragmentar a face:** a face interna sai como uma face só, com um quad de moldura por aresta, e as faces vizinhas não ganham vértices. Antes, cada triângulo interno da face (por exemplo, a base de um cone) virava um pedaço próprio, o que também fazia o arredondamento recusar arestas próximas. Faces com UVs que não são lineares no plano continuam divididas para preservar a pintura.
- **Arrastar valores das operações:** arrastar Largura em Arredondar (ou outro campo do painel da operação) não passa mais para o campo de baixo quando a dica do painel desaparece.

## 0.4.1 — 2026-10-10

- **Estúdio no visual novo:** Modelagem, Pintura e Animação usam as duas linhas de cabeçalho, o chip de status no viewport e blocos em Propriedades (Malha, Textura, operação em prévia com Cancelar/Aplicar). A Animação ganhou a linha do tempo no painel inferior, com trilhas por peça, régua adaptável, barra em duas linhas em janelas estreitas e as ações da animação no próprio seletor.
- **Lógica redesenhada:** barra com o objeto e a contagem do grafo, catálogo **Adicionar nó** por categorias (ou menu, em janelas estreitas), nós com setas de execução e resumo dos parâmetros, legenda de portas, resultado de **Validar** sobre o canvas e sugestões de início para objetos sem lógica. O nó selecionado é editado em Propriedades, com Parâmetros e Conexões. **Ações de entrada** virou uma tabela com "Usada por" e exclusão confirmada na linha; o **Guia** tem navegação lateral com receitas e nós por categoria.
- **Mensagens sem identificadores:** validação, conexões recusadas e referências de ações citam nós e objetos pelo nome, e todo nó registrado tem descrição.
- **Hierarquia:** na Lógica, objetos com nós mostram a quantidade de nós.
- **Propriedades:** componentes em linhas alinhadas, com o rótulo à esquerda e o campo à direita. No Colisor 3D, uma única escolha de **Forma** (Caixa, Cápsula, Esfera, Convexo da peça, Malha estática da peça) substitui os botões de gerar, **Tipo** Sólido | Área substitui a caixa de seleção e os grupos de colisão ficam numa subseção recolhida; Personagem 3D, Corpo de movimento, Controle da câmera, Plataforma e os componentes 2D seguem o mesmo padrão. O painel não cresce mais sozinho, e um componente recém-adicionado abre o próprio bloco.
- **Fonte:** IBM Plex Sans e IBM Plex Mono embutidas no editor e no player, sob a licença OFL 1.1 (em `licenses/`).

## 0.4.0 — 2026-10-09

- **Nova estrutura da tela:** barra superior única (projeto, desfazer/refazer e salvamento; modos Cena · Estúdio · Lógica no centro; Jogar/Pausar/Parar à direita), painel inferior com abas Biblioteca, Console e Avisos, e barra de status. A aba Jogo deixa de existir: o teste aparece na área central.
- **Hierarquia:** seletor de cena e busca no topo, ícone por tipo de objeto e chevrons desenhados.
- **Propriedades:** cabeçalho do objeto e blocos recolhíveis com campos alinhados; interruptor de ativo no cabeçalho e Remover componente no menu ⋯.
- **Estúdio isolado:** o objeto selecionado e seus filhos aparecem sozinhos, com câmera própria enquadrada nele. A Hierarquia mostra só a peça em edição e **Voltar à cena**.
- **Biblioteca:** cartões compactos; Localizar na biblioteca filtra pelo nome do recurso.

## 0.3.4 — 2026-10-09

- **Aparência:** tema neutro com um único destaque azul dessaturado, aplicado ao editor e ao player. Barras e botões de ícone mais compactos (24 px), espaçamentos e fonte reduzidos, viewport com fundo e grade neutros. As cores da interface passam a vir de tokens em `oxy_render::theme`.
- **Repositório:** testes nativos e medições gravam capturas em `target/qa/` (ou `OXY_QA_DIR`), fora do Git. A pasta `qa/` guarda só as evidências da versão atual; as anteriores ficam nas tags.
- **Erros tipados:** `migration` e `persistence` devolvem `MigrationError` e `PersistenceError` (thiserror), com as mesmas mensagens de antes. Quem chama pode distinguir, por exemplo, versão incompatível, asset ausente e salvamento revertido; `?` continua convertendo para `String` onde a interface só exibe o texto.
- **Código do editor:** a struct `Editor` passou de 59 para 41 campos. Opções de visualização, sessão de jogo, hierarquia, biblioteca, transformação, console e tempos de quadro viraram sub-estados próprios, sem mudança de comportamento.
- **Documentação:** README curto; fluxo de trabalho, atalhos e domínio suportado movidos para o [guia do editor](docs/guia-do-editor.md).

## 0.3.3.1 — 2026-09-13

Corpo de movimento alinhável por deslocamento local X/Y/Z, com campos e gizmo, sem mover a origem nem a aparência. No viewport 3D, RMB + **E/C** sobe/desce no eixo global Y. [Relatório](docs/v0.3.3.1.md).

## 0.3.3 — 2026-09-12

Navegação livre no viewport 3D: **RMB + mouse + WASD**, Shift rápido, Ctrl preciso e roda para ajustar a velocidade. Soltar RMB devolve W/E/R às ferramentas. A vista não altera o projeto, o histórico nem a câmera de jogo. [Relatório](docs/v0.3.3.md).

## 0.3.2 — 2026-09-12

Corpo de movimento configurável (cápsula, caixa, esfera e convexo), guias editáveis de câmera e prévia nativa sem iniciar o jogo. Jogador e Câmera continuam separados; a cápsula da 0.3.1 é migrada para o novo corpo. [Relatório](docs/v0.3.2.md).

## 0.3.1 — 2026-09-11

Inspetor refinado e autoria por objetos explícitos: componentes compatíveis em **+ Adicionar componente**, seções recolhíveis e câmera independente com controle vinculado ao personagem. [Relatório](docs/v0.3.1.md).

## 0.3.0 — 2026-09-11

Movimento 3D: consultas de forma com Rapier/Parry, controlador em primeira pessoa com entrada relativa, motor persistente com superfícies, postura e plataformas, perfis de parkour (pulos encadeados, deslize, sensores varridos), terceira pessoa protegida, nós de movimento tipados e o Laboratório 3D editável. [Relatório](docs/v0.3.0.md).

## 0.2.1 — 2026-09-09

Modelagem direta: gestos que aplicam um único comando de histórico, ajuste da última operação, borda interna com preservação de UV, extrusão pela normal e overlays de componentes em buffers GPU reutilizáveis. [Relatório](docs/v0.2.1.md).

## 0.2.0 — 2026-09-09

Malhas poligonais editáveis com criação paramétrica, extrusão, criação de face/aresta, alinhamento de vértices, corte em loop, bisturi e arredondamento. Pintura em malhas modeladas, ações de entrada, guia offline com receitas, migração segura de schema, avisos, escala de interface e gerenciamento de cenas. [Relatório](docs/v0.2.0.md).

## 0.1.3 — 2026-09-08

Otimizações com harness determinístico: índices por fase, transformações e grafos reutilizados, filtros de colisão antecipados. Distribuição como aplicativo portátil para Windows. [Medições](benchmarks/README.md).

## 0.1.2 — 2026-09-07

Edição de colisores e pivôs preservando animações existentes.

## 0.1.1 — 2026-09-07

Refinamentos de UX e desempenho.

## 0.1.0 — 2026-09-07

Primeira versão: editor e player em Rust com cenas 2D/3D, pintura PNG, animação rígida e lógica visual.
