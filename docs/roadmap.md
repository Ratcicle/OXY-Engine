# Roadmap da OXY Engine

Plano combinado em 11/10/2026, depois do lançamento da 0.4.2. Ele guia as próximas versões a partir dos jogos que a engine precisa suportar. Atualize este arquivo quando uma etapa terminar ou a ordem mudar.

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

1. **0.4.3: consolidação**
   - Investigar os testes nativos instáveis: `native_navigation_v033` (repintura com o editor parado) e `native_layout_matrix_workflow` (a janela não redimensiona para 920×600).
   - Levar ao visual novo as telas que faltam: Diagnóstico de desempenho, Console (distinguir erro, aviso e log), Excluir recurso, Animação em uso, Prévia da câmera e Ajustar caixa.
   - Conferir se o guia do editor descreve as telas atuais.
2. **0.5: camada de comandos, runtime headless e servidor MCP** (implementada; veja [Agentes de IA (MCP)](agentes-mcp.md))
   - **Camada de comandos:** operações sobre o projeto com validação e desfazer (criar objeto, mudar componente, ligar nós, criar cena…). Editor, MCP, testes e uma futura camada de script usam a mesma porta.
   - **Runtime headless:** rodar a simulação do jogo sem janela por um tempo e devolver logs, estado e erros. Serve ao MCP, aos testes, aos bots e a um servidor online.
   - **Servidor MCP:** para agentes de IA ajudarem no desenvolvimento de jogos. Ele trabalha sobre os arquivos do projeto e **também se conecta ao editor aberto e age ao vivo**: as mudanças aparecem na hora, entram no histórico e podem ser desfeitas. Para isso, o editor expõe um canal local. Primeira versão:
     - ler o projeto;
     - editar cenas, objetos, componentes e lógica;
     - validar;
     - rodar o jogo sem janela;
     - capturar telas.
3. **0.5.1: MCP pronto para o parkour.** Uma análise com agentes (12 de outubro de 2026) montou um trecho de pista pelo MCP e conferiu 88 lacunas no código. Antes da 0.6 entram as que impedem um agente de construir e conferir o próprio trabalho:
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
4. **0.6: parkour.** Feito com ajuda do MCP, que serve de teste do próprio MCP. Lacunas da engine que o parkour exige, encontradas na mesma análise:
   - dados salvos entre partidas (melhor tempo);
   - estado que sobrevive à troca de cena;
   - menu de pausa controlado pelo jogo;
   - ligar e desligar colisores, sensores e plataformas 3D pela lógica;
   - som em loop e canais de música/efeitos;
   - formatação de cronômetro (mm:ss.cc) e mais matemática (piso, resto, mín/máx);
   - determinismo: gerador aleatório semeado, IDs estáveis para objetos criados no jogo e checagem de replay;
   - transformar objetos pela lógica e plataformas que giram carregando o jogador;
   - exportar o jogo pelo editor.

   Do lado do MCP, durante a 0.6 entram as melhorias que o uso real pedir: iniciar e parar o teste no editor sem tomar o mouse da pessoa, entradas de "bot" que seguem um caminho, interface do jogo desenhada nas capturas, lógica compacta com layout legível, colisores a partir da forma da peça, salvar ao vivo opcional (preferência própria, desligada).
5. **0.7: FPS.** Lacunas da engine já identificadas: origem do tiro pela câmera, criar inimigos a partir de modelos em posições calculadas, navegação/busca de caminho, eventos de mudança de atributo e de timer.
6. **Depois:**
   - tabelas de dados e módulos de jogo em Rust (o terreno do Shadow Duel);
   - automação/tower defense;
   - port do Shadow Duel;
   - soulslike.

## Shadow Duel

O port do Shadow Duel é um dos motivos da OXY existir. Ele não começa agora, mas a 0.5 e a 0.6 já preparam o terreno.

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
- **Cartas** continuam como dados, editáveis no editor por meio das tabelas de dados.
- **Online:** a mesma crate roda no servidor, como autoridade da partida. Elixir só entraria para lobby e pareamento, se for preciso.
- **Lua:** opcional, como válvula para efeitos que o catálogo de ações não cobrir.
- **Migração verificada:** tocar os mesmos replays no motor TypeScript e no motor Rust e comparar o estado depois de cada ação, arquétipo por arquétipo.
- **Antes do online:** o embaralhamento do Shadow Duel (`Player.ts`) ainda usa `Math.random` e precisa passar a usar um gerador semeado.

**Terreno que a OXY prepara antes do port:**
1. runtime headless (0.5);
2. determinismo (0.6);
3. módulos de jogo em Rust: um projeto inclui uma crate própria, ligada ao player, com a lógica que os nós não comportam;
4. tabelas de dados com esquema, que servem também a itens, torres e inimigos dos outros jogos.
