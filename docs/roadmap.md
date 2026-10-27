# Roadmap da OXY Engine

Plano combinado em 11/10/2026, depois do lançamento da 0.4.2, e reordenado em 17/10/2026, quando a camada de scripts entrou antes da rede. Ele guia as próximas versões a partir dos jogos que a engine precisa suportar. Atualize este arquivo quando uma etapa terminar ou a ordem mudar.

## Jogos-alvo

| Jogo | Complexidade | O que exige da engine |
| --- | --- | --- |
| Parkour e plataforma | simples | ciclo de jogo completo: menu, fases, transição de cenas, cronômetro, melhor tempo salvo, pausa, som, exportação |
| FPS em arena fechada | simples | disparo por raio, vida e dano, inimigos com busca de caminho, ondas, HUD de combate |
| Automação e gerenciamento de recursos, com bases e tower defense | médio/avançado | centenas de entidades, grade de construção, economia, muita interface, busca de caminho em escala; aqui otimização passa a valer a pena |
| Shadow Duel, port para dentro da engine | avançado (lógica) | núcleo de regras e IAs em Rust, cartas como dados, determinismo, modo online, interface 2D de cartas |
| RPG de ação / soulslike | avançado | mistura de animações, máquinas de estado de combate, trava de alvo, estamina, inimigos com padrões |

Cada jogo constrói sistemas que o seguinte reaproveita. Combate, IA e animação passam pelo FPS antes de chegar ao soulslike.

## Ordem

1. **0.4.3: consolidação** (lançada junto com a 0.5.0)
   - Investigar os testes nativos instáveis: `native_navigation_v033` (repintura com o editor parado) e `native_layout_matrix_workflow` (a janela não redimensiona para 920×600).
   - Levar ao visual novo as telas que faltam: Diagnóstico de desempenho, Console (distinguir erro, aviso e log), Excluir recurso, Animação em uso, Prévia da câmera e Ajustar caixa.
   - Conferir se o guia do editor descreve as telas atuais.
2. **0.5: camada de comandos, runtime headless e servidor MCP** (lançada na 0.5.0, em 12/10/2026; veja [Agentes de IA (MCP)](agentes-mcp.md))
   - **Camada de comandos:** operações sobre o projeto com validação e desfazer (criar objeto, mudar componente, ligar nós, criar cena…). Editor, MCP, testes e uma futura camada de script usam a mesma porta.
   - **Runtime headless:** rodar a simulação do jogo sem janela por um tempo e devolver logs, estado e erros. Serve ao MCP, aos testes, aos bots e a um servidor online.
   - **Servidor MCP:** para agentes de IA ajudarem no desenvolvimento de jogos. Ele trabalha sobre os arquivos do projeto e **também se conecta ao editor aberto e age ao vivo**: as mudanças aparecem na hora, entram no histórico e podem ser desfeitas. Para isso, o editor expõe um canal local. Primeira versão:
     - ler o projeto;
     - editar cenas, objetos, componentes e lógica;
     - validar;
     - rodar o jogo sem janela;
     - capturar telas.
3. **0.5.1: MCP pronto para o parkour** (lançada em 12/10/2026). Uma análise com agentes (12 de outubro de 2026) montou um trecho de pista pelo MCP e conferiu 88 lacunas no código. Antes da 0.6 entram as que impedem um agente de construir e conferir o próprio trabalho:
   - **Simulação confiável e legível:**
     - entradas que sobrevivem à troca de cena;
     - resumo das ações e logs com nomes;
     - velocidade, chão, postura e câmera do personagem, com eventos de pulo, aterrissagem e áreas;
     - preparar o teste numa cópia e parar quando algo acontecer;
     - estado da interface do jogo.
   - **Enxergar o nível:** posições no mundo e caixas envolventes; capturas com colisores e de qualquer ângulo.
   - **Construir sem adivinhar:**
     - opções válidas no catálogo, recusando valores fora delas;
     - ações de entrada, superfícies e grupos de colisão por comando;
     - presets e perfis que se descrevem;
     - modelos entre fases e duplicação em série;
     - plataformas móveis por animação;
     - importar PNG/WAV.
   - **Contexto e diagnóstico:** respostas compactas; Console e estado do teste no editor ao vivo.
4. **0.6: parkour** (lançada em 17/10/2026; o resumo está no [CHANGELOG](../CHANGELOG.md)). Feito com ajuda do MCP, que serve de teste do próprio MCP. Lacunas da engine que o parkour exige, encontradas na mesma análise, todas resolvidas com ferramentas genéricas:
   - dados salvos entre partidas (melhor tempo);
   - estado que sobrevive à troca de cena;
   - menu de pausa controlado pelo jogo;
   - ligar e desligar colisores, sensores e plataformas 3D pela lógica;
   - som em loop e canais de música/efeitos;
   - formatação de cronômetro (mm:ss.cc) e mais matemática (piso, resto, mín/máx);
   - determinismo: gerador aleatório semeado, IDs estáveis para objetos criados no jogo e checagem de replay;
   - transformar objetos pela lógica e plataformas que giram carregando o jogador;
   - exportar o jogo pelo editor.

   Do lado do MCP, durante a 0.6 entraram as melhorias que o uso real pediu: iniciar e parar o teste no editor sem tomar o mouse da pessoa, um bot que segue um caminho (também em cenas abertas depois do menu, esperando plataformas), interface do jogo desenhada nas capturas, lógica compacta com layout legível, colisores a partir da forma da peça, salvar ao vivo opcional (preferência própria, desligada), criar projetos e exportar o jogo, cenas e objetos pelo nome em todas as ferramentas e relatórios que dizem por que o teste parou.

   **Online (decidido em 13/10/2026):** o parkour terá o modo online como foco, com corrida em tempo real e colisão entre jogadores. A 0.6 sai offline e já monta o parkour pensando no online: o estado de cada jogador fica no próprio personagem, e as variáveis do jogo guardam só o estado da partida e dos menus. O online vinha na 0.7; em 17/10/2026 passou para a 0.8, depois da camada de scripts.

   **Princípio:** o parkour só mostra o que falta. A engine ganha ferramentas genéricas, que qualquer jogo usa; nada de checkpoint, linha de chegada ou "melhor tempo" embutidos. O jogo é conteúdo feito com essas ferramentas, em `examples/parkour`.

   **Decisões (12/10/2026):**
   - **Formato do projeto 5**, uma vez só, com migração que preenche parâmetros novos dos nós com o padrão e backup no primeiro salvamento. A 0.5.x não abre projetos salvos pela 0.6.
   - **Variáveis do jogo** declaradas no projeto (nome, tipo, valor inicial, "Salva entre partidas"), com chave opcional nos nós, por exemplo um valor por fase. Botão "Variáveis do jogo" na barra da Lógica.
   - **Progresso salvo** em `%APPDATA%\OXY Engine\Jogos\<id do projeto>\progresso.json`. No teste do editor, o progresso fica em memória durante a sessão.
   - **Catálogo:** categorias novas **Jogo** (variáveis, progresso, cena atual, pausa, sair) e **Som** ("Reproduzir WAV" vai para Som). Formatar tempo corta as casas e usa ponto (01:23.45).
   - **Pausa:** a pausa do jogo (menu feito na lógica) e a pausa da engine são separadas e podem coexistir. O Esc abre o menu do jogo quando a cena tem o evento "Ao pausar ou retomar", no player e no teste do editor; perder o foco sempre aciona a pausa da engine, com a janela "Jogo pausado". "Sair do jogo" fecha o player e para o teste do editor.
   - **Interface:** sem tipo novo; a Imagem de interface sem imagem passa a desenhar um retângulo na Cor do elemento, para fundos de menus.
   - **Áudio:** cpal para o dispositivo, com mixador próprio (loops, transições, canais de música e efeitos).
   - **Plataformas que giram** passam a carregar o personagem (posição e giro), revertendo o contrato da 0.3.0; a câmera em primeira pessoa gira junto.
   - **Exportação** pelo editor e pelo MCP (`oxy_export`, só num destino que a pessoa indicar). A janela do jogo usa o nome do jogo e o ícone da OXY; Alt+Enter alterna a tela cheia; ícone próprio do jogo fica para depois.
   - **Agentes:** a barra do teste mostra "Agente jogando" com o botão "Assumir"; salvar ao vivo é uma preferência própria, desligada, que guarda um backup antes.
   - Protótipos aprovados: Lógica e Variáveis do jogo (https://claude.ai/artifact/DpTXojrNpJKgH7a31ZUWsM); teste, pausa e exportação (https://claude.ai/artifact/RGSjLXekAJGumKdV9YzdMz), com as mudanças acima.

   **Ordem de trabalho:**
   1. Fundação: referências dos nós pelo catálogo, estado da partida no runtime e ordem determinística dos eventos (feito).
   2. Formato 5, matemática e Formatar tempo, colisores e plataformas pela lógica (feito).
   3. Cena atual e entrada Cena em Mudar de cena, Referência de cena, sorteio com semente, IDs estáveis e checagem de replay (feito).
   4. Variáveis do jogo (núcleo, MCP e editor) e progresso salvo (feito).
   5. Pausa do jogo, mixador e som em loop, transformações pela lógica e plataformas que giram (feito), com a correção do personagem que afundava no meio de pisos grandes.
   6. MCP: interface do jogo nas capturas, layout automático da lógica, colisores pela forma e cópias no mesmo lote, bot que segue um caminho, teste e salvamento ao vivo (feito).
   7. Exportação, o parkour (offline, pronto para o online), guia e receitas (feito).
5. **0.7: camada de scripts em Luau** (lançada em 27/10/2026; o resumo está no [CHANGELOG](../CHANGELOG.md)). Decidida em 17/10/2026: o grafo de nós pedia um nó novo para cada ideia e um planejamento de cada passo, e o Luau passa a ser o caminho principal da lógica nos cinco jogos, com o Rust para a engine e para os módulos pesados, como as regras e as IAs do Shadow Duel. Ferramentas genéricas, já pensadas para a rede da 0.8:
   - scripts em Luau (pelo `mlua`), escritos por um agente de IA pelo MCP ou por uma pessoa no VS Code, com a extensão luau-lsp, e anexados aos objetos, com erros que dizem o arquivo e a linha e recarga durante o teste do editor;
   - nenhum estado do jogo dentro do Luau: atributos, a memória que cada script declara, variáveis do jogo, temporizadores e mensagens ficam na engine, para a recarga, os saves e a correção (rollback) da rede;
   - a API v1, gerada de uma tabela única (175 funções com nomes em inglês) e com tudo o que os nós fazem; da mesma tabela saem os tipos do VS Code, a referência e as páginas do Guia;
   - a árvore do projeto: a Biblioteca mostra as pastas reais do disco, com arquivos de nomes legíveis e modelos como arquivos, e os projetos antigos são organizados em pastas no primeiro salvamento, com backup;
   - o parkour reescrito em Luau, conferido contra a versão em nós, e três receitas em script.

   **Decisões (18 a 24/10/2026):**
   - **Esperas** por temporizador e sequência (`self:after`, `self:sequence`); `wait()` com corrotina só em scripts de interface, na 0.8, nunca nos da partida.
   - **Dados próprios:** o que a interface, o grafo ou outro objeto leem vai em atributo; o resto, na memória do script, guardada pela engine.
   - **Erros:** só a chamada que errou para; o erro vai ao Console com a linha, uma vez e com contador, e um laço sem fim ou um limite estourado desliga o script naquele objeto. Igual no editor e no jogo exportado.
   - **Nomes da API** em inglês com sublinhado (`scene.find`, `on_area_enter`); nomes de objetos, atributos e sons, as mensagens de erro e o Guia continuam em português.
   - **Parâmetros:** o script declara os atributos que usa, e a OXY os cria ao anexá-lo, num passo de desfazer.
   - **Quem escreve:** a pessoa traz as ideias, e os scripts são escritos por agentes de IA; por isso as ferramentas MCP têm prioridade (erros claros, ferramentas completas), e a interface serve para jogar, ler e ver.
   - **Interface da 0.7.0:** cartão Scripts nas Propriedades, código numa aba só de leitura do painel de baixo, scripts e pastas na Biblioteca, Console com o erro clicável, selo "Nós (legado)" na Lógica, seção Scripts no Guia e a opção de continuar o teste fora de foco. Protótipo aprovado: https://claude.ai/artifact/WSNseQHUyhsn6YrjpxU2ad.
   - **Árvore do projeto (20/10/2026):** uma árvore só, com as pastas reais do projeto; nome único por tipo no projeto inteiro; "Descartar alterações" desfaz também as operações de arquivo; um agente pode organizar sozinho um projeto antigo em pastas, com backup, contando o que mudou.
   - **Tabelas de conteúdo** como módulos Luau travados, lidos com `require`; arquivos de dados com editor só quando o Shadow Duel pedir.
   - **Rust por jogo:** decidido no porte do Shadow Duel; a 0.7 só reserva `require("@native/...")`, com um erro claro.
   - **Grafo de nós:** escanteado, não apagado. Nenhum nó novo; os projetos com nós continuam funcionando, e as receitas em nós ficam no Guia, em "Nós (legado)".

   **Para a 0.7.1:** painel de código no editor, "Configurar VS Code", Console completo, inspetor da memória dos scripts no teste, Guia dividido, tela inicial com o parkour em scripts primeiro, "Tocar ao iniciar" nas animações, scripts no Desempenho, "Criar atributos que faltam", o luau-lsp no pacote para conferir os tipos e as receitas 01, 05, 13 e 14 em script.
6. **0.8: rede e parkour online.** Corrida em tempo real com colisão entre jogadores. Ferramentas genéricas, que o FPS e o Shadow Duel reaproveitam:
   - servidor com autoridade sobre a física: um `oxy-server` sem janela, sobre a simulação headless, e o mesmo servidor rodando dentro do jogo de quem hospeda;
   - entradas por jogador (cada personagem controlado por um jogador);
   - snapshot e restauração do estado da partida, para previsão no cliente e correção (rollback), com o estado dos scripts, que já fica na engine;
   - salas, conexão e replicação de objetos;
   - scripts com vários jogadores: cada função da API já diz onde vai rodar; entram `players`, `session`, `on_player_join`, `on_player_leave` e `on_request` (nomes já reservados na 0.7), o `audience` dos sons e as corrotinas dos scripts de interface. Se o grafo de nós fica só offline se decide no desenho da 0.8.

   **Decisões (13 e 17/10/2026):**
   - **Previsão:** cada máquina adivinha a partida inteira e corrige quando o servidor responde. Se o protótipo medir, no PC de entrada a 100 ms com 8 jogadores, um refazer pior que 1 passo, mais de ~40 % de um núcleo ou mais de ~3 Mbit/s de subida de quem hospeda, a previsão passa a ser só do próprio personagem (troca por configuração).
   - **Alvo:** PC de entrada (notebook de 4 núcleos de uns 5 anos) e até 100 ms de atraso.
   - **Sala:** até 8 jogadores, com o máximo configurável pelo jogo, e trancada durante a corrida ("corrida em andamento, tente em instantes"); espectador na 0.8.x.
   - **Pausa online:** quem hospeda pode pausar para todos. Para os outros, Esc abre só o menu local, e perder o foco mostra "Jogo pausado" só para quem perdeu, sem parar a partida. Offline, a pausa continua como na 0.6.
   - **Para quem:** só amigos. Conexão sem criptografia, com senha opcional da sala; a conexão cifrada fica desenhada para depois.
   - **Bibliotecas:** a família renet com postcard, atrás de uma crate própria, `oxy_net`, com as versões fixadas.
   - **Conexão:** lista de salas na rede local, endereço digitado, aviso do Firewall do Windows e receita de túnel ou VPN de jogo no guia; UPnP e diagnóstico de CGNAT na 0.8.x; nenhum serviço central da OXY.
   - **Tela online:** uma tela genérica da engine na 0.8.0, aberta pela lógica; telas feitas pelo jogo depois, e as de erro sempre da engine.
   - O que a 0.8.0 leva além do essencial ainda vai ser combinado.

   As revisões do parkour da 0.6 apontaram o que a lógica precisa para o mesmo jogo valer com vários jogadores, sem nada que pressuponha "o único jogador":
   - **personagem local:** uma referência ao personagem de quem joga, resolvida em cada cliente, para a interface e a câmera se ligarem "ao personagem local" em vez de a um objeto fixo;
   - **entradas encaminhadas por jogador** e bots que conduzem vários personagens no mesmo teste (hoje a entrada é uma só para o jogo inteiro, e o bot leva um personagem por pedido);
   - **evento "ao surgir"** para personagens criados no meio da partida, como quem entra numa sala já em andamento (na 0.7, o `on_start` de um script já roda quando uma cópia surge);
   - **escopos de variáveis:** do cliente (menus, volumes, preferências) ou da partida, guardadas pelo servidor;
   - **eventos entre objetos** e um evento **"variável mudou"**, para a lógica reagir sem consultar a cada passo (as mensagens entre objetos, `obj:send` e `on_message`, vieram com os scripts da 0.7);
   - **reinício por personagem**, que leva um jogador de volta ao ponto de partida sem recarregar a cena para todos;
   - **menu sobreposto local**, que abre no cliente sem pausar a simulação da partida.
7. **0.9: FPS.** Lacunas da engine que ainda faltam: navegação/busca de caminho e eventos de mudança de atributo. A mira pelo personagem (`obj.character.aim`, igual em toda máquina), criar inimigos a partir de modelos em posições calculadas (`scene.spawn_model`) e os temporizadores vieram com os scripts da 0.7.
8. **Depois:**
   - tabelas de dados com editor e módulos de jogo em Rust (o terreno do Shadow Duel), decididos no porte;
   - automação/tower defense;
   - port do Shadow Duel;
   - soulslike.

## Shadow Duel

O port do Shadow Duel é um dos motivos da OXY existir. Ele não começa agora, mas a 0.5, a 0.6 e a 0.7 já preparam o terreno.

**Tamanho em 11/10/2026** (repositório [Ratcicle/Shadow-Duel](https://github.com/Ratcicle/Shadow-Duel), 863 commits):
- **Motor (`src/core`): ~210 mil linhas.**
  - IAs: ~89 mil, com busca em feixe, árvore de jogo e estratégia por arquétipo;
  - jogo/regras: ~37 mil;
  - efeitos: ~27 mil;
  - handlers de ações: ~20 mil;
  - contratos: ~13 mil;
  - cadeia: ~10 mil.
- **Cartas: ~229 em 11 arquétipos, descritas como dados** sobre um catálogo de ~119 ações, não como scripts por carta.
- **Interface (DOM + Pixi):** ~15 mil linhas.
- **Testes:** ~99 mil linhas em 423 arquivos, além do replay canônico e do benchmark das IAs.
- **Motor e interface já são separados.** O modo online está planejado e ainda não implementado.

**Direção:**
- **Núcleo de regras e IAs** numa crate Rust determinística. As IAs simulam muitas partidas e copiam o estado do jogo a cada busca, então desempenho e determinismo pesam.
- **Luau sobre o núcleo em Rust** (decidido em 17/10/2026): a lógica de jogo, as telas e o fluxo da partida em scripts Luau, como nos outros jogos, chamando as regras e as IAs da crate Rust. Quanto vai para Rust em cada jogo, e como um projeto liga a sua crate (o `require("@native/...")`, reservado na 0.7), se decide no começo do porte.
- **Cartas** continuam como dados: na 0.7, módulos Luau travados, lidos com `require`; arquivos de dados com editor quando o porte pedir.
- **Online:** a mesma crate roda no servidor, como autoridade da partida. Elixir só entraria para lobby e pareamento, se for preciso.
- **Migração verificada:** tocar os mesmos replays no motor TypeScript e no motor Rust e comparar o estado depois de cada ação, arquétipo por arquétipo.
- **Antes do online:** o embaralhamento do Shadow Duel (`Player.ts`) ainda usa `Math.random` e precisa passar a usar um gerador semeado.

**Terreno que a OXY prepara antes do port:**
1. runtime headless (0.5);
2. determinismo (0.6: sorteios semeados, IDs estáveis para cópias, resumo do estado e checagem de replay);
3. scripts em Luau com o estado do jogo na engine, conferidos por uma VM recriada no meio do teste, e tabelas de conteúdo como módulos travados (0.7);
4. módulos de jogo em Rust: um projeto inclui uma crate própria, ligada ao player, com as regras e as IAs que os scripts chamam (decididos no porte);
5. tabelas de dados com esquema e editor, que servem também a itens, torres e inimigos dos outros jogos, quando o porte pedir.
