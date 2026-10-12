# Agentes de IA (MCP)

A OXY Engine traz um servidor MCP, o `oxy-mcp.exe`, que deixa agentes de IA como o Claude ajudarem no desenvolvimento dos jogos. Com ele, o agente:
- lê o projeto e consulta o catálogo de nós;
- edita cenas, objetos, componentes e lógica;
- valida o projeto;
- roda o jogo sem janela para testar;
- captura imagens.

## Configurar

1. **No editor:** abra **Preferências do editor** (o botão **Interface: 100%** na barra de status) e ligue **Permitir agentes de IA**. A opção começa desligada. Com ela ligada, a barra de status mostra **Agentes ligados**, ou **Agente conectado** quando um agente está conectado.
2. **No Claude Code:** registre o servidor, apontando para a pasta onde está o `oxy-mcp.exe`:

   ```
   claude mcp add oxy -- "C:\caminho\da\OXY Engine\oxy-mcp.exe"
   ```

3. **No Claude Desktop:** acrescente em `claude_desktop_config.json`:

   ```json
   {
     "mcpServers": {
       "oxy": { "command": "C:\\caminho\\da\\OXY Engine\\oxy-mcp.exe" }
     }
   }
   ```

Para compilar a partir do código-fonte, use `cargo build -p oxy_mcp --release`. O executável sai em `target/release/oxy-mcp.exe`.

## Como o agente trabalha

**Ao vivo (projeto aberto no editor).** O servidor encontra o editor aberto pelo canal local e age sobre ele:
- Cada lote de comandos aparece na hora e vira um passo no histórico, com o rótulo **Agente: …**.
- O Console mostra as ações com a origem **Agente**, e você pode desfazer tudo com Ctrl+Z.
- O agente nunca salva; quem salva é você.
- Se você estiver arrastando ou digitando, o pedido espera o gesto terminar.
- Durante um teste do jogo, uma operação de modelagem em prévia ou uma pose provisória da animação, o editor recusa a edição e explica o motivo.

**Arquivos (sem editor aberto).** Com o caminho do projeto, o agente edita os arquivos diretamente:
- Antes de salvar, a versão anterior é guardada em `project.oxy.json.agente.bak`.
- Se o mesmo projeto estiver aberto num editor, o servidor sempre usa o editor, para não haver duas versões.

**Atomicidade.** Os lotes valem por inteiro: se um comando falha, nada muda, e a mensagem diz qual comando falhou e por quê. As regras são as mesmas do editor, porque a validação é a mesma de salvar.

## Ferramentas

| Ferramenta | O que faz |
| --- | --- |
| `oxy_status` | Editores abertos com agentes permitidos, projeto, cena ativa, alterações não salvas, desfazer/refazer |
| `oxy_project` | Cenas, recursos e ações de entrada |
| `oxy_scene` | Árvore de objetos da cena, com componentes e tamanho da lógica |
| `oxy_entity` | Um objeto completo e os componentes que ainda pode receber |
| `oxy_find` | Busca objetos pelo nome em todas as cenas |
| `oxy_catalog` | Nós da lógica (portas, parâmetros, descrições), componentes, formas e presets |
| `oxy_apply` | Aplica um lote de comandos (criar, transformar, componentes, atributos, nós, ligações…) |
| `oxy_validate` | Valida o projeto com as regras de salvar |
| `oxy_simulate` | Roda o jogo sem janela numa cópia do projeto, com entradas programadas, preparação e condições de parada, e relata trocas de cena, logs, ações, sons, eventos de movimento e áreas, a câmera, a interface do jogo e os objetos observados |
| `oxy_screenshot` | Imagem da vista do editor (ao vivo), da câmera do jogo, de uma câmera livre ou de cima e de lado, com os colisores quando pedido |
| `oxy_select`, `oxy_frame` | Ao vivo: seleciona e enquadra objetos para você ver do que o agente fala |
| `oxy_undo`, `oxy_redo` | Ao vivo: desfaz ou refaz o último passo |

As descrições das ferramentas estão em inglês, para os agentes. As mensagens e erros do editor continuam em português.

## Testes com `oxy_simulate`

O teste roda numa cópia do projeto, na memória. Nada é salvo, e nada muda no editor nem no histórico.

- **Entradas programadas** (`inputs`): ações de entrada com horário. Continuam valendo depois de uma troca de cena ou de reiniciar a cena, e uma tecla segurada continua segurada.
  - `press` e `release` usam os nomes das ações de entrada do projeto.
  - `movement` é a intenção de andar, `[direita, frente]` de -1 a 1, e vale até o próximo `movement`.
  - `look` é um movimento do mouse, aplicado uma vez: cada unidade gira a câmera pela sensibilidade dela (no controle de câmera, 0,12° por padrão). `x` positivo vira para a direita e `y` positivo olha para baixo, a menos que a câmera inverta o eixo.
  - `click` clica num objeto ou botão da interface.
- **Preparar o teste** (`setup` e `spawn`):
  - `setup` aplica comandos do `oxy_apply` só na cópia, por exemplo um objeto de teste ou outro valor de atributo. Os comandos sem cena usam a cena em que o teste começa: a de `scene` ou, sem ela, a cena inicial do projeto. Ao vivo também é assim; o `oxy_apply`, ao contrário, usa a cena aberta no editor.
  - `spawn` coloca um objeto, como o jogador, numa posição do mundo (e virado para um lado) antes de começar.
  - Os IDs criados no `setup` valem como `"$0"`, `"$1"`… no resto do pedido, e o relatório os lista em `setup`.
- **Parar quando algo acontecer** (`until`): troca de cena, um texto no log, um atributo que chega a um valor, um objeto que some ou um evento (pulo, aterrissagem, entrada numa área…). `seconds` vira o limite, e `stopped_at` diz o que parou o teste e quando. O valor comparado precisa ser do tipo do atributo; num atributo de objeto ou de superfície, o texto é o ID (vale `"$0"`).
- **Objetos observados** (`watch`): posição, rotação e escala no mundo, visibilidade e atributos (todos, alguns ou nenhum). Para personagens 3D, também velocidade, chão, postura, apoio e direção do olhar. As amostras (`samples`) ficam espaçadas ao longo de `seconds`, então um teste que o `until` para cedo traz menos amostras.
- **Relatório:**
  - trocas de cena, com `reload` quando a cena reinicia;
  - logs com o nome do objeto e o nó que os gerou;
  - um resumo das ações da lógica: cada ação aparece uma vez por nome de objeto, com quantas vezes rodou e quando;
  - sons;
  - eventos de movimento (pulo, aterrissagem, saída do chão, contato lateral, superfície, postura) e de áreas (entrada e saída);
  - a câmera do jogo no fim;
  - a interface do jogo no fim (`ui`), na ordem em que é desenhada: cada elemento com o tipo, se está visível, o retângulo na vista do jogo, o texto como aparece (com o valor ligado, como `Tempo: 12.50`), o quanto uma barra está cheia (`fraction`) e se um botão dá para clicar. O retângulo é medido numa vista de `width` × `height` pixels (960×540 por padrão). `ui: false` deixa a interface fora do relatório.

**Eventos.** Os eventos entram no relatório no momento em que o jogo os dispara. Por isso aparecem mesmo quando a lógica, no mesmo passo, remove a área ou o objeto (moedas, pontos de controle, zonas de morte) ou troca de cena. Uma área que desliga o próprio colisor não gera saída, como no jogo; ao ser religada, gera uma nova entrada.

**Resumo das ações.**
- A lógica que roda a cada passo (`event.step`), como cronômetros, fica fora do resumo, a menos que o agente peça (`actions_filter: {exclude_operations: []}`).
- A troca de cena (`action.scene`) sempre aparece, mesmo quando um `event.step` a dispara. Assim, cada troca de cena tem a ação que a explica.
- Objetos com o mesmo nome dividem uma linha, como as cópias criadas durante o jogo (`Bala (cópia)`).
- `actions: "all"` lista cada ação com o horário e usa o mesmo filtro. Na 0.5.0 a lista completa era o padrão.
- Cada lista do relatório guarda as 300 entradas mais recentes; no resumo, ficam as 300 linhas com atividade mais recente. `truncated` conta o que ficou de fora.

## Imagens com `oxy_screenshot`

- **Vista do editor** (`view: "editor"`, ao vivo): o que aparece na vista do editor. Com `colliders: true`, o editor desenha a mesma vista de novo com os colisores por cima, como na vista de colisores, sem mexer no que aparece na tela. Durante um teste do jogo, mostra a câmera do jogo.
- **Câmera do jogo** (`view: "game"`, o padrão nos arquivos): roda o jogo numa cópia por `seconds` (0 é o primeiro quadro), com as mesmas `inputs`, `setup`, `spawn` e `until` do `oxy_simulate`, e captura pela câmera do jogo. O texto traz a interface do jogo em `ui`, como no `oxy_simulate`.
- **Câmera livre** (`view: "free"`): uma câmera em `eye` olhando para `target`, que é um ponto ou o ID de um objeto (o centro da forma dele ou, sem forma, a posição), com `fov` em graus (60 por padrão). Só em cenas 3D.
- **De cima e de lado** (`view: "top"` e `view: "side"`): vistas ortográficas, sem perspectiva, que enquadram os objetos de `ids` e seus filhos, ou tudo o que tem forma. De cima, o -Z fica para cima na imagem. De lado, a câmera olha ao longo do -Z ou, quando a parte enquadrada é mais funda (Z) do que larga (X), ao longo do -X, para o lado comprido ocupar a imagem. Em cenas 2D, as duas mostram o plano da cena.
- **Colisores** (`colliders: true`): desenha os colisores sólidos em verde e as áreas em amarelo, inclusive de objetos invisíveis, como pontos de controle.
- O texto da resposta diz o tempo, a cena e a câmera usada (posição, direção e o campo de visão ou a largura e a altura vistas, em metros).

## Limites desta versão

- **Imagem do jogo:** não desenha a interface do jogo (textos, botões e barras). Ela vem descrita em `ui`.
- **Simulação:** nem sempre é determinística. Objetos criados durante o jogo (`action.spawn`) recebem IDs novos a cada execução. Isso fica para a 0.6.
- **Importação:** o agente ainda não importa PNG/WAV; use **Importar** na Biblioteca.
- **Pintura e animação:** pintura e quadros-chave de animação ainda não têm comandos próprios. `update_entity` edita os campos do objeto, mas não os pixels das texturas.
- **Segurança:** o canal só aceita conexões do próprio computador (`127.0.0.1`) e exige o token que o editor grava em `%LOCALAPPDATA%\OXY Engine\agents`. Desligar a opção fecha o canal e apaga esse arquivo.
