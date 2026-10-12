# Agentes de IA (MCP)

A OXY Engine traz um servidor MCP, o `oxy-mcp.exe`, que deixa agentes de IA como o Claude ajudarem no desenvolvimento dos jogos. Com ele, o agente:
- lê o projeto e consulta o catálogo de nós;
- edita cenas, objetos, componentes e lógica;
- importa imagens PNG e sons WAV;
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
- O agente nunca salva; quem salva é você. A importação copia o arquivo para a pasta `assets` do projeto na hora, como o **Importar** da Biblioteca, mas o arquivo do projeto só muda quando você salva.
- O agente lê o Console e sabe se você está testando o jogo, para entender o que deu errado no seu teste.
- Se você estiver arrastando ou digitando, o pedido espera o gesto terminar.
- Durante um teste do jogo, uma operação de modelagem em prévia ou uma pose provisória da animação, o editor recusa a edição e explica o motivo.

**Arquivos (sem editor aberto).** Com o caminho do projeto, o agente edita os arquivos diretamente:
- Antes de salvar, a versão anterior é guardada em `project.oxy.json.agente.bak`.
- Um projeto de formato anterior (de uma OXY mais antiga) é salvo no formato 5 já na primeira edição. Além do `.agente.bak`, que cada edição substitui, fica um backup permanente do arquivo como estava antes da conversão, como `project.oxy.json.schema-4.….backup.json`. Depois disso, a 0.5.x e versões anteriores não abrem mais o projeto.
- Se o mesmo projeto estiver aberto num editor, o servidor sempre usa o editor, para não haver duas versões.

**Atomicidade.** Os lotes valem por inteiro: se um comando falha, nada muda, e a mensagem diz qual comando falhou e por quê. As regras são as mesmas do editor, porque a validação é a mesma de salvar. O comando é contado do 0, como em `"$N"` e em `created`, e a posição vem também por extenso: `Comando 1 (set_node_params, o 2º do lote): Nó … não existe neste objeto. Nada foi alterado.` Na 0.5.0, a mensagem contava do 1 e dizia `Comando 2`. Os erros de leitura dos comandos também vêm em português, como ``valor `Parkour` desconhecido; os aceitos são `direct`, `parkour`, `chained` (maiúsculas e minúsculas contam: escreva `parkour`)``, `falta o campo …`, `tipo errado: recebeu uma lista, esperava um objeto` e `tamanho errado: 2 itens, esperava uma lista de 3 itens` (uma `position` com dois números). Um tipo que a tradução não conhece aparece com o nome em inglês, como o leitor de JSON o escreve.

## Ferramentas

| Ferramenta | O que faz |
| --- | --- |
| `oxy_status` | Editores abertos com agentes permitidos, projeto, cena ativa, alterações não salvas, desfazer/refazer e se há um teste do jogo rodando |
| `oxy_project` | Cenas, recursos (com o arquivo e quem usa cada um), ações de entrada (com a tecla e quem usa cada uma), superfícies físicas, variáveis do jogo (com o valor inicial e quem usa cada uma) e nomes dos grupos de colisão |
| `oxy_scene` | Árvore de objetos da cena, com componentes e tamanho da lógica; sob pedido, posição no mundo, tamanhos e caixas, e leitura por partes |
| `oxy_entity` | Um ou vários objetos, inteiros ou só as partes pedidas, com a posição no mundo, as caixas, a lógica resumida e os componentes que ainda podem receber |
| `oxy_find` | Busca objetos pelo nome em todas as cenas, por páginas |
| `oxy_catalog` | Nós da lógica (resumidos, ou com portas, parâmetros e valores aceitos para os nós pedidos), componentes, formas, presets (o que cada um cria e as opções), perfis de movimento (com os valores) e as teclas que as ações de entrada aceitam |
| `oxy_apply` | Aplica um lote de comandos (criar, transformar, componentes, ajuste dos colisores, atributos, nós, ligações, ações de entrada, superfícies físicas, variáveis do jogo, modelos, cópias entre cenas, animações, perfis de movimento…) |
| `oxy_import` | Importa uma imagem PNG ou um som WAV como recurso do projeto e, se pedido, já aplica a imagem num objeto |
| `oxy_validate` | Valida o projeto com as regras de salvar e avisa o que o jogo ignoraria ou o que nele pararia com erro |
| `oxy_simulate` | Roda o jogo sem janela numa cópia do projeto, com entradas programadas, preparação e condições de parada, e relata trocas de cena, logs, ações, sons, eventos de movimento e áreas, a câmera, a interface do jogo, os objetos observados, as variáveis do jogo e o resumo do estado no fim; se pedido, confere que o teste se repete |
| `oxy_screenshot` | Imagem da vista do editor (ao vivo), da câmera do jogo, de uma câmera livre ou de cima e de lado, com os colisores quando pedido |
| `oxy_select`, `oxy_frame` | Ao vivo: seleciona e enquadra objetos para você ver do que o agente fala |
| `oxy_undo`, `oxy_redo` | Ao vivo: desfaz ou refaz o último passo |
| `oxy_console` | Ao vivo: as linhas do Console (editor, agente, jogo e lógica), só as novas quando o agente pede |

As descrições das ferramentas estão em inglês, para os agentes. As mensagens e erros do editor continuam em português.

## Leituras compactas

Os clientes MCP cortam respostas longas, e cada caractere ocupa o contexto do agente. Por isso:
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

**Catálogo** (`oxy_catalog`): sem opções, os nós vêm resumidos (operação, nome, categoria e descrição), com a lista de categorias em `categories`. Para montar a lógica, o agente pede os nós que vai usar com `operations` (por exemplo `["event.input", "character."]`, em que o ponto final pega todas as operações que começam assim) ou `category`, e recebe só esses nós, com portas e parâmetros. Um parâmetro de valores fixos (o modo de uma ação de entrada, o operador de uma conta, o tipo de evento do personagem…) traz a lista em `choices`, com o valor e o nome de cada opção, e `ref` diz o que um parâmetro aponta: uma cena, um áudio, uma animação, um objeto, um atributo, uma ação de entrada, uma superfície, um marcador ou uma variável do jogo (`variable`). Com os dois, as operações pedidas precisam ser da categoria; uma que existe em outra categoria é recusada com o nome da categoria dela, e não como desconhecida. `brief` escolhe o formato, e `offset` e `limit` leem os nós por partes.

Sem opções, o catálogo traz também:
- `input_keys`: as teclas que as ações de entrada aceitam;
- `surface_presets`: os presets de superfície;
- `presets`: cada preset do `create_entity` com `id`, `description` (para que serve: `platform` é uma plataforma móvel, e não um personagem de jogo de plataforma), `creates` (as peças que ele cria, na ordem em que o lote as devolve, com a forma, os componentes, o pai e as animações) e `params` (as opções, com o valor padrão);
- `movement_profiles`: os perfis Direto, Parkour e Saltos encadeados, com `id` (`direct`, `parkour` ou `chained`), `label` e `values`, os 14 valores que o perfil escreve no personagem.

**Projeto** (`oxy_project`):
- `input_actions`: cada ação de entrada com `id`, `name`, `key` e `used_by`, os objetos que a usam (`{entity, name, role}`, em que `role` é o componente, como `character` ou `controller2d`, ou a operação do nó, como `event.input`). Quando outra ação usa a mesma tecla, `shares_key_with` lista essas ações (`{id, name}`): no jogo, a tecla dispara todas.
- `surfaces`: cada superfície física com os valores dela (`friction`, `traction`, `speed_multiplier`, `conveyor`, `conveyor_space`) e `used_by`, como nas ações (`role` `collider3d`, `character`, a operação do nó ou `attribute <nome>`).
- `variables`: cada variável do jogo com `id`, `name`, `type` (`Number`, `Text`, `Bool`, `Vector2` ou `Vector3`), `value` (o valor inicial, com o tipo, como `{"Number": 0}`), `persistent` (marcada para ser salva entre partidas) e `used_by`, como nas ações (`role` `variable.get` ou `variable.set`).
- `assets`: cada recurso com `id`, `name`, `kind` (`Texture`, `Audio` ou `Model`), `path` (o arquivo, relativo à pasta do projeto; os modelos ficam no próprio arquivo do projeto e não têm `path`) e `used_by`, como nas ações (`role` `material`, `ui` para a imagem de um elemento da interface, `model` para um modelo colocado, ou a operação do nó, como `action.sound`). Um modelo lista cada colocação uma vez, pela raiz, e não cada peça colocada.
- Um objeto que só existe dentro de um modelo vem em `used_by` com `model`, o nome do modelo: o `oxy_entity` e o `oxy_find` não chegam a ele, que aparece nas cenas onde o modelo for colocado.
- `used_by` lista até 10 objetos; quando há mais, `used_by_total` diz quantos são.
- `collision_groups`: os nomes dos grupos de colisão, `{bit, name}`, em que `bit` é o bit da categoria e da máscara dos colisores. Vêm os três grupos com nome padrão e os que o projeto nomeou; os outros se chamam `Grupo N`, com N = bit + 1. Por enquanto só se leem; os nomes se mudam no editor.

**Mudanças em relação à 0.5.0:** o `oxy_find` devolve `{objects, total}` em vez de uma lista; o `oxy_entity` resume a lógica e a geometria e deixa de fora as partes vazias; o `oxy_catalog` resume os nós quando não recebe opções e descreve os presets (`presets` passou de uma lista de nomes para uma lista de objetos, com o nome em `id`); o `oxy_scene` devolve até 100 objetos por vez, contando os filhos; e o `oxy_project` lista as ações de entrada como `[{id, name, key, used_by}]`, em vez de um mapa de ID para tecla.

## Comandos do `oxy_apply`

**Resposta.** O lote responde `{created, warnings}`:
- `created` liga o índice de cada comando que criou algo (contando do 0) aos IDs criados, na ordem, com o nome de cada um: `{"0": [{"id": "…", "name": "Porta"}]}`. O nome de um nó é a operação dele. O que um comando seguinte do mesmo lote apagou continua com o nome que tinha ao ser criado; antes, vinha `null`. Os comandos que não criam nada ficam de fora.
- `warnings` só aparece quando o lote deixa algo que o projeto salva, mas que o jogo vai ignorar ou em que vai parar com erro: um nó que usa uma ação de entrada que o projeto não tem (ou que guarda o nome dela, e não o ID), uma tecla que o jogo não lê ou um parâmetro com valor fora das opções. Um valor vazio também está fora das opções; a exceção é o modo vazio de um nó de ação de entrada, que o jogo lê como "pressionar". Avisa também de um `action.component` cujo alvo, escolhido no parâmetro `target` (e não por uma porta), não tem o que o nó liga e desliga, como uma plataforma móvel num objeto sem ela, e de um `variable.set` cujo parâmetro `value` (sem a entrada ligada) não é do tipo da variável: no jogo, esses nós param com erro. Vêm só os avisos que o lote criou, tanto ao vivo quanto nos arquivos (um lote vazio responde `{"created": {}}`); o `oxy_validate` lista todos os do projeto.
- O lote também avisa quando deixa duas ações de entrada atendendo pelo mesmo nome ou na mesma tecla (veja abaixo). O jogo distingue os nomes, e há projetos que repetem um nome ou uma tecla de propósito (um rótulo para as teclas 1, 2 e 3; pular e confirmar no Espaço), então o `oxy_validate` não lista esses avisos. O aviso vem só do lote que junta uma ação ao nome ou à tecla: excluir uma de três ações na mesma tecla, ou levá-la para uma tecla livre, não avisa sobre as duas que ficam.
- E avisa o que os comandos deixaram e o jogo leva de outro jeito, com o comando que o deixou (`Comando 2 (update_entity, o 3º do lote): …`): um colisor de caixa que ficou com a forma antiga do objeto (veja **Colisores**) e nós copiados para outra cena que ainda abrem ou nomeiam a cena de onde vieram. Esses avisos olham o lote já terminado: se um comando seguinte resolve, o aviso não vem.
- Nos arquivos, a resposta traz também `saved` e `previous_version`.

**IDs criados no mesmo lote.** Num comando, um texto que é inteiro `"$N"` vale o primeiro ID criado pelo comando N, e `"$N.M"`, o M-ésimo (os dois contam do 0):
- Um preset devolve todas as peças, a raiz primeiro: com o comando 0 criando um jogador em primeira pessoa, `"$0"` é o jogador e `"$0.2"`, a câmera.
- `duplicate_entities` devolve uma cópia por objeto, na ordem: `"$3.1"` é a segunda cópia. Com `count`, devolve todas as cópias, uma rodada depois da outra.
- `place_model` devolve todas as peças do modelo colocado, a raiz primeiro; `copy_entities`, uma cópia por objeto; `set_clip`, o ID da animação nova; `save_model`, o ID do modelo.
- Os marcadores valem em qualquer lugar do comando, inclusive dentro do `patch` do `update_entity` e dos `params` dos nós.
- Só o texto inteiro conta: `"$10 de bônus"` continua sendo texto. Para escrever o próprio texto `"$5"`, use `"$$5"`.
- Um marcador que não aponta para um ID criado antes no lote é recusado, com a dica do `$$`.

**Valores fixos.** `add_node` e `set_node_params` recusam valores fora das opções do parâmetro, como o modo `segurando` numa ação de entrada, e a mensagem lista as opções. Antes, o valor era aceito e o nó nunca disparava. O editor usa as mesmas listas.

**Valores dos parâmetros e atributos.** Os valores levam o tipo: `{"Number": 3}`, `{"Text": "…"}`, `{"Bool": true}`, `{"Object": "<ID>"}`, `{"Vector2": [x, y]}`, `{"Vector3": [x, y, z]}`, `{"Surface": "<ID>"}`.
- Num parâmetro de nó, o valor também pode vir simples quando o catálogo dá o tipo do parâmetro e a leitura é óbvia: um número num parâmetro `Number`, um texto num `Text` e `true` ou `false` num `Bool`. Assim, `"params": {"action": "Pausar", "mode": "held"}` vale o mesmo que com `{"Text": …}`.
- Alguns parâmetros aceitam qualquer tipo: os que têm uma entrada de mesmo nome do tipo `any` no catálogo, como o `value` do `attribute.set` e o `a` e o `b` do `condition.compare`. O padrão deles é só o valor inicial, não o tipo, e um valor simples vale pela forma: um número vira `Number`, um texto vira `Text` e `true` ou `false` vira `Bool`. Assim, `{"attribute": "Checkpoint", "value": "Outro"}` guarda o texto `Outro`. Uma lista ou um objeto precisa do tipo, como `{"Vector3": [0, 1, 0]}` ou `{"Object": "<ID>"}`.
- Os outros valores simples são recusados com uma mensagem em português que diz o parâmetro e a forma esperada, como `O parâmetro target do nó action.damage precisa de um valor marcado com o tipo, como {"Object":"abc"}; recebeu "abc"`. Um texto nunca vira objeto ou superfície sozinho, e um tipo com a inicial minúscula (`{"number": 3}`) também é recusado, com o aviso de que o nome do tipo começa com maiúscula.
- Os atributos não têm tipo no catálogo: o `set_attribute` só aceita valores com o tipo, e um valor simples é recusado da mesma forma (`O atributo vida precisa de um valor marcado com o tipo, como {"Number":3}; recebeu 3`).
- Antes, todos esses casos davam o erro do leitor de JSON, em inglês, sem dizer o parâmetro.

**Ações de entrada.**
- `create_input_action {name, key}` cria uma ação. O ID dela é o que os nós e o `oxy_simulate` usam (`"$N"` no mesmo lote).
- `rename_input_action {action, name}`, `set_input_key {action, key}` e `delete_input_action {action}` aceitam o ID ou o nome da ação. Um texto igual ao ID de uma ação vale por ela, antes dos nomes.
- A tecla é uma das de `input_keys` no catálogo: letras, números, as outras teclas do teclado, F1 a F35, Shift, os botões do mouse e a roda. Outras grafias que o jogo entende viram o nome do catálogo (`a` vira `A`, `ArrowUp` vira `Up`). Escape (pausa o jogo e libera o mouse) e F3 (diagnóstico do player) são recusadas.
- `create_input_action` e `rename_input_action` recusam um nome que outra ação já tem ou que é o ID de outra ação (como `atacar` ou `pular` em projetos antigos), sem diferenciar maiúsculas.
- Os presets e componentes que criam as ações padrão (Pular, Correr, Agachar…) não fazem essa verificação. Se o nome de uma ação coincidir com o de outra, o lote avisa em `warnings`, e por esse nome os comandos e o `oxy_simulate` recusam a ação (ou, se ele for o ID de uma delas, escolhem essa); renomeie uma delas ou use o ID.
- Uma tecla que outra ação já usa é aceita, e o lote avisa em `warnings`, com as ações dessa tecla: no jogo, apertá-la dispara todas, e o `oxy_simulate`, que aperta ações e não teclas, não mostra isso. O `oxy_project` marca essas ações com `shares_key_with`.
- Nos nós, o parâmetro de ação de entrada (`ref` `input_action` no catálogo, como o `action` do `event.input` e do `input.read`) aceita o ID ou o nome da ação, com as mesmas regras do `rename_input_action`, e guarda o ID, que é o que o jogo lê. Antes, o nome era guardado como estava e o nó nunca disparava, com um aviso dizendo que a ação não existia. Um nome que duas ações têm é recusado; uma ação que o projeto não tem fica como foi escrita, com o aviso. Um nó que já guardava o nome de uma ação ganha um aviso próprio: `set_node_params` com o nome guarda o ID. Se o nome guardado for de mais de uma ação, o aviso lista os IDs delas, e o `set_node_params` precisa do ID da ação certa.
- Excluir uma ação em uso é recusado, com a lista de quem a usa.

**Superfícies físicas.**
- `create_surface` cria uma superfície a partir de um preset (`common`, `ice`, `mud` ou `conveyor`; `common` por padrão) e aplica os valores dados: `name`, `friction`, `traction`, `speed_multiplier`, `conveyor` (velocidade da esteira em m/s) e `conveyor_space` (`World` ou `Local`).
- `update_surface {surface, …}` muda só os valores dados. Valores negativos ou não finitos são recusados.
- `delete_surface {surface}` é recusado enquanto um colisor, personagem, atributo ou nó usa a superfície.
- Para aplicar uma superfície, use o `update_entity` em `physics3d.surface` ou `character3d.body.surface`.

**Variáveis do jogo.** Guardam valores do jogo inteiro, como uma contagem ou a cena em que o jogador parou, que a lógica lê e muda e que continuam iguais ao mudar ou reiniciar a cena. Cada partida começa com os valores iniciais: o teste do editor, o jogo aberto de novo e cada `oxy_simulate`. A exceção são as variáveis salvas entre partidas, que começam no progresso salvo, quando há (veja **Progresso salvo**, abaixo).
- `create_variable {name, value, persistent?}` cria uma variável e responde o ID dela (`"$N"` no mesmo lote). `value` é o valor inicial e dá o tipo da variável: `Number`, `Text`, `Bool`, `Vector2` ou `Vector3`. Um número, um texto ou `true`/`false` simples vale pela forma; um vetor leva o tipo (`{"Vector3": [0, 1, 0]}`). Objetos e superfícies são recusados: um objeto deixa de existir ao mudar de cena. `persistent` marca a variável para ser salva entre partidas (falso por padrão): o nó `save.write` a salva, e a partida seguinte começa no valor salvo.
- `update_variable {variable, name?, value?, persistent?}` muda o que vier. Um valor simples é lido no tipo atual da variável (num `Number`, `"dez"` é recusado com a forma esperada); para mudar o tipo, mande o valor com o tipo, como `{"Text": "Início"}`.
- `delete_variable {variable}` é recusado enquanto um nó usa a variável, com a lista desses nós.
- `variable` é o ID ou o nome. Os nomes não se repetem, sem diferenciar maiúsculas, e não têm `·`, que os relatórios usam para separar a chave (`Nome · chave`). O projeto tem até 256 variáveis, e um texto tem até 4096 bytes.
- Na lógica, `variable.get` (**Ler variável do jogo**) fornece o valor em `value`, e `variable.set` (**Alterar variável do jogo**) grava o `value`, que precisa ser do tipo da variável: outro tipo para o comportamento do objeto, com o erro no relatório. Os dois ficam na categoria **Jogo**. O parâmetro `variable` (`ref` `variable` no catálogo) aceita o ID ou o nome da variável e guarda o ID; uma variável que o projeto não tem é recusada, com a lista das variáveis.
- A chave (`key`, texto, no parâmetro ou na entrada de mesmo nome) guarda um valor por chave: ligue `scene.current.name -> variable.set.key` (e o mesmo no `variable.get`) para um valor por cena. Sem chave, vale o valor único da variável; uma chave em que nada foi gravado dá o valor inicial. Cada variável guarda até 1024 chaves durante a partida, e uma chave tem até 4096 bytes.

**Progresso salvo.** Os nós da categoria **Jogo** guardam as variáveis marcadas com `persistent` de uma partida para outra. Nada é salvo sem eles: mudar uma variável não basta.
- `save.write` (**Salvar progresso**) salva o valor atual de cada variável salva entre partidas, com todas as chaves. As outras variáveis ficam de fora.
- `save.clear` (**Apagar progresso**) apaga o progresso e volta as variáveis salvas entre partidas ao valor inicial, chaves incluídas.
- `save.state` (**Ler progresso salvo**) fornece `exists` (booleano): se há progresso salvo, de uma partida anterior ou desta. Serve, por exemplo, para mostrar um botão Continuar.
- O jogo exportado guarda o progresso em `%APPDATA%\OXY Engine\Jogos\<id do projeto>\progresso.json`, pelo ID do projeto, que não muda ao renomear o jogo. Um arquivo que não dá para ler é guardado à parte (`progresso.json.ilegivel-….json`) e o jogo começa sem progresso; um arquivo de uma versão mais nova do jogo nunca é gravado por cima. Os problemas aparecem só no diagnóstico do jogo (F3).
- O teste do editor guarda o progresso na memória, separado do jogo exportado: o próximo **Jogar** começa dele, e ele some ao abrir outro projeto ou com **Apagar progresso do teste**, em **Variáveis do jogo**. Cada gravação e cada apagamento viram uma linha no Console, como `Teste: progresso salvo — Recorde · Cena 2 = 41.2`.
- O `oxy_simulate` nunca lê nem grava esses progressos: o progresso entra pelo `save` e sai no relatório (veja abaixo).

Os comandos de ações de entrada, de superfícies e de variáveis valem para o projeto todo e, como os de cena, não recebem `scene`.

**Colisores.**
- O `add_component` dá ao colisor de caixa (`collider3d`, `sensor3d`, `collider2d`, `sensor2d`) as dimensões da forma naquele momento. Depois, a caixa acompanha a escala do objeto, mas não as mudanças de dimensões: um `update_entity` em `dimensions` depois do `add_component` deixa a caixa com o tamanho antigo, e o jogador atravessa a parte nova. O lote avisa quando isso acontece, com o tamanho da forma e o da caixa. O aviso vale também para uma caixa que o `fit_collider` ajustou à forma do objeto, com ou sem `margin`; numa forma plana (um `Plane`, um `Rectangle`), a espessura da caixa não conta.
- `fit_collider {entity, children?, margin?}` ajusta a caixa à forma do objeto, como o **Ajustar ao objeto** do editor, ou, com `children: true`, à forma dele e de todas as peças dentro dele, como o **Ajustar ao grupo/filhos** (câmeras e elementos da interface ficam de fora). `margin` soma uma folga, em metros, em todos os lados (0 por padrão).
- No 3D, a caixa continua girada com o objeto: o tamanho e o centro são medidos nos eixos dele, então um grupo girado e com escala envolve as peças sem sobra. No 2D, o colisor de caixa recebe a caixa das peças no mundo, como no editor.
- São recusados, com o motivo: cápsulas, esferas e colisores convexos ou de malha (o `fit_collider` ajusta só caixas), o Personagem 3D (a colisão dele é o corpo, em `character3d.body`), um objeto sem colisor e, sem `children`, um objeto sem forma própria, como um grupo.
- Na lógica, o nó `action.component` com `component` `collider` (**Colisão / área (2D e 3D)**) liga e desliga o colisor do objeto: o colisor de caixa 2D, o colisor 3D sólido ou a área 3D (os dois, num objeto antigo que tem ambos). Antes, valia só para o colisor de caixa, e num colisor 3D o nó parava com erro. Desligado, o colisor sai das consultas da lógica já no mesmo passo, e quem estava em cima de um colisor sólido perde o apoio e cai.

**Reaproveitar entre fases.**
- `duplicate_entities {ids, offset?, count?}` faz `count` cópias (de 1 a 100; 1 por padrão). A cópia k fica a k × `offset` do original, em relação ao pai (`[0.5, 0, 0]` por padrão, como no editor), e as animações que movem o objeto vão junto. Uma fileira de plataformas a cada 4 m é um comando só: `{"op": "duplicate_entities", "ids": ["$0"], "offset": [4, 0, 0], "count": 10}` põe 10 cópias depois da original. Com mais de uma cópia, os nomes ganham o número: `Plataforma (cópia 3)`.
- `copy_entities {ids, to_scene, parent?}` copia objetos, com tudo o que está dentro deles, da cena `scene` (a do comando ou a da ferramenta) para `to_scene`, na raiz ou dentro de `parent`. Eles ficam no mesmo lugar do mundo, e as animações que os movem fazem o mesmo percurso no mundo: uma plataforma que anda 4 m continua andando 4 m dentro de um grupo com escala 2 ou girado. Se a escala não uniforme do pai de destino deformasse o objeto ou a animação, a cópia é recusada. Para outra cena, as referências precisam ficar dentro da cópia: um atributo, um vínculo da interface, uma câmera, um nó ou uma animação que aponta para um objeto que não vai junto faz o comando ser recusado, com a lista do que falta. Copie esses objetos junto.
- Os nós `action.scene` copiados continuam abrindo a cena que já abriam, e os `value.scene`, fornecendo a cena que já forneciam. Quando é a cena de onde a cópia saiu (um "reiniciar fase" levado para a fase 2 reabriria a fase 1), o lote avisa, com a lista desses nós; cada um vem com o nome e o ID da cópia que o tem (`Filho (<ID>) → nó action.scene (<nó>)`), inclusive as peças dentro dos objetos copiados, que não aparecem em `created`. Aponte-os para a cena nova com `set_node_params`. Um nó com a entrada `scene` ligada não entra no aviso, mesmo que o parâmetro ainda nomeie a cena antiga: no jogo, quem decide é a entrada.
- `save_model {entity, name?}` guarda o objeto e tudo o que está dentro dele como um modelo, um recurso do projeto (o nome padrão é o do objeto), com a mesma regra das referências. Um nome que outro modelo já tem é recusado, sem diferenciar maiúsculas, porque o `place_model` acha os modelos pelo nome. O **Salvar hierarquia como modelo** do editor também não repete nomes: se já houver um modelo `Torre`, o novo se chama `Torre 2`. `place_model {model, parent?, position?, name?}` coloca uma cópia do modelo na cena, pelo ID ou pelo nome do modelo, com a raiz em `position` (em relação ao pai). As referências internas (lógica, câmera, animações) apontam para as peças colocadas. Um modelo que não existe é recusado com a lista dos modelos do projeto.
- Os nós `action.scene` e `value.scene` de um modelo continuam com a cena que tinham quando ele foi salvo, e o `place_model` não avisa: o modelo não guarda de que cena veio, e uma porta para a fase seguinte abre outra cena de propósito. Um "reiniciar fase" salvo na fase 1 e colocado na fase 2 reabre a fase 1; depois do `place_model`, aponte esses nós para a cena nova com `set_node_params` (os IDs estão em `created`, a raiz primeiro), ou monte o reinício pela entrada `scene`, como abaixo.
- **Reiniciar e seguir para outra cena sem ID fixo.** O `action.scene` tem a entrada `scene` (Texto). Ligada, ela decide a cena que o nó abre, no lugar do parâmetro `scene`, que fica como está (vazio, de preferência). O nó `scene.current` (**Cena atual**, categoria **Jogo**) fornece a cena em que o objeto está: `scene`, o ID, e `name`, o nome.
  - Reiniciar a cena: `event.input(reiniciar).exec -> action.scene.exec` e `scene.current.scene -> action.scene.scene`. A mesma lógica, levada com `copy_entities` ou salva num modelo e colocada com `place_model`, reinicia a cena em que estiver, sem aviso e sem `set_node_params`. No `oxy_simulate`, a troca vem com `reload: true`.
  - Seguir para a próxima cena: o nó `value.scene` (**Referência de cena**, categoria **Dados**) fornece em `value` o ID da cena escolhida no parâmetro `scene`. Ligue `value.scene.value -> action.scene.scene`, ou guarde o ID num atributo de texto do objeto e ligue `attribute.get.value -> action.scene.scene`, com `attribute` no nome do atributo. O atributo recebe o ID pelo `set_attribute` (`{"Text": "<ID da cena>"}`) ou, na lógica, por `value.scene.value -> attribute.set.value`. Cada cena tem a sua cópia do objeto, com a cena seguinte.
  - A entrada espera o ID da cena, não o nome. Um texto vazio, um nome ou um ID que o projeto não tem para o comportamento do objeto, e o relatório traz o erro com o objeto e o nó: `Cena de destino não existe: a entrada Cena recebeu "Fase 2", o nome de uma cena; a entrada espera o identificador…`. Sem a entrada ligada, o parâmetro vale como antes, e um parâmetro vazio continua sendo erro.
  - Com a entrada ligada, o parâmetro é ignorado e não segura a cena que nomeia: `delete_scene` (e o **Excluir cena** do editor) não recusa a exclusão por causa desse nó e deixa o parâmetro vazio.
  - O editor não mostra IDs de cena. Quem monta pelo editor escolhe a cena pelo nome: no parâmetro de cada cópia do `action.scene` ou numa **Referência de cena**, que leva o ID à entrada `scene` ou a um atributo de texto. O `value.scene` segura a cena que nomeia como o `action.scene`: o `delete_scene` recusa a exclusão, e o aviso do `copy_entities` o lista. Vazio, ele para o comportamento que o lê, com erro.
- O jogador dos presets tem a câmera ao lado, e não dentro dele. Para levar o jogador com a câmera para outra fase, agrupe os dois (`create_entity` de um grupo e `set_parent`) e salve o grupo como modelo. A interface e a lógica de um cronômetro vão do mesmo jeito: num grupo, com os objetos que elas usam.

**Plataformas móveis e animações.**
- O preset `platform` aceita `travel` (quanto a plataforma anda a partir de onde é posta, em metros; `[4, 0, 0]` por padrão) e `period` (segundos da ida e da volta; 4 por padrão). Ele cria uma animação comum, em loop, que um nó toca quando a cena começa.
- `set_transform` move junto os quadros-chave do objeto quando as animações dele mesmo o movem, como as de uma plataforma móvel. Assim, a plataforma movida continua indo e voltando a partir do lugar novo; na 0.5.0, o jogo a devolvia ao percurso antigo. Um giro gira o percurso em volta do lugar do objeto, e os ângulos de cada quadro-chave mudam o mesmo tanto que os do objeto (um quadro-chave a 120° num objeto girado 30° passa a 150°, e um a 360°, a 390°). Uma escala muda a escala dos quadros-chave, sem mudar o percurso. `move_tracks: false` deixa os quadros-chave como estão; `move_tracks: true` também move as trilhas que outras animações têm para o objeto (a peça de um boneco, por exemplo). O editor segue a mesma regra ao mover, girar ou escalar o objeto pelas alças ou pelas Propriedades, fora da aba Animação.
- `set_clip {entity, clip?, name, duration, looping?, tracks, events?}` cria uma animação do objeto ou substitui a animação `clip` (pelo ID ou pelo nome), mantendo o ID que os nós usam. Cada trilha anima o próprio objeto (`target` ausente ou `"self"`) ou uma peça dentro dele, com quadros-chave `{time, position?, rotation?, scale?, interpolation?}`: transformações locais, com a rotação em graus e `interpolation` `linear` (padrão) ou `hold` (fica parado até o próximo). O que o quadro-chave não diz vem da transformação atual do alvo, e o pivô dele é mantido. `events` são marcadores `{time, name}`, que o nó `event.animation` usa pelo nome. Para tocar a animação, use o nó `action.animation` com o ID dela em `clip`.
- `delete_clip {entity, clip}` exclui uma animação; é recusado enquanto um nó `action.animation` a toca, com a lista desses nós.
- Na lógica, o `action.component` com `component` `platform` (**Plataforma móvel**) liga e desliga a plataforma móvel do objeto. Uma plataforma por velocidade para e volta a andar de onde parou; ao voltar, leva quem está em cima no mesmo passo em que anda, sem lançá-lo. Uma plataforma que segue a animação só deixa de levar quem está em cima, e a animação continua: para parar o movimento, desligue também a animação (`component` `animation`).

**Presets e perfis de movimento.**
- Os presets de personagem (`first_person` e `third_person`) aceitam `camera`: `create` (padrão) cria a Câmera principal; `reuse` faz a câmera ativa da cena (ou a primeira) seguir o novo jogador, sem criar outra; `none` não põe câmera nenhuma. `reuse` é recusado se a cena não tem câmera ou se a câmera já segue outro objeto. Sem a Câmera principal, as peças seguintes sobem uma posição na resposta.
- `apply_movement_profile {entity, profile}` aplica um perfil de movimento (`direct`, `parkour` ou `chained`) a um personagem 3D, como o botão do editor: os valores listados no catálogo são escritos no personagem e continuam editáveis.

**Mudanças em relação à 0.5.0:**
- O `oxy_apply` responde `{created, warnings}` em vez de `{applied: [{op, created}]}`.
- Os marcadores também valem dentro do `patch`. Um texto que é inteiro `"$N"` passou a ser marcador também no primeiro comando do lote, onde é recusado; para o texto em si, use `"$$N"`.
- Um preset devolve todas as peças, não só a raiz.
- `add_node` e `set_node_params` recusam valores fora das opções.
- `set_transform` move junto os quadros-chave dos objetos que as próprias animações movem, como as plataformas móveis (`move_tracks: false` mantém o comportamento antigo). O editor faz o mesmo.
- `duplicate_entities` sem `count` nem `offset` continua fazendo uma cópia a 0,5 m em X.
- As mensagens de erro contam os comandos do 0, como `"$N"` e `created`, e dizem a posição por extenso (`Comando 1 (set_node_params, o 2º do lote)`); antes, contavam do 1.
- Os parâmetros de ação de entrada dos nós guardam o ID também quando recebem o nome, e os parâmetros aceitam valores simples quando o catálogo dá o tipo ou, nos parâmetros de qualquer tipo, pela forma do valor.
- `save_model` recusa o nome de outro modelo; `fit_collider` é novo.
- Há variáveis do jogo (`create_variable`, `update_variable` e `delete_variable`), os nós `variable.get` e `variable.set` e os de progresso salvo, `save.write`, `save.clear` e `save.state`.
- O `action.scene` ganhou a entrada `scene` e há os nós `scene.current`, na categoria nova **Jogo**, e `value.scene` (**Referência de cena**), em **Dados**. Não há migração: a entrada não é salva no nó, só a conexão. O aviso de nós copiados que ainda abrem ou nomeiam a cena de onde vieram deixa de fora os que têm essa entrada ligada e sugere ligá-la, e esses nós também não impedem o `delete_scene`.

## Testes com `oxy_simulate`

O teste roda numa cópia do projeto, na memória. Nada é salvo, e nada muda no editor nem no histórico.

- **Entradas programadas** (`inputs`): ações de entrada com horário. Continuam valendo depois de uma troca de cena ou de reiniciar a cena, e uma tecla segurada continua segurada.
  - `press` e `release` usam o ID ou o nome de uma ação de entrada do projeto (ou `"$N"` de uma ação criada no `setup`). Uma ação que o projeto não tem é recusada, com a lista das ações; na 0.5.0, ela era apertada em vão.
  - `movement` é a intenção de andar, `[direita, frente]` de -1 a 1, e vale até o próximo `movement`.
  - `look` é um movimento do mouse, aplicado uma vez: cada unidade gira a câmera pela sensibilidade dela (no controle de câmera, 0,12° por padrão). `x` positivo vira para a direita e `y` positivo olha para baixo, a menos que a câmera inverta o eixo.
  - `click` clica num objeto ou botão da interface.
- **Preparar o teste** (`setup` e `spawn`):
  - `setup` aplica comandos do `oxy_apply` só na cópia, por exemplo um objeto de teste ou outro valor de atributo. Os comandos sem cena usam a cena em que o teste começa: a de `scene` ou, sem ela, a cena inicial do projeto. Ao vivo também é assim; o `oxy_apply`, ao contrário, usa a cena aberta no editor.
  - Para começar uma variável do jogo em outro valor (testar com 1 vida em vez de 3, por exemplo), use `update_variable` no `setup`: o teste sempre começa com os valores iniciais da cópia (as salvas entre partidas começam no `save`, quando ele vem).
  - `save` é o progresso que um jogador que volta ao jogo tem: `{variável: valor}`, com cada variável salva entre partidas pelo ID ou pelo nome (vale `"$N"` do `setup`), `Nome · chave` para o valor de uma chave e o valor simples ou com o tipo, como em `{"Recorde · Cena 2": 41.2, "Moedas": {"Number": 3}}`. Sem `save`, o jogo roda como recém-instalado. Uma variável que o projeto não tem, ou que não é salva entre partidas, é recusada; para testar uma variável comum com outro valor, use `update_variable` no `setup`.
  - `spawn` coloca um objeto, como o jogador, numa posição do mundo (e virado para um lado) antes de começar.
  - Os IDs criados no `setup` valem como `"$0"`, `"$1"`… (`"$0.1"` para o segundo ID do comando 0) no resto do pedido, e o relatório os lista em `setup`.
- **Parar quando algo acontecer** (`until`): troca de cena, um texto no log, um atributo que chega a um valor, uma variável do jogo que chega a um valor (`variable: {variable, key?, op, value}`, com o ID ou o nome da variável e, para comparar o valor de uma chave, `key`), um objeto que some ou um evento (pulo, aterrissagem, entrada numa área…). `seconds` vira o limite, e `stopped_at` diz o que parou o teste (`reason`) e quando, com o que aconteceu em palavras em `detail`, como `Jogador aterrissou em Pouso`; quando é um evento, `kind` traz o tipo dele (`land`, `jump`…), como em `events`. O valor comparado precisa ser do tipo do atributo ou da variável; num atributo de objeto ou de superfície, o texto é o ID (vale `"$0"`).
- **Objetos observados** (`watch`): posição, rotação e escala no mundo, visibilidade e atributos (todos, alguns ou nenhum). Para personagens 3D, também velocidade, chão, postura, apoio e direção do olhar. As amostras (`samples`) são exatamente as pedidas, a última com o estado final, e ficam espaçadas ao longo de `seconds`, no máximo uma por passo de 1/60 s. Um teste que o `until` para cedo traz menos amostras.
- **Relatório:**
  - trocas de cena, com `reload` quando a cena reinicia;
  - logs com o nome do objeto e o nó que os gerou. Os avisos de movimento nomeiam o personagem: `Personagem Jogador: Apoio em Elevador liberado sem herdar velocidade…` aparece só quando uma plataforma que gira, muda de escala ou salta de posição deixa alguém sem apoio, não a cada início de cena. Sai uma vez por personagem e plataforma até a cena recomeçar e cita a parte que se moveu: numa ponte de várias tábuas que gira inteira, a ponte, não a tábua sob os pés;
  - um resumo das ações da lógica: cada ação aparece uma vez por nome de objeto, com quantas vezes rodou e quando;
  - sons, inclusive os pedidos no mesmo passo de uma troca de cena, como um som tocado logo antes de reiniciar a fase (o jogo também os toca). Ficam de fora só os sons de uma cena que começa e termina no mesmo passo, como numa cena que se reinicia ao começar;
  - eventos de movimento (pulo, aterrissagem, saída do chão, contato lateral, superfície, postura) e de áreas (entrada e saída);
  - a câmera do jogo no fim;
  - a interface do jogo no fim (`ui`), na ordem em que é desenhada: cada elemento com o tipo, se está visível, o retângulo na vista do jogo, o texto como aparece (com o valor ligado, como `Tempo: 12.50`; para ler `Tempo: 01:23.45`, a lógica formata os segundos com o nó `text.time` e grava o texto num atributo de texto ligado ao elemento), o quanto uma barra está cheia (`fraction`) e se um botão dá para clicar. O retângulo é medido numa vista de `width` × `height` pixels (960×540 por padrão). `ui: false` deixa a interface fora do relatório;
  - as variáveis do jogo no fim (`variables`), pelo nome, com o valor e o tipo (`{"Moedas": {"Number": 12}}`); o valor de uma chave vem como `Nome · chave`, como `Pontos · Cena 2`. Vêm até 300 valores: primeiro o valor único de cada variável, depois os das chaves, em ordem de nome; `truncated` conta os que ficaram de fora. Num projeto sem variáveis, o campo não vem;
  - o progresso salvo: `saves` lista cada `save.write` e `save.clear` do teste, `{time, kind: "write" | "clear", values}`, com os valores salvos (só na gravação) no formato de `variables`, e `save` traz o progresso no fim, no mesmo formato da entrada `save`. Para testar a volta do jogador, passe o `save` de um relatório no `save` do teste seguinte: na segunda corrida, por exemplo, um recorde menor substitui o salvo. Sem progresso no fim (nada salvo, ou apagado), `save` não vem. Cada gravação em `saves` e o `save` trazem até 300 valores; `values_truncated` (na gravação) e `truncated.save` contam os que ficaram de fora.

**Repetir um teste.** O mesmo pedido dá o mesmo jogo; veja [Determinismo](#determinismo).

**Eventos.** Os eventos entram no relatório no momento em que o jogo os dispara. Por isso aparecem mesmo quando a lógica, no mesmo passo, remove a área ou o objeto (moedas, pontos de controle, zonas de morte) ou troca de cena. Uma área, 2D ou 3D, que a lógica desliga (`action.component` com `collider`) não gera saída para quem estava dentro, como no jogo; ao ser religada, quem está dentro entra de novo, numa nova ativação (um `action.damage` de uma vez por ativação acerta outra vez), mesmo que a área tenha sido desligada e religada no mesmo passo.

**Mudanças no relatório em relação à 0.5.0:**
- `scene_changes` passou de `{time, text}` para `{time, scene, name, reload}`.
- Os logs saem sem IDs no texto e ganham `object` e `node`.
- `actions` passou a ser um resumo por padrão (veja abaixo); a lista completa vem com `actions: "all"`.
- As amostras de `watch` só trazem os campos pedidos em `fields`, e a rotação no mundo é lida como no `oxy_scene` (um objeto girado 120° lê `[0, 120, 0]`, não `[180, 60, 180]`).
- `oxy_simulate` e `oxy_screenshot` recusam `width` e `height` fora de 16–4096, e o texto do `oxy_screenshot` nas vistas do jogo passou a ser JSON (tempo, cena e câmera usada).
- `samples` traz exatamente o número pedido; antes, quando os passos não se dividiam igualmente, vinha uma amostra a mais.
- O `detail` de um `stopped_at` por evento passou de `land de Jogador (Pouso)` para `Jogador aterrissou em Pouso`, e o tipo do evento ganhou o campo `kind`.
- O relatório traz `seed`, a semente dos sorteios, e `state_hash`, o resumo do estado no fim; as cópias do `action.spawn` mantêm os IDs de uma execução para outra.

**Resumo das ações.**
- A lógica que roda a cada passo (`event.step`), como cronômetros, fica fora do resumo, a menos que o agente peça (`actions_filter: {exclude_operations: []}`).
- A troca de cena (`action.scene`) sempre aparece, mesmo quando um `event.step` a dispara. Assim, cada troca de cena tem a ação que a explica.
- Objetos com o mesmo nome dividem uma linha, como as cópias criadas durante o jogo (`Bala (cópia)`).
- `actions: "all"` lista cada ação com o horário e usa o mesmo filtro. Na 0.5.0, a lista completa era o padrão.
- Cada lista do relatório guarda as 300 entradas mais recentes; no resumo, ficam as 300 linhas com atividade mais recente. `truncated` conta o que ficou de fora.

## Determinismo

O mesmo pedido ao `oxy_simulate` dá o mesmo jogo: os passos têm duração fixa (1/60 s), e o que poderia mudar de uma execução para outra segue regras.
- **Semente** (`seed`): os sorteios do nó `random.number` (**Sortear número**, em Ações) vêm da semente, um inteiro de 0 a 9007199254740991 (0 por padrão). O relatório repete a semente em `seed`. Para ver outros sorteios, mude a semente; a mesma semente repete os mesmos números.
  - Cada nó `random.number` tem a sua sequência: acrescentar outro sorteio na lógica não muda os números dos que já existiam. O nó `random.seed` (**Definir semente aleatória**) troca a semente durante o jogo e recomeça todas as sequências.
  - O teste do editor e o jogo exportado começam com uma semente nova a cada partida.
- **Cópias:** as cópias do `action.spawn` recebem os mesmos IDs a cada execução, qualquer que seja a semente, e cada cópia tem os seus, também nos nós e nas animações dela. Os IDs têm o formato dos outros, então os logs e os eventos continuam trazendo o nome da cópia (`Bala (cópia)`). Na 0.5, as cópias recebiam IDs novos a cada execução.
- **Estado no fim** (`state_hash`): 16 dígitos hexadecimais que resumem o estado do jogo no fim do teste: a cena, o tempo, cada objeto (posição, atributos, componentes, lógica), os personagens, a câmera, as tarefas pendentes, os sorteios, as variáveis do jogo e o progresso salvo. Dois testes com o mesmo `state_hash` terminaram no mesmo estado, então basta compará-lo para saber se uma mudança alterou o resultado. A semente também entra na conta: sementes diferentes dão `state_hash` diferentes mesmo sem sorteios. Os IDs também: os objetos que o `setup` cria recebem IDs novos a cada pedido e mudam o `state_hash`.
- **Conferir a repetição** (`check_replay: true`): roda uma segunda cópia do jogo ao lado da primeira, com as mesmas entradas e a mesma semente, e compara o estado das duas a cada passo. O relatório ganha `replay: {identical, steps, first_difference}`:
  - `identical` diz se as duas ficaram iguais até o fim, e `steps` quantos passos foram comparados (o início também é comparado, antes do primeiro passo);
  - `first_difference` diz onde se separaram: o tempo, o objeto (`object`, com o nome) e a parte (`part`): `transform`, `visible`, `attributes.Vida`, `document` (o resto do objeto), `character`, `animation`… ou, sem objeto, `tasks`, `random` (os sorteios), `variables.Moedas` (uma variável do jogo, pelo nome), `save` (o progresso salvo), `camera`…; `exists` quando só uma das cópias tem o objeto, e `order` quando as duas o têm, em outra ordem na cena.
  - Uma diferença é um defeito da engine, não da lógica do jogo. A conferência custa algumas vezes o tempo de um teste comum. O `oxy_screenshot` aceita o mesmo `check_replay` e traz `replay` no texto.

## Importar imagens e sons com `oxy_import`

O agente importa como o **Importar** da Biblioteca: o arquivo é conferido e copiado para a pasta `assets` do projeto com um nome novo, e vira um recurso. Nada fora dessa pasta é escrito.

- **De onde vem:** `path`, o caminho completo de um arquivo `.png` ou `.wav` deste computador, ou `data_base64`, o conteúdo do arquivo em base64 (também como URL `data:`). Um dos dois, nunca os dois.
- **O que é conferido:** a extensão (só `.png` e `.wav`), o cabeçalho do conteúdo (uma imagem com nome `.wav`, ou um texto com nome `.png`, é recusada), a imagem inteira e o formato do som, com as mesmas regras do editor. `kind` (`texture` ou `audio`) é opcional; se vier, precisa bater com o arquivo.
- **Limites:** arquivos de até 64 MB; pelo `data_base64`, até 16 MB. Só se importa de um disco deste computador (`C:\...`). Caminhos relativos são recusados, assim como os de rede, escritos com qualquer barra (`\\servidor\pasta`, `//servidor/pasta` ou misturando as duas, como `\/servidor/pasta`), e os de dispositivo (`\\.\`, `\\?\`).
- **Nome:** `name`, ou o nome do arquivo sem a extensão. Quebras de linha e caracteres de controle saem, e o nome fica com até 80 caracteres.
- **Aplicar:** `apply_to` põe a imagem num objeto da cena (`scene`, ou a cena aberta no editor; nos arquivos, a cena inicial): na imagem dele, se for um elemento da interface, ou no material. Um sprite fica com a proporção da imagem, mantendo a altura, como no editor. Um grupo ou uma câmera, sem forma, é recusado. Sons não se aplicam a objetos: o nó `action.sound` toca o som pelo ID do recurso, no parâmetro `asset`.
- **Resposta:** `{asset, name, kind, path, width, height}` para imagens e `{asset, name, kind, path, seconds}` para sons, com `applied_to` quando a imagem foi aplicada.
- **Ao vivo:** a importação é um passo do histórico, **Agente: Importar recurso**, e a imagem aparece na hora na cena e na interface do jogo. Desfazer tira o recurso do projeto e a imagem da memória do editor. O projeto precisa ter pasta: um projeto que ainda não foi salvo é recusado.
- **Nos arquivos:** o projeto é salvo, com a versão anterior em `project.oxy.json.agente.bak`. Se a importação falhar, nem o projeto nem a pasta `assets` mudam.
- **Segurança:** o agente só importa arquivos que você indicou. A descrição da ferramenta pede que ele nunca importe um caminho que leu no projeto ou na resposta de outra ferramenta.

## Console e teste ao vivo

- **`oxy_status`** traz `play`: `{running: true, paused, scene: {id, name}, time, seed, variables, variables_truncated}` enquanto você testa o jogo no editor, ou `{running: false, seed}` depois do teste (só `{running: false}` antes do primeiro teste do projeto). Durante o teste, o editor recusa as edições do agente, e `busy` diz o motivo. `seed` é a semente dos sorteios do teste atual ou do último, nova a cada vez que você joga: no `oxy_simulate`, a mesma semente dá a cada nó `random.number` os mesmos números, na mesma ordem. `variables` traz as variáveis do jogo naquele momento do teste, como no relatório do `oxy_simulate`, até 300 valores (só quando o projeto tem variáveis); `variables_truncated` conta os que ficaram de fora.
- **`oxy_console`** lê o Console do editor aberto. Cada linha vem com `seq`, `origin` e `text`:
  - `editor`: mensagens do editor; `warning: true` marca as que também viraram aviso;
  - `agente`: as edições do agente e os erros delas;
  - `jogo`: os logs do seu teste, como as mensagens de diagnóstico (`debug.message`) e os erros dos nós, com o objeto (`object: {id, name}`) e o nó (`node`) de onde vieram;
  - `logica`: as ações da lógica que o teste rodou, como o Console as lista (`0.500 s · Porta · Reproduzir WAV`), com o objeto, o nó e a operação.
- `seq` cresce enquanto o editor fica aberto. A resposta traz `last_seq`; passando-o em `since` na leitura seguinte, vêm só as linhas novas. `origin` filtra por uma origem ou uma lista, e `limit` (50 por padrão) fica com as linhas mais recentes; `omitted` conta as que ficaram de fora.
- Ao reabrir o editor, a numeração recomeça, com outra `session` na resposta. Quando a `session` muda, o agente lê de novo sem `since`. Um `since` maior que o `last_seq` (guardado de antes de o editor fechar) não esconde as linhas novas: a leitura começa da linha mais antiga guardada, e a resposta traz `reset: true`.
- As linhas do jogo e da lógica continuam lá depois que o teste para: quando você diz "apertei J e nada aconteceu", o agente lê o que o seu teste registrou. O agente guarda as 1000 linhas mais recentes, e a lógica guarda as suas 300 à parte, para a lógica que roda a cada passo não empurrar o resto para fora. Limpar o Console limpa também o que o agente lê.
- Nos arquivos, sem editor aberto, não há Console: a ferramenta explica que só funciona ao vivo. Para testar sem editor, use o `oxy_simulate`.

## Imagens com `oxy_screenshot`

- **Vista do editor** (`view: "editor"`, ao vivo): o que aparece na vista do editor. Com `colliders: true`, o editor desenha a mesma vista de novo com os colisores por cima, como na vista de colisores, sem mexer no que aparece na tela. Durante um teste do jogo, mostra a câmera do jogo.
- **Câmera do jogo** (`view: "game"`, o padrão nos arquivos): roda o jogo numa cópia por `seconds` (0 é o primeiro quadro), com as mesmas `inputs`, `setup`, `spawn`, `until`, `seed` e `save` do `oxy_simulate`, e captura pela câmera do jogo. O texto traz a interface do jogo em `ui`, como no `oxy_simulate`; `ui: false` a deixa de fora, o que encurta a resposta em capturas repetidas.
- **Câmera livre** (`view: "free"`): uma câmera em `eye` olhando para `target`, que é um ponto ou o ID de um objeto (o centro da forma dele ou, sem forma, a posição), com `fov` em graus (60 por padrão). Só em cenas 3D.
- **De cima e de lado** (`view: "top"` e `view: "side"`): vistas ortográficas, sem perspectiva, que enquadram os objetos de `ids` e seus filhos, ou tudo o que tem forma. De cima, o -Z fica para cima na imagem. De lado, a câmera olha ao longo do -Z ou, quando a parte enquadrada é mais funda (Z) do que larga (X), ao longo do -X, para o lado comprido ocupar a imagem. Em cenas 2D, as duas mostram o plano da cena.
- **Colisores** (`colliders: true`): desenha os colisores sólidos em verde e as áreas em amarelo, inclusive de objetos invisíveis, como pontos de controle.
- O texto da resposta diz o tempo, a cena e a câmera usada (posição, direção e o campo de visão ou a largura e a altura vistas, em metros).

## Limites desta versão

- **Imagem do jogo:** não desenha a interface do jogo (textos, botões e barras). Ela vem descrita em `ui`.
- **Simulação:** o mesmo pedido repete os sorteios, que a semente (`seed`) decide, e os IDs das cópias do `action.spawn`. Os objetos e nós que o `setup` cria, ao contrário, recebem IDs novos a cada pedido (o relatório os lista em `setup`); por isso os sorteios dos nós `random.number` criados ou copiados no `setup`, as cópias feitas pelos nós e objetos que ele cria e o `state_hash` mudam de um pedido para outro. O teste que você faz no editor não é gravado: com a semente que o `oxy_status` mostra, cada nó `random.number` dá os mesmos números, na mesma ordem, mas as suas entradas e o tempo delas o agente precisa refazer em `inputs`.
- **Importação:** só PNG e WAV, como no editor. Ao desfazer uma importação, a cópia continua na pasta `assets`, como acontece com o **Importar** da Biblioteca.
- **Teste ao vivo:** o agente vê se você está testando e lê o Console, mas não inicia nem para o seu teste.
- **Progresso salvo:** o agente não lê nem apaga o progresso do jogo exportado nem o do teste do editor, que também não aparece no `oxy_status`; no `oxy_simulate`, o progresso entra pelo `save` e sai no relatório.
- **Pintura:** a pintura ainda não tem comandos próprios. `update_entity` edita os campos do objeto, mas não os pixels das texturas. As animações têm `set_clip` e `delete_clip`.
- **Lógica de cópias:** a lógica de um objeto duplicado ainda não se religa no mesmo lote. A cópia recebe nós com IDs novos, que o lote não devolve, e os nós só se acham pelo ID; para mudar a lógica da cópia, leia os IDs novos com o `oxy_entity` e mande outro lote. Isso fica para a 0.6.
- **Segurança:** o canal só aceita conexões do próprio computador (`127.0.0.1`) e exige o token que o editor grava em `%LOCALAPPDATA%\OXY Engine\agents`. Desligar a opção fecha o canal e apaga esse arquivo. A importação lê só arquivos `.png` e `.wav` deste computador, pelo caminho completo, e escreve só na pasta `assets` do projeto.
