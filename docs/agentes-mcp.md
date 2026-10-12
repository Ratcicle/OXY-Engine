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
| `oxy_project` | Cenas, recursos, ações de entrada (com a tecla e quem usa cada uma), superfícies físicas e nomes dos grupos de colisão |
| `oxy_scene` | Árvore de objetos da cena, com componentes e tamanho da lógica; sob pedido, posição no mundo, tamanhos e caixas, e leitura por partes |
| `oxy_entity` | Um ou vários objetos, inteiros ou só as partes pedidas, com a posição no mundo, as caixas, a lógica resumida e os componentes que ainda podem receber |
| `oxy_find` | Busca objetos pelo nome em todas as cenas, por páginas |
| `oxy_catalog` | Nós da lógica (resumidos, ou com portas, parâmetros e valores aceitos para os nós pedidos), componentes, formas, presets e as teclas que as ações de entrada aceitam |
| `oxy_apply` | Aplica um lote de comandos (criar, transformar, componentes, atributos, nós, ligações, ações de entrada, superfícies físicas…) |
| `oxy_validate` | Valida o projeto com as regras de salvar e avisa o que o jogo ignoraria |
| `oxy_simulate` | Roda o jogo sem janela numa cópia do projeto, com entradas programadas, preparação e condições de parada, e relata trocas de cena, logs, ações, sons, eventos de movimento e áreas, a câmera, a interface do jogo e os objetos observados |
| `oxy_screenshot` | Imagem da vista do editor (ao vivo), da câmera do jogo, de uma câmera livre ou de cima e de lado, com os colisores quando pedido |
| `oxy_select`, `oxy_frame` | Ao vivo: seleciona e enquadra objetos para você ver do que o agente fala |
| `oxy_undo`, `oxy_redo` | Ao vivo: desfaz ou refaz o último passo |

As descrições das ferramentas estão em inglês, para os agentes. As mensagens e erros do editor continuam em português.

## Leituras compactas

Os clientes MCP cortam respostas longas, e cada caractere ocupa o contexto do agente. Por isso, desde a 0.5.1:
- As respostas vêm em JSON compacto, sem espaços, com os números arredondados em 4 casas decimais (`0.4000000059604645` vira `0.4`). Abaixo de 1, guardam 4 algarismos significativos, para que um valor pequeno volte como foi posto (`0.00015` continua `0.00015`) e possa ser escrito de novo sem mudar. Valores menores que 0,00001 viram 0: é o ruído das contas em f32, como o que um grupo girado 90° deixa nas posições dos filhos no mundo.
- Ao vivo e nos arquivos, as leituras aceitam as mesmas opções e dão a mesma resposta: o editor responde pela mesma função do núcleo que o servidor usa nos arquivos.
- Opções inválidas são recusadas antes de o servidor procurar o editor ou o projeto, com o nome da opção na mensagem.

**Árvore da cena** (`oxy_scene`):
- `fields` acrescenta a cada objeto:
  - `world_position`: onde a posição do objeto (o pivô) fica no mundo. É o que conta para medir distâncias entre peças de grupos diferentes;
  - `rotation` (local, em graus, como o `set_transform` recebe), `scale` (local) e `dimensions` (da forma);
  - `visual_bounds`: a caixa `{min, max}` no mundo do que o objeto e os filhos mostram, inclusive peças ocultas;
  - `collider_bounds`: a caixa `{min, max, center}` no mundo do colisor que o jogo usa (o corpo do personagem 3D, o colisor 3D ou o colisor de caixa), com `sensor: true` nas áreas e `enabled: false` quando está desligado;
  - `bounds`: as duas caixas.
- `root` lista os filhos de um objeto em vez dos objetos da raiz da cena, e a resposta o identifica em `root`.
- `depth` limita os níveis de filhos; abaixo dele, cada objeto só diz quantos filhos tem (`children_count`).
- `limit` (100 por padrão) é o máximo de objetos na resposta, contando os filhos. `offset` e `limit` leem os objetos listados por partes: a resposta traz `total` e, quando há mais, `next_offset`. Os filhos entram nível a nível enquanto cabem; um objeto cujos filhos já não cabem só diz quantos tem (`children_count`), e a resposta traz `truncated: true`. Para ler esses filhos, use `root` com o ID do objeto.

**Objetos** (`oxy_entity`):
- `id` lê um objeto; `ids` lê vários, devolvidos em `objects`.
- A resposta é a forma de documento do objeto, sem as partes vazias (nulas), mais `world` (posição, rotação em graus e escala no mundo), `visual_bounds` e `collider_bounds` como na árvore, os componentes e os que ainda pode receber.
- A rotação no mundo (`world.rotation_degrees`) se lê como a do `set_transform`. Num objeto da raiz, é a própria `rotation_degrees`. Sob um grupo, a mesma rotação pode ser escrita com dois trios de ângulos, como `[0, 120, 0]` e `[180, 60, 180]`; vem o mais próximo da rotação do próprio objeto, entre -180 e 180. Assim, uma peça girada 120° junto com o grupo lê `[0, 120, 0]`, e não parece de cabeça para baixo.
- `fields` corta a resposta nas partes pedidas, além do ID e do nome, como `character3d.actions`, `transform.position` ou `graph.edges`. Uma parte que não existe vem `null`.
- A lógica vem resumida: `nodes` liga o rótulo de cada nó (a operação e o parâmetro principal, como `event.input(reiniciar)`) ao ID dele e aos parâmetros que fogem do padrão, e `edges` traz as ligações em texto, como `event.input(reiniciar).exec -> character.teleport.exec`. Quando dois nós têm o mesmo rótulo, ele termina em `#N`, a posição do nó no grafo.
- Malhas e geometria de colisão convexa ou de triângulos vêm como contagens.
- `graph: "full"` e `geometry: "full"` devolvem esses trechos como estão gravados, a forma que o `update_entity` edita.

**Busca** (`oxy_find`): devolve `{objects, total}` e `next_offset` quando há mais, com `offset` e `limit` (50 por padrão).

**Catálogo** (`oxy_catalog`): sem opções, os nós vêm resumidos (operação, nome, categoria e descrição), com a lista de categorias em `categories`. Para montar a lógica, o agente pede os nós que vai usar com `operations` (por exemplo `["event.input", "character."]`, em que o ponto final pega todas as operações que começam assim) ou `category`, e recebe só esses nós, com portas e parâmetros. Um parâmetro de valores fixos (o modo de uma ação de entrada, o operador de uma conta, o tipo de evento do personagem…) traz a lista em `choices`, com o valor e o nome de cada opção, e `ref` diz o que um parâmetro aponta: uma cena, um áudio, uma animação, um objeto, um atributo, uma ação de entrada, uma superfície ou um marcador. Sem opções, o catálogo traz também as teclas que as ações de entrada aceitam (`input_keys`) e os presets de superfície. Com os dois, as operações pedidas precisam ser da categoria; uma que existe em outra categoria é recusada com o nome da categoria dela, e não como desconhecida. `brief` escolhe o formato, e `offset` e `limit` leem os nós por partes.

**Projeto** (`oxy_project`):
- `input_actions`: cada ação de entrada com `id`, `name`, `key` e `used_by`, os objetos que a usam (`{entity, name, role}`, em que `role` é o componente, como `character` ou `controller2d`, ou a operação do nó, como `event.input`).
- `surfaces`: cada superfície física com os valores dela (`friction`, `traction`, `speed_multiplier`, `conveyor`, `conveyor_space`) e `used_by`, como nas ações (`role` `collider3d`, `character`, a operação do nó ou `attribute <nome>`).
- `used_by` lista até 10 objetos; quando há mais, `used_by_total` diz quantos são.
- `collision_groups`: os nomes dos grupos de colisão, `{bit, name}`, em que `bit` é o bit da categoria e da máscara dos colisores. Vêm os três grupos com nome padrão e os que o projeto nomeou; os outros se chamam `Grupo N`, com N = bit + 1. Por enquanto só se leem; os nomes se mudam no editor.

**Mudanças em relação à 0.5.0:** o `oxy_find` devolve `{objects, total}` em vez de uma lista; o `oxy_entity` resume a lógica e a geometria e deixa de fora as partes vazias; o `oxy_catalog` resume os nós quando não recebe opções; o `oxy_scene` devolve até 100 objetos por vez, contando os filhos; e o `oxy_project` lista as ações de entrada como `[{id, name, key, used_by}]`, em vez de um mapa de ID para tecla.

## Comandos do `oxy_apply`

**Resposta.** O lote responde `{created, warnings}`:
- `created` liga o índice de cada comando que criou algo (contando do 0) aos IDs criados, na ordem, com o nome de cada um: `{"0": [{"id": "…", "name": "Porta"}]}`. O nome de um nó é a operação dele. Os comandos que não criam nada ficam de fora.
- `warnings` só aparece quando o lote deixa algo que o jogo vai ignorar sem dar erro: um nó que usa uma ação de entrada que o projeto não tem, uma tecla que o jogo não lê ou um parâmetro com valor fora das opções. Um valor vazio também está fora das opções; a exceção é o modo vazio de um nó de ação de entrada, que o jogo lê como "pressionar". Vêm só os avisos que o lote criou, tanto ao vivo quanto nos arquivos (um lote vazio responde `{"created": {}}`); o `oxy_validate` lista todos os do projeto.
- O lote também avisa quando deixa duas ações de entrada atendendo pelo mesmo nome (veja abaixo). O jogo as distingue e há projetos que repetem um nome de propósito (um rótulo para as teclas 1, 2 e 3), então o `oxy_validate` não lista esse aviso.
- Nos arquivos, a resposta traz também `saved` e `previous_version`.

**IDs criados no mesmo lote.** Num comando, um texto que é inteiro `"$N"` vale o primeiro ID criado pelo comando N, e `"$N.M"`, o M-ésimo (os dois contam do 0):
- Um preset devolve todas as peças, a raiz primeiro: com o comando 0 criando um jogador em primeira pessoa, `"$0"` é o jogador e `"$0.2"`, a câmera.
- `duplicate_entities` devolve uma cópia por objeto, na ordem: `"$3.1"` é a segunda cópia.
- Os marcadores valem em qualquer lugar do comando, inclusive dentro do `patch` do `update_entity` e dos `params` dos nós.
- Só o texto inteiro conta: `"$10 de bônus"` continua sendo texto. Para escrever o próprio texto `"$5"`, use `"$$5"`.
- Um marcador que não aponta para um ID criado antes no lote é recusado, com a dica do `$$`.

**Valores fixos.** `add_node` e `set_node_params` recusam valores fora das opções do parâmetro, como o modo `segurando` numa ação de entrada, e a mensagem lista as opções. Antes, o valor era aceito e o nó nunca disparava. O editor usa as mesmas listas.

**Ações de entrada.**
- `create_input_action {name, key}` cria uma ação. O ID dela é o que os nós e o `oxy_simulate` usam (`"$N"` no mesmo lote).
- `rename_input_action {action, name}`, `set_input_key {action, key}` e `delete_input_action {action}` aceitam o ID ou o nome da ação. Um texto igual ao ID de uma ação vale por ela, antes dos nomes.
- A tecla é uma das de `input_keys` no catálogo: letras, números, as outras teclas do teclado, F1 a F35, Shift, os botões do mouse e a roda. Outras grafias que o jogo entende viram o nome do catálogo (`a` vira `A`, `ArrowUp` vira `Up`). Escape (pausa o jogo e libera o mouse) e F3 (diagnóstico do player) são recusadas.
- `create_input_action` e `rename_input_action` recusam um nome que outra ação já tem ou que é o ID de outra ação (como `atacar` ou `pular` em projetos antigos), sem diferenciar maiúsculas.
- Os presets e componentes que criam as ações padrão (Pular, Correr, Agachar…) não fazem essa verificação. Se o nome de uma ação coincidir com o de outra, o lote avisa em `warnings`, e por esse nome os comandos e o `oxy_simulate` recusam a ação (ou, se ele for o ID de uma delas, escolhem essa); renomeie uma delas ou use o ID.
- Excluir uma ação em uso é recusado, com a lista de quem a usa.

**Superfícies físicas.**
- `create_surface` cria uma superfície a partir de um preset (`common`, `ice`, `mud` ou `conveyor`; `common` por padrão) e aplica os valores dados: `name`, `friction`, `traction`, `speed_multiplier`, `conveyor` (velocidade da esteira em m/s) e `conveyor_space` (`World` ou `Local`).
- `update_surface {surface, …}` muda só os valores dados. Valores negativos ou não finitos são recusados.
- `delete_surface {surface}` é recusado enquanto um colisor, personagem, atributo ou nó usa a superfície.
- Para aplicar uma superfície, use o `update_entity` em `physics3d.surface` ou `character3d.body.surface`.

Os comandos de ações de entrada e de superfícies valem para o projeto todo e, como os de cena, não recebem `scene`.

**Mudanças em relação à 0.5.0:**
- O `oxy_apply` responde `{created, warnings}` em vez de `{applied: [{op, created}]}`.
- Os marcadores também valem dentro do `patch`. Um texto que é inteiro `"$N"` passou a ser marcador também no primeiro comando do lote, onde é recusado; para o texto em si, use `"$$N"`.
- Um preset devolve todas as peças, não só a raiz.
- `add_node` e `set_node_params` recusam valores fora das opções.

## Testes com `oxy_simulate`

O teste roda numa cópia do projeto, na memória. Nada é salvo, e nada muda no editor nem no histórico.

- **Entradas programadas** (`inputs`): ações de entrada com horário. Continuam valendo depois de uma troca de cena ou de reiniciar a cena, e uma tecla segurada continua segurada.
  - `press` e `release` usam o ID ou o nome de uma ação de entrada do projeto (ou `"$N"` de uma ação criada no `setup`). Uma ação que o projeto não tem é recusada, com a lista das ações; na 0.5.0, ela era apertada em vão.
  - `movement` é a intenção de andar, `[direita, frente]` de -1 a 1, e vale até o próximo `movement`.
  - `look` é um movimento do mouse, aplicado uma vez: cada unidade gira a câmera pela sensibilidade dela (no controle de câmera, 0,12° por padrão). `x` positivo vira para a direita e `y` positivo olha para baixo, a menos que a câmera inverta o eixo.
  - `click` clica num objeto ou botão da interface.
- **Preparar o teste** (`setup` e `spawn`):
  - `setup` aplica comandos do `oxy_apply` só na cópia, por exemplo um objeto de teste ou outro valor de atributo. Os comandos sem cena usam a cena em que o teste começa: a de `scene` ou, sem ela, a cena inicial do projeto. Ao vivo também é assim; o `oxy_apply`, ao contrário, usa a cena aberta no editor.
  - `spawn` coloca um objeto, como o jogador, numa posição do mundo (e virado para um lado) antes de começar.
  - Os IDs criados no `setup` valem como `"$0"`, `"$1"`… (`"$0.1"` para o segundo ID do comando 0) no resto do pedido, e o relatório os lista em `setup`.
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
