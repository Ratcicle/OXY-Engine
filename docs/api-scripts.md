# API dos scripts

<!-- Gerado da tabela da API (crates/oxy_core/src/script/registry.rs). Não edite: rode os testes do oxy_core com OXY_UPDATE_API=1 para gravar de novo. -->

A referência das funções que os scripts em Luau alcançam, gerada da mesma tabela que instala a API no jogo e que escreve os tipos do VS Code (`.oxy/tipos/oxy.d.luau`). Os nomes da API são em inglês; os de objetos, atributos, sons, ações e variáveis são os do projeto, e as mensagens de erro são em português, com o arquivo e a linha. Como escrever, anexar e testar scripts está em [Agentes de IA (MCP)](agentes-mcp.md#scripts-com-o-agente).

Cada exemplo usa um módulo chamado `M` e roda num teste da engine. **0.8** diz onde a função vai rodar com vários jogadores.

## Eventos

Funções do módulo com nomes fixos, que a engine chama sozinha; `self` é o objeto que anexa o script. Rodam na ordem da cena, depois da lógica de nós do objeto e na ordem da lista de scripts dele. Uma função com nome de evento que não existe (`on_aera_enter`) nunca roda, e a conferência avisa.

### `on_start(self: Object)`

A cena começou, ou uma cópia com o script surgiu.

Roda uma vez em cada objeto que anexa o script: no começo da cena, na ordem da cena, depois que todos os objetos dela existem; numa cópia criada durante o jogo, quando ela surge. As variáveis do jogo guardam o valor quando a cena reinicia: zere aqui o que precisa recomeçar.

- Nós: Ao iniciar cena (event.scene_start).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
function M.on_start(self: Object)
    self.attr.Vida = 3
end
```

### `on_step(self: Object, dt: number)`

A cada passo de jogo, 60 vezes por segundo.

Não roda com o jogo pausado. Para esperar, use um temporizador (`self:after`) em vez de somar `dt` todo passo.

- `dt` (`number`): a duração do passo, 1/60 s
- Nós: A cada passo da simulação (event.step).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
function M.on_step(self: Object, dt: number)
    self.position += vector.create(0, 0, 2 * dt)
end
```

### `on_input(self: Object, action: string, phase: "pressed" | "released")`

Uma ação de entrada foi apertada ou solta.

Chega a todos os objetos cujos scripts definem a função.

- `action` (`string`): o nome da ação, como em Lógica > Ações de entrada
- `phase` (`"pressed" | "released"`): "pressed" ao apertar, "released" ao soltar
- Nós: Ação de entrada (event.input).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_input(self: Object, action: string, phase: string)
    if action == "Pular" and phase == "pressed" then
        print("pulou")
    end
end
```

### `on_click(self: Object)`

Um clique no objeto ou no botão de interface dele.

O objeto precisa ser clicável: um objeto visual ou um botão de interface.

- Nós: Ao clicar (event.click).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_click(self: Object)
    audio.play("Bipe")
end
```

### `on_area_enter(self: Object, other: Object)`

Outro objeto entrou na área do objeto.

A área é um colisor marcado como área (sensor), e o outro objeto precisa de um colisor também. Qualquer objeto com colisor entra, como uma caixa de física: confira um atributo dele antes de agir.

- `other` (`Object`): o objeto que entrou
- Nós: Ao entrar na área (event.area_enter).
- 0.8: M (partida). Desde a 0.7.0.

```lua
function M.on_area_enter(self: Object, other: Object)
    if other:has_attribute("Vida") then
        other.attr.Vida -= 1
    end
end
```

### `on_area_exit(self: Object, other: Object)`

Outro objeto saiu da área do objeto.

Como `on_area_enter`, na saída.

- `other` (`Object`): o objeto que saiu
- Nós: Ao sair da área (event.area_exit).
- 0.8: M (partida). Desde a 0.7.0.

```lua
function M.on_area_exit(self: Object, other: Object)
    print(other.name, "saiu")
end
```

### `on_marker(self: Object, marker: string, clip: string)`

Uma animação do objeto passou por um marcador.

Os nomes são os do Estúdio do modelo.

- `marker` (`string`): o nome do marcador
- `clip` (`string`): o nome da animação
- Nós: Marcador de animação (event.animation).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
function M.on_marker(self: Object, marker: string, clip: string)
    print(clip, marker)
end
```

### `on_character(self: Object, event: CharacterEvent)`

Um evento do personagem 3D do objeto: pulo, pouso e outros.

Só do personagem do próprio objeto: quem quer saber do personagem de outro objeto recebe uma mensagem dele.

- `event` (`CharacterEvent`): o que houve (`event.kind`) e os valores do evento
- Nós: Evento do personagem 3D (event.character).
- 0.8: M (partida). Desde a 0.7.0.

```lua
function M.on_character(self: Object, event: CharacterEvent)
    if event.kind == "land" then
        print("pousou", event.impact)
    end
end
```

### `on_pause(self: Object, paused: boolean)`

O jogo entrou ou saiu da pausa dele.

Os temporizadores param com o jogo pausado e voltam a contar quando ele retoma.

- `paused` (`boolean`): true ao pausar, false ao retomar
- Nós: Ao pausar ou retomar (event.pause).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_pause(self: Object, paused: boolean)
    self.attr.Aviso = if paused then "Pausado" else ""
end
```

### `on_message(self: Object, name: string, value: any, from: Object)`

Uma mensagem que um objeto mandou com `obj:send`.

Chega na fase seguinte da lógica, depois dos eventos e temporizadores dela, na ordem em que as mensagens foram mandadas. Mandar a um objeto sem `on_message` em nenhum script gera um aviso, uma vez por linha que manda.

- `name` (`string`): o nome da mensagem
- `value` (`any`): uma cópia do valor mandado; nil sem valor
- `from` (`Object`): o objeto que mandou
- 0.8: regras das mensagens. Desde a 0.7.0.

```lua
function M.on_message(self: Object, name: string, value: any, from: Object)
    print(from.name, name, value)
end
```

## Objeto

Os objetos chegam aos scripts como valores estáveis: `self`, o `other` dos eventos, os atributos Objeto, o remetente das mensagens e o que `scene` acha. Um objeto removido continua sendo um valor no script, mas ler ou escrever nele é um erro (`obj:exists()` diz se ele ainda está na cena).

### `obj.id: string`

O ID do objeto: fixo no projeto e, numa cópia feita no jogo, derivado do original.

Só leitura. Dois objetos podem ter o mesmo nome, nunca o mesmo ID: serve de chave para guardar algo por objeto. Para mostrar um objeto a quem joga, use `name`.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.name, self.id)
```

### `obj.name: string`

O nome do objeto na cena.

Só leitura. `print` e `tostring` mostram um objeto por esse nome. A raiz de uma cópia de `obj:clone` leva " (cópia)" no fim do nome, como as do nó Criar cópia de objeto, e o que está nela fica com os nomes do original; as cópias de `scene.spawn_model` ficam com os nomes do modelo.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.name)
```

### `obj.parent: Object?`

O pai do objeto na Hierarquia, ou nil num objeto da raiz.

Só leitura. Um script compartilhado por vários objetos alcança o grupo de cada um por aqui, sem uma referência fixa.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local pai = self.parent
if pai then
    print(pai.name)
end
```

### `obj:children(): { Object }`

Os filhos diretos do objeto, na ordem da cena.

Uma lista nova a cada chamada: guardá-la ou mudá-la não muda a cena.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
for _, filho in self:children() do
    print(filho.name)
end
```

### `obj:find_child(name: string, deep: boolean?): Object?`

O primeiro filho com o nome, na ordem da cena, ou nil.

Para quando a ausência é esperada; um filho fixo de um modelo se pega com `get_child`, que dá um erro claro quando ele falta.

- `name` (`string`): o nome do filho
- `deep` (`boolean?`): true procura em tudo o que está abaixo do objeto, não só nos filhos
- Para com erro: o nome não é um texto; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local alca = self:find_child("Maçaneta")
if alca then
    alca.visible = false
end
```

### `obj:get_child(name: string, deep: boolean?): Object`

O primeiro filho com o nome, na ordem da cena; um erro se não houver.

Como `find_child`, mas o filho é certo: o erro diz o nome mais parecido e os filhos que existem (um filho renomeado quebra o script na linha, e não depois).

- `name` (`string`): o nome do filho
- `deep` (`boolean?`): true procura em tudo o que está abaixo do objeto, não só nos filhos
- Para com erro: nenhum filho com o nome (o erro lista os filhos); o nome não é um texto; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self:get_child("Maçaneta").visible = false
```

### `obj:exists(): boolean`

Diz se o objeto ainda está na cena.

Um objeto removido continua sendo um valor no script (num atributo, na memória ou numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.

- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self:exists() then
    print("ainda aqui")
end
```

### `obj.visible: boolean`

Se o objeto aparece: false o esconde, true o mostra. Leitura e escrita.

Como o nó Ativar / desativar componente (Visibilidade). Esconder um grupo esconde o que está nele, mas o `visible` dos filhos continua true; um botão escondido (ou num grupo escondido) não recebe cliques.

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.visible = false
```

### `obj.logic_enabled: boolean`

Se a lógica de nós (o grafo) do objeto roda; os scripts dele seguem. Leitura e escrita.

Como o nó Ativar / desativar componente (Comportamento visual): false para o grafo e descarta as esperas dele; um erro no grafo também o põe em false. Os scripts têm o próprio liga e desliga.

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.logic_enabled = false
```

### `obj:has_component(kind: "animation" | "area" | "camera" | "character" | "collider" | "controller" | "platform" | "ui"): boolean`

Diz se o objeto tem o componente, ligado ou não.

Confira antes de agir sobre outro objeto, como o que entrou numa área: um personagem, uma caixa de física.

- `kind` (`"animation" | "area" | "camera" | "character" | "collider" | "controller" | "platform" | "ui"`): o componente: animation (tem animações), area (colisor marcado como área), camera, character (Personagem 3D), collider (colisor 2D ou 3D, área incluída), controller (Movimento 2D), platform (plataforma móvel) ou ui (elemento de interface)
- Para com erro: um componente que não é um dos da lista (o erro lista os nomes); o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self:has_component("collider") then
    print("tem colisor")
end
```

### `obj:destroy()`

Tira o objeto e tudo o que está nele da cena, na hora.

Como o nó Remover objeto. Os temporizadores, as mensagens e a memória dele vão junto. `self:destroy()` encerra a chamada em silêncio no próximo uso de `self`; o que vem antes vale.

- Para com erro: o objeto não existe mais.
- Nós: Remover objeto (action.remove).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self:destroy()
```

### `obj:clone(options: SpawnOptions?): Object`

Copia o objeto e tudo o que está nele, como está agora, e devolve a cópia.

Como o nó Criar cópia de objeto: a cópia fica no mesmo pai e no mesmo lugar do original (`offset` a desloca nos eixos do pai; `position` a põe num lugar), com os atributos, os scripts e a lógica dele; a memória dos scripts nasce com os valores declarados, e o `on_start` deles roda depois desta chamada. Os IDs da cópia saem deste objeto, deste script e de quantas cópias ele já fez: a mesma partida dá os mesmos IDs.

- `options` (`SpawnOptions?`): `{ offset = v }` ou `{ position = v }`, e `rotation`, `parent` e `attributes`
- Para com erro: uma opção desconhecida, offset com position, ou um valor de outro tipo; um atributo em `attributes` que a cópia não tem, ou de outro tipo (nada é criado); o objeto não existe mais.
- Nós: Criar cópia de objeto (action.spawn).
- 0.8: M (partida). Desde a 0.7.0.

```lua
if self.attr.Vida == 3 then
    local copia = self:clone({ offset = vector.create(2, 0, 0) })
    copia.attr.Vida = 1
end
```

### `obj:first_hit(other: Object): boolean`

true só na primeira vez que `other` é atingido nesta ativação da área do objeto.

A regra "Uma vez por ativação" do nó Aplicar dano, sem o dano: a área conta quem já atingiu até ser desligada e ligada de novo, e os acertos do grafo e dos scripts somam juntos. Em `on_area_enter` e `on_area_exit` da própria área, conta na ativação do evento, como o nó, mesmo que a área tenha desligado depois; fora deles, na ativação de agora, e uma área desligada dá false. O dano vira uma conta: `other.attr.Vida -= 25`.

- `other` (`Object`): o objeto atingido
- Para com erro: o objeto não é uma área (um colisor marcado como área); other não é um objeto ou não existe mais.
- Nós: Aplicar dano (action.damage).
- 0.8: M (partida). Desde a 0.7.0.

```lua
function M.on_area_enter(self: Object, other: Object)
    if other:has_attribute("Vida") and self:first_hit(other) then
        other.attr.Vida -= 1
    end
end
```

### `obj:send(name: string, value: any?)`

Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.

A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os scripts da cena, 4.096.

- `name` (`string`): o nome da mensagem, de 1 a 256 bytes
- `value` (`any?`): o valor, copiado: nil, números, textos, booleanos, vetores, objetos e tabelas desses valores, até 4 KB
- Para com erro: o alvo não é um objeto ou não existe mais; nome vazio ou com mais de 256 bytes; valor que uma mensagem não leva (função, tabela com metatabela, mais de 4 KB); mais de 1.024 mensagens do objeto no passo: o script para no objeto; mais de 4.096 mensagens na cena no passo: só esta chamada falha.
- 0.8: regras das mensagens. Desde a 0.7.0.

```lua
self:send("abrir", 2)
```

## Cena

A cena que está rodando: achar objetos pelo nome ou por um atributo, pôr modelos da Biblioteca nela e trocar de cena. As listas vêm na ordem da cena.

### `scene.find(name: string): Object?`

O primeiro objeto da cena com o nome, na ordem da cena, ou nil.

O nome é o exato, com maiúsculas e espaços. Para apontar um objeto que pode ser renomeado, prefira um atributo Objeto (`param.object()`), escolhido nas Propriedades.

- `name` (`string`): o nome do objeto, como na Hierarquia
- Para com erro: o nome não é um texto.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local porta = scene.find("Porta")
if porta then
    print(porta.name)
end
```

### `scene.get(name: string): Object`

O primeiro objeto da cena com o nome; um erro se não houver.

Como `scene.find`, para um objeto que tem de existir: o erro diz o nome mais parecido da cena.

- `name` (`string`): o nome do objeto, como na Hierarquia
- Para com erro: nenhum objeto com o nome (com o nome mais parecido); o nome não é um texto.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
scene.get("Porta").attr.Vida = 2
```

### `scene.find_all(name: string): { Object }`

Todos os objetos da cena com o nome, na ordem da cena.

Uma lista nova a cada chamada, vazia quando nenhum tem o nome.

- `name` (`string`): o nome dos objetos, como na Hierarquia
- Para com erro: o nome não é um texto.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
for _, porta in scene.find_all("Porta") do
    porta.attr.Vida = 3
end
```

### `scene.with_attribute(name: string): { Object }`

Todos os objetos da cena que têm o atributo, na ordem da cena.

Um atributo serve de marca: os inimigos de uma onda, os interruptores de uma porta.

- `name` (`string`): o nome do atributo
- Para com erro: o nome não é um texto.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
for _, alvo in scene.with_attribute("Vida") do
    alvo.attr.Vida -= 1
end
```

### `scene.spawn_model(model: string, options: SpawnOptions?): Object`

Põe uma cópia de um modelo da Biblioteca na cena e devolve a raiz dela.

O nome do modelo vale sem diferenciar maiúsculas. A cópia, com os nomes que os objetos têm no modelo, vai para a raiz da cena (ou para `parent`), na posição salva no modelo (ou em `position`, nos eixos do pai); o `on_start` dos scripts dela roda depois desta chamada. Os IDs saem deste objeto, deste script e de quantas cópias ele já fez, como em `obj:clone`.

- `model` (`string`): o nome do modelo na Biblioteca
- `options` (`SpawnOptions?`): `{ position = v }`, e `rotation`, `parent` e `attributes`
- Para com erro: um modelo que o projeto não tem (o erro lista os modelos); um modelo de formas 2D numa cena 3D, ou o contrário; uma opção desconhecida ou um valor de outro tipo; um atributo em `attributes` que a raiz da cópia não tem (nada é criado).
- 0.8: M (partida). Desde a 0.7.0.

```lua
local bala = scene.spawn_model("Bala", { position = vector.create(0, 1, 0) })
print(bala.name)
```

### `scene.current(): string`

O nome da cena que está rodando.

O mesmo nome que `scene.change` recebe.

- Nós: Cena atual (scene.current).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(scene.current())
```

### `scene.change(name: string)`

Abre outra cena quando a função termina.

O nome vale sem diferenciar maiúsculas. O resto da função ainda roda na cena atual; depois ela fecha, com os temporizadores e as mensagens dela, e a nova começa, como no nó Mudar de cena. Chamar de novo na mesma função troca o destino. As variáveis do jogo continuam.

- `name` (`string`): o nome da cena, como no projeto
- Para com erro: uma cena que o projeto não tem (com o nome mais parecido e a lista); duas cenas com o mesmo nome; o nome não é um texto.
- Nós: Mudar de cena (action.scene), Referência de cena (value.scene).
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
scene.change("Menu")
```

### `scene.restart()`

Começa a cena atual de novo quando a função termina.

Como `scene.change(scene.current())`. As variáveis do jogo guardam o valor: zere no `on_start` o que precisa recomeçar.

- Nós: Mudar de cena (action.scene), Cena atual (scene.current).
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
if vars.get("Pontos") == 0 then
    vars.set("Pontos", 1)
    scene.restart()
end
```

## Transformação

Onde o objeto está e como está girado, em metros e graus, como vetores do Luau (`vector.create(x, y, z)`) e quatérnios (`quat`). Escrever passa pelas mesmas regras do nó Mover / girar / escalar objeto.

### `obj.position: vector`

A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram. Leitura e escrita.

Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma animação tocando posiciona.

- Para com erro: escrever outra coisa que um vetor; mover um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Ler posição e orientação (transform.read), Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.position = vector.create(0, 2, 0)
```

### `obj.world_position: vector`

A posição do pivô do objeto no mundo, em metros. Leitura e escrita.

Num personagem 3D, a origem dele. Escrever move o objeto como `position`.

- Para com erro: escrever outra coisa que um vetor; mover um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Ler posição e orientação (transform.read), Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
print(self.world_position)
```

### `obj.rotation: vector`

A rotação do objeto em graus, nos eixos X, Y e Z do pai, como as Propriedades mostram. Leitura e escrita.

Escrever gira o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa. Para girar aos poucos ou combinar giros, use `orientation` e `quat`.

- Para com erro: escrever outra coisa que um vetor; girar um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Ler posição e orientação (transform.read), Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.rotation = vector.create(0, 90, 0)
```

### `obj.orientation: Quat`

A rotação do objeto como quatérnio, nos eixos do pai. Leitura e escrita.

Escrever gira o objeto como `rotation`, com os ângulos mais perto dos de agora. As funções de `quat` combinam e interpolam giros.

- Para com erro: escrever outra coisa que um quatérnio; girar um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Ler posição e orientação (transform.read), Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.orientation = quat.from_euler(0, 45, 0)
```

### `obj.scale: vector`

A escala do objeto em X, Y e Z, relativa ao pai, como as Propriedades mostram. Leitura e escrita.

Escrever muda o tamanho, como o nó Mover / girar / escalar objeto: cada eixo fica acima de 0,00001 em valor absoluto.

- Para com erro: escrever outra coisa que um vetor; uma escala que chega a zero; escalar um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.scale = vector.create(2, 2, 2)
```

### `obj.yaw: number`

O giro horizontal do objeto no mundo, em graus: para onde a frente (-Z) aponta.

Só leitura, como o Giro horizontal do nó Ler posição e orientação: 0 olha para -Z e 90 para -X. Um personagem 3D gira com o giro do personagem.

- Para com erro: o objeto não existe mais.
- Nós: Ler posição e orientação (transform.read).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.yaw)
```

### `obj:jump_to(position: vector)`

Leva o objeto para uma posição do mundo de uma vez, como um salto.

Como escrever `world_position`, mas as plataformas móveis do objeto e do que está nele soltam quem está em cima, sem lançá-lo (o modo Salto do nó Mover / girar / escalar objeto).

- `position` (`vector`): onde o pivô do objeto fica, no mundo, em metros
- Para com erro: a posição não é um vetor; mover um personagem 3D ou um objeto que uma animação tocando posiciona; o objeto não existe mais.
- Nós: Mover / girar / escalar objeto (transform.write).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self:jump_to(vector.create(0, 5, 0))
```

### `obj:to_world_direction(direction: vector): vector`

Uma direção nos eixos do objeto, girada para os eixos do mundo.

Só a rotação conta (a escala e a posição não): a frente do objeto no mundo é `self:to_world_direction(vector.create(0, 0, -1))`.

- `direction` (`vector`): uma direção nos eixos do objeto, como vector.create(0, 0, -1), a frente
- Para com erro: a direção não é um vetor; o objeto não existe mais.
- Nós: Transformar direção entre espaços (vector.direction).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self:to_world_direction(vector.create(0, 0, -1)))
```

### `obj:to_local_direction(direction: vector): vector`

Uma direção do mundo, girada para os eixos do objeto.

O contrário de `to_world_direction`: só a rotação conta.

- `direction` (`vector`): uma direção nos eixos do mundo
- Para com erro: a direção não é um vetor; o objeto não existe mais.
- Nós: Transformar direção entre espaços (vector.direction).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self:to_local_direction(vector.create(0, 0, -1)))
```

## Matemática

O que a OXY soma à biblioteca do Luau (`vector`, `math`): `vmath`, para vetores, e `quat`, para giros. Um quatérnio nunca muda: toda função devolve um novo.

### `vmath.lerp(a: vector, b: vector, t: number): vector`

O vetor entre `a` e `b` na fração `t`: a + (b - a) * t.

O mesmo que o `vector.lerp` do Luau; `math.lerp` faz o mesmo com números.

- `a` (`vector`): o vetor de partida
- `b` (`vector`): o vetor de chegada
- `t` (`number`): 0 dá a, 1 dá b; fora disso, além deles
- Para com erro: a ou b que não é um vetor, t que não é um número.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(vmath.lerp(vector.zero, vector.one, 0.5))
```

### `vmath.distance(a: vector, b: vector): number`

A distância entre dois pontos, em metros.

O comprimento de `b - a`, em precisão dupla.

- `a` (`vector`): um ponto
- `b` (`vector`): outro ponto
- Para com erro: a ou b que não é um vetor.
- Nós: Operações com Vetor3 (vector3.math).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(vmath.distance(self.position, vector.create(0, 0, 0)))
```

### `vmath.normalize_or_zero(v: vector): vector`

O vetor com comprimento 1, ou zero quando ele não tem direção.

Como o A normalizado (zero seguro) do nó Operações com Vetor3: um vetor zero dá zero, sem NaN (o `vector.normalize` do Luau dá NaN).

- `v` (`vector`): o vetor
- Para com erro: v que não é um vetor.
- Nós: Operações com Vetor3 (vector3.math).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(vmath.normalize_or_zero(vector.create(3, 0, 4)))
```

### `vmath.flat(v: vector): vector`

O vetor no chão: o mesmo com y = 0.

Para direções e distâncias no plano, sem a altura.

- `v` (`vector`): o vetor
- Para com erro: v que não é um vetor.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(vmath.flat(vector.create(1, 2, 3)))
```

### `vmath.yaw_of(direction: vector): number`

O giro horizontal, em graus, que aponta a frente de um objeto para a direção.

O mesmo giro de `obj.yaw`: 0 olha para -Z e 90 para -X; a altura não conta, e uma direção vertical ou zero dá 0. `obj.rotation = vector.create(0, vmath.yaw_of(d), 0)` vira o objeto para `d`.

- `direction` (`vector`): a direção, no mundo
- Para com erro: direction que não é um vetor.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.rotation = vector.create(0, vmath.yaw_of(vector.create(-1, 0, 0)), 0)
```

### `quat.identity(): Quat`

O quatérnio sem giro.

Um quatérnio nunca muda; feito no topo do script, é uma constante.

- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.identity()
```

### `quat.from_euler(x: number, y: number, z: number): Quat`

O giro dos ângulos X, Y e Z em graus, na ordem das Propriedades.

`quat.from_euler(0, 90, 0)` é o mesmo giro que `rotation = vector.create(0, 90, 0)`.

- `x` (`number`): graus em X
- `y` (`number`): graus em Y
- `z` (`number`): graus em Z
- Para com erro: um ângulo que não é um número.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.from_euler(0, 90, 0)
```

### `quat.from_axis_angle(axis: vector, degrees: number): Quat`

O giro de `degrees` graus em volta de um eixo.

Visto da ponta do eixo, um ângulo positivo gira no sentido anti-horário.

- `axis` (`vector`): o eixo do giro (qualquer comprimento)
- `degrees` (`number`): o ângulo, em graus
- Para com erro: axis que não é um vetor ou é zero; degrees que não é um número.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.from_axis_angle(vector.create(0, 1, 0), 90)
```

### `quat.look_at(forward: vector, up: vector?): Quat`

O giro que aponta a frente de um objeto (-Z) para uma direção.

O topo fica o mais perto possível de `up`.

- `forward` (`vector`): para onde a frente (-Z) deve olhar
- `up` (`vector?`): para onde o topo (+Y) deve ficar; +Y sem ele
- Para com erro: forward ou up que não é um vetor, forward zero; forward na mesma direção de up.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.look_at(vector.create(1, 0, 0))
```

### `quat.slerp(a: Quat, b: Quat, t: number): Quat`

O giro entre `a` e `b` na fração `t`, pelo caminho mais curto.

Para girar aos poucos: chame a cada passo com `t` crescendo.

- `a` (`Quat`): o giro de partida
- `b` (`Quat`): o giro de chegada
- `t` (`number`): 0 dá a, 1 dá b
- Para com erro: a ou b que não é um quatérnio, t que não é um número.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.slerp(quat.identity(), quat.from_euler(0, 90, 0), 0.5)
```

### `quat.multiply(a: Quat, b: Quat): Quat`

O giro `b` seguido do giro `a`.

O operador `a * b` faz o mesmo, mas os tipos do VS Code não o conhecem: com `--!strict`, use a função.

- `a` (`Quat`): o giro de fora
- `b` (`Quat`): o giro aplicado primeiro
- Para com erro: a ou b que não é um quatérnio.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.multiply(self.orientation, quat.from_euler(0, 10, 0))
```

### `quat.rotate(q: Quat, v: vector): vector`

O vetor `v` girado por `q`.

O operador `q * v` faz o mesmo, mas os tipos do VS Code não o conhecem: com `--!strict`, use a função.

- `q` (`Quat`): o giro
- `v` (`vector`): o vetor a girar
- Para com erro: q que não é um quatérnio ou v que não é um vetor.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(quat.rotate(quat.from_euler(0, 90, 0), vector.create(0, 0, -1)))
```

### `quat.inverse(q: Quat): Quat`

O giro contrário de `q`.

`quat.multiply(quat.inverse(q), q)` não gira nada.

- `q` (`Quat`): o giro
- Para com erro: q que não é um quatérnio.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.orientation = quat.inverse(self.orientation)
```

### `quat.to_euler(q: Quat): vector`

Os ângulos X, Y e Z em graus do giro, na ordem das Propriedades.

O contrário de `quat.from_euler`. É o que a memória, os temporizadores e as mensagens guardam de um giro: eles não guardam quatérnios.

- `q` (`Quat`): o giro
- Para com erro: q que não é um quatérnio.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(quat.to_euler(self.orientation))
```

## Atributos

Os atributos do objeto, os mesmos das Propriedades: a ponte com a interface, o grafo e os outros objetos. O script declara os que espera em `Modulo.attributes`, e anexá-lo cria no objeto os que faltam.

### `obj.attr: { [string]: any }`

Os atributos do objeto, pelo nome.

`obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto lê o objeto, e um Superfície, a superfície (com `.name`), ou nil quando vazios, e recebem nil para ficar vazios. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`. Quando a chamada para num erro com nil, o erro diz o último atributo que ela leu como nil.

- Para com erro: escrever num atributo que o objeto não tem (o erro diz como criá-lo); escrever um valor de outro tipo; escrever infinito ou NaN; o objeto não existe mais.
- Nós: Ler atributo (attribute.get), Alterar atributo (attribute.set).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.attr.Vida -= 1
```

### `obj:has_attribute(name: string): boolean`

Diz se o objeto tem o atributo.

Confira antes de agir sobre outro objeto, como o que entrou numa área.

- `name` (`string`): o nome do atributo
- Para com erro: o nome não é um texto; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self:has_attribute("Vida") then
    print("tem vida")
end
```

### `Modulo.attributes: { [string]: any }`

Os atributos que o script espera no objeto, com o valor padrão de cada um.

O tipo sai do padrão: número, texto, booleano ou vetor (Vetor3); para os tipos sem valor literal, `param.object()`, `param.surface(nome?)` e `param.vector2(x, y)`. Anexar o script cria no objeto, no mesmo passo de desfazer, os que ele não tem; um que ele tem com outro tipo fica como está, com um aviso, e dois scripts do objeto que declaram o mesmo nome com tipos diferentes também geram um aviso. Um atributo declarado depois de anexar só aparece ao anexar de novo.

- Para com erro: um padrão que não é número, texto, booleano, vetor nem marcador `param`: anexar avisa e não cria nenhum; `param.surface` com o nome de uma superfície que o projeto não tem: anexar avisa e não cria nenhum.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Velocidade = 90, Aviso = "", Alvo = param.object(), Piso = param.surface() }
```

### `param.object(): Object?`

Declara um atributo Objeto, vazio até alguém escolher o objeto.

Em `Modulo.attributes = { Alvo = param.object() }`: anexar cria o atributo vazio, e o objeto se escolhe nas Propriedades (ou com `set_attribute` pelo MCP), o que sobrevive a renomear o objeto. Lido no script, é o objeto ou nil. Em `Modulo.state`, declara um campo de objeto que começa vazio.

- Nós: Referência de objeto (value.object).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Alvo = param.object() }
```

### `param.surface(name: string?): Surface?`

Declara um atributo Superfície, com a superfície do projeto de nome `name`.

Em `Modulo.attributes = { Piso = param.surface("Gelo") }`: anexar procura a superfície pelo nome (sem diferenciar maiúsculas) e avisa, sem criar nada, quando o projeto não a tem. Lido no script, é a superfície (com `.name`) ou nil para a padrão. Em `Modulo.state`, só `param.surface()`: um campo que começa vazio.

- `name` (`string?`): o nome de uma superfície do projeto; sem ele, a superfície padrão
- Para com erro: um nome que não é um texto.
- Nós: Referência de superfície (value.surface).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Piso = param.surface("Gelo") }
```

### `param.vector2(x: number, y: number): vector`

Declara um atributo Vetor2 com o valor padrão (x, y).

Em `Modulo.attributes = { Tamanho = param.vector2(1, 1) }`, e num campo de `Modulo.state`. Lido no script, um Vetor2 é um vetor com z = 0; escrito, recebe o x e o y de um vetor.

- `x` (`number`): o x padrão
- `y` (`number`): o y padrão
- Para com erro: x ou y que não é um número, ou que um Vetor2 não guarda (infinito, NaN, além de ±3,4e38).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Tamanho = param.vector2(1, 1) }
```

## Memória do script

O que o script guarda em cada objeto e só ele lê, declarado em `Modulo.state`. A engine guarda a memória: ela atravessa a recarga do script. O que outro objeto, a interface ou o grafo leem vai num atributo.

### `Modulo.state: { [string]: any }`

Os campos da memória do script em cada objeto, com o valor inicial de cada um.

Valores: booleano, número, texto, vetor, objeto, superfície e tabelas desses valores, com chaves inteiras ou de texto, até 4 níveis e 1.024 entradas por tabela; até 64 KB por objeto. Um campo precisa de um valor inicial de verdade (um nil some da tabela); um campo de objeto ou de superfície que começa vazio e um Vetor2 se declaram com os marcadores: `alvo = param.object()`, `piso = param.surface()`, `tamanho = param.vector2(1, 1)`.

- Para com erro: um valor que a memória não guarda (função, tabela com metatabela, quatérnio, marcador `param` dentro de uma tabela, `param.surface` com nome): o script não carrega; mais de 64 KB: o script não carrega.
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
M.state = { pulos = 0, nomes = {} :: { string }, alvo = param.object() }
```

### `obj.state: any`

A memória deste script neste objeto, com os campos de `Modulo.state`.

Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto, superfície) é lido e gravado na engine na hora, conferido na linha; um campo de objeto ou de superfície recebe nil para ficar vazio. Uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.

- Para com erro: o script não declara `Modulo.state`; um campo que `Modulo.state` não declara; um valor de outro tipo que o do campo; a memória de outro objeto: só `self.state` existe; mais de 64 KB no objeto: o script para no objeto.
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self.state.contagem += 1
```

## Temporizadores

Esperar sem parar a função: a engine guarda o nome da função do módulo e os dados, e a chama no tempo de jogo certo, que para com o jogo pausado. Só no próprio objeto; caem quando o objeto sai, quando a cena troca e quando o script para no objeto.

### `obj:after<X...>(seconds: number, fn: (Object, X...) -> (), ...: X...): number`

Chama `fn(self, ...)` depois de `seconds` segundos de jogo.

No mínimo no passo seguinte, como o nó Esperar sem bloquear: os dois, começados no mesmo passo com o mesmo tempo, disparam no mesmo passo. A engine guarda o nome da função e os dados, então o temporizador atravessa a recarga do script. Devolve o número que `cancel` recebe.

- `seconds` (`number`): segundos de jogo, de 0 a 86400
- `fn` (`(Object, X...) -> ()`): uma função da tabela do módulo, como `M.fechar`
- `...` (`X...`): os dados que `fn` recebe depois de `self`
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor, objeto nem superfície, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
- Nós: Esperar sem bloquear (control.wait).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self:after(2, M.fechar)
```

### `obj:every<X...>(seconds: number, fn: (Object, X...) -> (), ...: X...): number`

Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.

A primeira chamada sai depois de `seconds`. As regras de `after` valem aqui.

- `seconds` (`number`): o intervalo em segundos de jogo
- `fn` (`(Object, X...) -> ()`): uma função da tabela do módulo, como `M.mostrar`
- `...` (`X...`): os dados que `fn` recebe depois de `self`
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor, objeto nem superfície, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
- Nós: Esperar sem bloquear (control.wait).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self:every(1, M.mostrar, 1)
```

### `obj:sequence<T>(interval: number, fn: (Object, T) -> (), items: { T }): number`

Chama `fn(self, item)` com cada item da lista, um a cada `interval` segundos.

O item k sai aos (k-1) × `interval` segundos da chamada, como `self:after` chamaria: o primeiro no passo seguinte, como `self:after(0)`, e os outros na grade das esperas do grafo. O estado que precisa valer já neste passo, grave direto antes; o primeiro item o grava de novo, e um efeito dele (um som) sairia duas vezes.

- `interval` (`number`): segundos de jogo entre um item e o seguinte
- `fn` (`(Object, T) -> ()`): uma função da tabela do módulo, que recebe cada item
- `items` (`{ T }`): a lista de itens, até 1.024
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor, objeto nem superfície, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
- Nós: Esperar sem bloquear (control.wait).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self:sequence(1, M.mostrar, { 3, 2, 1 })
```

### `obj:schedule(steps: { Step }): number`

Passos com atrasos próprios: `{ {atraso, M.f, dado?}, ... }`.

Cada atraso conta do passo anterior; o primeiro, da chamada, no mínimo o passo seguinte.

- `steps` (`{ Step }`): os passos `{ atraso, M.f, dado? }`, até 1.024
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor, objeto nem superfície, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
- Nós: Esperar sem bloquear (control.wait).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self:schedule({ { 0.5, M.mostrar, 1 }, { 1, M.fechar } })
```

### `obj:cancel(id: number): boolean`

Cancela o que falta de um temporizador do objeto; diz se ainda faltava algo.

Vale para os temporizadores de qualquer script do mesmo objeto.

- `id` (`number`): o número que `after`, `every`, `sequence` ou `schedule` devolveu
- Para com erro: chamado em outro objeto que não `self`; o id não é um número.
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
local id = self:after(5, M.fechar)
self:cancel(id)
```

## Áudio

Sons da Biblioteca, pelo nome que eles têm lá.

### `audio.play(name: string, options: PlayOptions?)`

Toca uma vez o som `name`, pelo nome que ele tem na Biblioteca.

O nome vale também sem diferenciar maiúsculas nem os espaços das pontas. `audience` é aceito desde já, mas só escolhe quem ouve a partir da 0.8, com vários jogadores: hoje o único jogador ouve todo som.

- `name` (`string`): o nome do som na Biblioteca
- `options` (`PlayOptions?`): `{ volume = 0.5, audience = obj }`
- Para com erro: o nome não é um texto; um som que o projeto não tem (o erro lista os sons); dois sons com o mesmo nome; uma opção que não é volume nem audience, ou um volume que não é número; audience que não é um objeto ou não existe mais.
- Nós: Reproduzir WAV (action.sound).
- 0.8: L (local). Desde a 0.7.0.

```lua
audio.play("Bipe", { volume = 0.5 })
```

## Variáveis do jogo

As variáveis do jogo do projeto (Lógica > Variáveis do jogo), pelo nome. Elas guardam o valor quando a cena reinicia.

### `vars.get(name: string, key: string?): any`

Lê uma variável do jogo pelo nome.

O nome vale sem diferenciar maiúsculas. Com `key`, o valor daquela chave; numa chave nunca gravada, o valor inicial da variável, como o nó Ler variável do jogo, e nunca nil. Um Vetor2 lê como vetor com z = 0. As variáveis guardam o valor quando a cena reinicia: zere no `on_start` o que precisa recomeçar.

- `name` (`string`): o nome da variável do jogo
- `key` (`string?`): a chave, numa variável com um valor por chave
- Para com erro: uma variável que o projeto não tem (com o nome mais próximo e a lista); nome ou chave que não é texto.
- Nós: Ler variável do jogo (variable.get).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(vars.get("Pontos"))
```

### `vars.set(name: string, value: VarValue, key: string?)`

Grava uma variável do jogo pelo nome, com um valor do tipo dela.

Com `key`, grava o valor daquela chave, até 1.024 chaves por variável. Uma variável Vetor2 recebe o x e o y de um vetor.

- `name` (`string`): o nome da variável do jogo
- `value` (`VarValue`): um valor do tipo da variável
- `key` (`string?`): a chave, numa variável com um valor por chave
- Para com erro: uma variável que o projeto não tem (com o nome mais próximo e a lista); um valor de outro tipo que o da variável; nome ou chave que não é texto, ou chave de mais de 4.096 bytes; mais de 1.024 chaves na variável.
- Nós: Alterar variável do jogo (variable.set).
- 0.8: do escopo da variável. Desde a 0.7.0.

```lua
vars.set("Pontos", 10)
```

## Diagnóstico

Linhas no Console com o nome do objeto e a linha do script. `error(texto)` e `assert` do Luau param a chamada com o texto, também na linha.

### `print(...: any)`

Escreve uma linha no Console, com o nome do objeto e a linha do script.

Objetos saem pelo nome e tabelas com o primeiro nível. O `oxy_simulate` e o `oxy_console` trazem as linhas com o arquivo e a linha.

- `...` (`any`): os valores, separados por espaço
- Nós: Mensagem de diagnóstico (debug.message).
- 0.8: L (local). Desde a 0.7.0.

```lua
print("vida", self.attr.Vida)
```

### `warn(...: any)`

Escreve um aviso no Console, com o nome do objeto e a linha do script.

Como `print`, com "aviso:" antes do texto.

- `...` (`any`): os valores, separados por espaço
- Nós: Mensagem de diagnóstico (debug.message).
- 0.8: L (local). Desde a 0.7.0.

```lua
warn("sem alvo")
```

## Módulos

Funções e dados que vários scripts usam, em outros arquivos `.luau` da pasta `scripts`: tabelas de cartas, de fases, de inimigos. Um módulo só guarda funções, constantes e dados; o que muda no jogo fica nos atributos e na memória.

### `require(path: string): any`

Carrega um módulo da pasta `scripts` e devolve o que ele devolve.

`require("./dados/cartas")` num script de `scripts/fases/` lê `scripts/fases/dados/cartas.luau`; o caminho nunca sai da pasta `scripts`. O módulo roda na primeira vez que um script o pede, e todos recebem a mesma tabela, travada: ninguém a muda depois. Costuma ficar no topo do script (`local Cartas = require("./dados/cartas")`). `@native/` fica reservado para os módulos em Rust de um jogo, que ainda não existem.

- `path` (`string`): o caminho do arquivo sem `.luau`, a partir da pasta deste script: `./` nela, `../` na de cima
- Para com erro: um caminho que não começa com `./` nem `../`, ou que sai da pasta `scripts`; um arquivo que não existe; um erro no módulo, com o arquivo e a linha dele; um módulo que pede, direta ou indiretamente, o script que o pediu (require circular); um apelido com `@` (`@native/` está reservado).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local dados = require("./dados")
print(dados.custo)
```

## Tipos

```lua
--- Um passo de `schedule`: `{ atraso, Modulo.f, dado? }`.
export type Step = { any }

--- Valor de uma variável do jogo.
export type VarValue = boolean | number | string | vector

--- Um evento do personagem 3D: `kind` é "jump", "land", "leave", "surface", "posture" ou "side", e os outros campos trazem os valores do nó Evento de personagem (`impact`, a velocidade do pouso, entre eles).
export type CharacterEvent = { kind: string, [string]: any }

--- Uma superfície física do projeto, como um atributo Superfície a aponta.
export type Surface = {
    --- O nome da superfície no projeto.
    name: string,
}

--- Um giro (quatérnio), feito pelas funções de `quat`. Nunca muda.
export type Quat = {
    --- O componente x.
    x: number,
    --- O componente y.
    y: number,
    --- O componente z.
    z: number,
    --- O componente w.
    w: number,
}

--- Um objeto da cena. Chega aos scripts como `self` e nos argumentos dos eventos.
export type Object = {
    --- O ID do objeto: fixo no projeto e, numa cópia feita no jogo, derivado do original.
    --- Só leitura. Dois objetos podem ter o mesmo nome, nunca o mesmo ID: serve de chave para guardar algo por objeto. Para mostrar um objeto a quem joga, use `name`.
    id: string,
    --- O nome do objeto na cena.
    --- Só leitura. `print` e `tostring` mostram um objeto por esse nome. A raiz de uma cópia de `obj:clone` leva " (cópia)" no fim do nome, como as do nó Criar cópia de objeto, e o que está nela fica com os nomes do original; as cópias de `scene.spawn_model` ficam com os nomes do modelo.
    name: string,
    --- O pai do objeto na Hierarquia, ou nil num objeto da raiz.
    --- Só leitura. Um script compartilhado por vários objetos alcança o grupo de cada um por aqui, sem uma referência fixa.
    parent: Object?,
    --- Os filhos diretos do objeto, na ordem da cena.
    --- Uma lista nova a cada chamada: guardá-la ou mudá-la não muda a cena.
    children: (self: Object) -> { Object },
    --- O primeiro filho com o nome, na ordem da cena, ou nil.
    --- Para quando a ausência é esperada; um filho fixo de um modelo se pega com `get_child`, que dá um erro claro quando ele falta.
    find_child: (self: Object, name: string, deep: boolean?) -> Object?,
    --- O primeiro filho com o nome, na ordem da cena; um erro se não houver.
    --- Como `find_child`, mas o filho é certo: o erro diz o nome mais parecido e os filhos que existem (um filho renomeado quebra o script na linha, e não depois).
    get_child: (self: Object, name: string, deep: boolean?) -> Object,
    --- Diz se o objeto ainda está na cena.
    --- Um objeto removido continua sendo um valor no script (num atributo, na memória ou numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.
    exists: (self: Object) -> boolean,
    --- Se o objeto aparece: false o esconde, true o mostra.
    --- Como o nó Ativar / desativar componente (Visibilidade). Esconder um grupo esconde o que está nele, mas o `visible` dos filhos continua true; um botão escondido (ou num grupo escondido) não recebe cliques.
    visible: boolean,
    --- Se a lógica de nós (o grafo) do objeto roda; os scripts dele seguem.
    --- Como o nó Ativar / desativar componente (Comportamento visual): false para o grafo e descarta as esperas dele; um erro no grafo também o põe em false. Os scripts têm o próprio liga e desliga.
    logic_enabled: boolean,
    --- Diz se o objeto tem o componente, ligado ou não.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área: um personagem, uma caixa de física.
    has_component: (self: Object, kind: "animation" | "area" | "camera" | "character" | "collider" | "controller" | "platform" | "ui") -> boolean,
    --- Tira o objeto e tudo o que está nele da cena, na hora.
    --- Como o nó Remover objeto. Os temporizadores, as mensagens e a memória dele vão junto. `self:destroy()` encerra a chamada em silêncio no próximo uso de `self`; o que vem antes vale.
    destroy: (self: Object) -> (),
    --- Copia o objeto e tudo o que está nele, como está agora, e devolve a cópia.
    --- Como o nó Criar cópia de objeto: a cópia fica no mesmo pai e no mesmo lugar do original (`offset` a desloca nos eixos do pai; `position` a põe num lugar), com os atributos, os scripts e a lógica dele; a memória dos scripts nasce com os valores declarados, e o `on_start` deles roda depois desta chamada. Os IDs da cópia saem deste objeto, deste script e de quantas cópias ele já fez: a mesma partida dá os mesmos IDs.
    clone: (self: Object, options: SpawnOptions?) -> Object,
    --- true só na primeira vez que `other` é atingido nesta ativação da área do objeto.
    --- A regra "Uma vez por ativação" do nó Aplicar dano, sem o dano: a área conta quem já atingiu até ser desligada e ligada de novo, e os acertos do grafo e dos scripts somam juntos. Em `on_area_enter` e `on_area_exit` da própria área, conta na ativação do evento, como o nó, mesmo que a área tenha desligado depois; fora deles, na ativação de agora, e uma área desligada dá false. O dano vira uma conta: `other.attr.Vida -= 25`.
    first_hit: (self: Object, other: Object) -> boolean,
    --- Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.
    --- A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os scripts da cena, 4.096.
    send: (self: Object, name: string, value: any?) -> (),
    --- A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram.
    --- Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma animação tocando posiciona.
    position: vector,
    --- A posição do pivô do objeto no mundo, em metros.
    --- Num personagem 3D, a origem dele. Escrever move o objeto como `position`.
    world_position: vector,
    --- A rotação do objeto em graus, nos eixos X, Y e Z do pai, como as Propriedades mostram.
    --- Escrever gira o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa. Para girar aos poucos ou combinar giros, use `orientation` e `quat`.
    rotation: vector,
    --- A rotação do objeto como quatérnio, nos eixos do pai.
    --- Escrever gira o objeto como `rotation`, com os ângulos mais perto dos de agora. As funções de `quat` combinam e interpolam giros.
    orientation: Quat,
    --- A escala do objeto em X, Y e Z, relativa ao pai, como as Propriedades mostram.
    --- Escrever muda o tamanho, como o nó Mover / girar / escalar objeto: cada eixo fica acima de 0,00001 em valor absoluto.
    scale: vector,
    --- O giro horizontal do objeto no mundo, em graus: para onde a frente (-Z) aponta.
    --- Só leitura, como o Giro horizontal do nó Ler posição e orientação: 0 olha para -Z e 90 para -X. Um personagem 3D gira com o giro do personagem.
    yaw: number,
    --- Leva o objeto para uma posição do mundo de uma vez, como um salto.
    --- Como escrever `world_position`, mas as plataformas móveis do objeto e do que está nele soltam quem está em cima, sem lançá-lo (o modo Salto do nó Mover / girar / escalar objeto).
    jump_to: (self: Object, position: vector) -> (),
    --- Uma direção nos eixos do objeto, girada para os eixos do mundo.
    --- Só a rotação conta (a escala e a posição não): a frente do objeto no mundo é `self:to_world_direction(vector.create(0, 0, -1))`.
    to_world_direction: (self: Object, direction: vector) -> vector,
    --- Uma direção do mundo, girada para os eixos do objeto.
    --- O contrário de `to_world_direction`: só a rotação conta.
    to_local_direction: (self: Object, direction: vector) -> vector,
    --- Os atributos do objeto, pelo nome.
    --- `obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto lê o objeto, e um Superfície, a superfície (com `.name`), ou nil quando vazios, e recebem nil para ficar vazios. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`. Quando a chamada para num erro com nil, o erro diz o último atributo que ela leu como nil.
    attr: { [string]: any },
    --- Diz se o objeto tem o atributo.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área.
    has_attribute: (self: Object, name: string) -> boolean,
    --- A memória deste script neste objeto, com os campos de `Modulo.state`.
    --- Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto, superfície) é lido e gravado na engine na hora, conferido na linha; um campo de objeto ou de superfície recebe nil para ficar vazio. Uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.
    state: any,
    --- Chama `fn(self, ...)` depois de `seconds` segundos de jogo.
    --- No mínimo no passo seguinte, como o nó Esperar sem bloquear: os dois, começados no mesmo passo com o mesmo tempo, disparam no mesmo passo. A engine guarda o nome da função e os dados, então o temporizador atravessa a recarga do script. Devolve o número que `cancel` recebe.
    after: <X...>(self: Object, seconds: number, fn: (Object, X...) -> (), X...) -> number,
    --- Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.
    --- A primeira chamada sai depois de `seconds`. As regras de `after` valem aqui.
    every: <X...>(self: Object, seconds: number, fn: (Object, X...) -> (), X...) -> number,
    --- Chama `fn(self, item)` com cada item da lista, um a cada `interval` segundos.
    --- O item k sai aos (k-1) × `interval` segundos da chamada, como `self:after` chamaria: o primeiro no passo seguinte, como `self:after(0)`, e os outros na grade das esperas do grafo. O estado que precisa valer já neste passo, grave direto antes; o primeiro item o grava de novo, e um efeito dele (um som) sairia duas vezes.
    sequence: <T>(self: Object, interval: number, fn: (Object, T) -> (), items: { T }) -> number,
    --- Passos com atrasos próprios: `{ {atraso, M.f, dado?}, ... }`.
    --- Cada atraso conta do passo anterior; o primeiro, da chamada, no mínimo o passo seguinte.
    schedule: (self: Object, steps: { Step }) -> number,
    --- Cancela o que falta de um temporizador do objeto; diz se ainda faltava algo.
    --- Vale para os temporizadores de qualquer script do mesmo objeto.
    cancel: (self: Object, id: number) -> boolean,
}

--- Objeto com atributos e memória tipados pelo script: `type Self = ScriptObject<typeof(Modulo.attributes), typeof(Modulo.state)>`. Um nome que o script não declara é acusado.
export type ScriptObject<A, S> = {
    --- O ID do objeto: fixo no projeto e, numa cópia feita no jogo, derivado do original.
    --- Só leitura. Dois objetos podem ter o mesmo nome, nunca o mesmo ID: serve de chave para guardar algo por objeto. Para mostrar um objeto a quem joga, use `name`.
    id: string,
    --- O nome do objeto na cena.
    --- Só leitura. `print` e `tostring` mostram um objeto por esse nome. A raiz de uma cópia de `obj:clone` leva " (cópia)" no fim do nome, como as do nó Criar cópia de objeto, e o que está nela fica com os nomes do original; as cópias de `scene.spawn_model` ficam com os nomes do modelo.
    name: string,
    --- O pai do objeto na Hierarquia, ou nil num objeto da raiz.
    --- Só leitura. Um script compartilhado por vários objetos alcança o grupo de cada um por aqui, sem uma referência fixa.
    parent: Object?,
    --- Os filhos diretos do objeto, na ordem da cena.
    --- Uma lista nova a cada chamada: guardá-la ou mudá-la não muda a cena.
    children: (self: ScriptObject<A, S>) -> { Object },
    --- O primeiro filho com o nome, na ordem da cena, ou nil.
    --- Para quando a ausência é esperada; um filho fixo de um modelo se pega com `get_child`, que dá um erro claro quando ele falta.
    find_child: (self: ScriptObject<A, S>, name: string, deep: boolean?) -> Object?,
    --- O primeiro filho com o nome, na ordem da cena; um erro se não houver.
    --- Como `find_child`, mas o filho é certo: o erro diz o nome mais parecido e os filhos que existem (um filho renomeado quebra o script na linha, e não depois).
    get_child: (self: ScriptObject<A, S>, name: string, deep: boolean?) -> Object,
    --- Diz se o objeto ainda está na cena.
    --- Um objeto removido continua sendo um valor no script (num atributo, na memória ou numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.
    exists: (self: ScriptObject<A, S>) -> boolean,
    --- Se o objeto aparece: false o esconde, true o mostra.
    --- Como o nó Ativar / desativar componente (Visibilidade). Esconder um grupo esconde o que está nele, mas o `visible` dos filhos continua true; um botão escondido (ou num grupo escondido) não recebe cliques.
    visible: boolean,
    --- Se a lógica de nós (o grafo) do objeto roda; os scripts dele seguem.
    --- Como o nó Ativar / desativar componente (Comportamento visual): false para o grafo e descarta as esperas dele; um erro no grafo também o põe em false. Os scripts têm o próprio liga e desliga.
    logic_enabled: boolean,
    --- Diz se o objeto tem o componente, ligado ou não.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área: um personagem, uma caixa de física.
    has_component: (self: ScriptObject<A, S>, kind: "animation" | "area" | "camera" | "character" | "collider" | "controller" | "platform" | "ui") -> boolean,
    --- Tira o objeto e tudo o que está nele da cena, na hora.
    --- Como o nó Remover objeto. Os temporizadores, as mensagens e a memória dele vão junto. `self:destroy()` encerra a chamada em silêncio no próximo uso de `self`; o que vem antes vale.
    destroy: (self: ScriptObject<A, S>) -> (),
    --- Copia o objeto e tudo o que está nele, como está agora, e devolve a cópia.
    --- Como o nó Criar cópia de objeto: a cópia fica no mesmo pai e no mesmo lugar do original (`offset` a desloca nos eixos do pai; `position` a põe num lugar), com os atributos, os scripts e a lógica dele; a memória dos scripts nasce com os valores declarados, e o `on_start` deles roda depois desta chamada. Os IDs da cópia saem deste objeto, deste script e de quantas cópias ele já fez: a mesma partida dá os mesmos IDs.
    clone: (self: ScriptObject<A, S>, options: SpawnOptions?) -> Object,
    --- true só na primeira vez que `other` é atingido nesta ativação da área do objeto.
    --- A regra "Uma vez por ativação" do nó Aplicar dano, sem o dano: a área conta quem já atingiu até ser desligada e ligada de novo, e os acertos do grafo e dos scripts somam juntos. Em `on_area_enter` e `on_area_exit` da própria área, conta na ativação do evento, como o nó, mesmo que a área tenha desligado depois; fora deles, na ativação de agora, e uma área desligada dá false. O dano vira uma conta: `other.attr.Vida -= 25`.
    first_hit: (self: ScriptObject<A, S>, other: Object) -> boolean,
    --- Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.
    --- A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os scripts da cena, 4.096.
    send: (self: ScriptObject<A, S>, name: string, value: any?) -> (),
    --- A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram.
    --- Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma animação tocando posiciona.
    position: vector,
    --- A posição do pivô do objeto no mundo, em metros.
    --- Num personagem 3D, a origem dele. Escrever move o objeto como `position`.
    world_position: vector,
    --- A rotação do objeto em graus, nos eixos X, Y e Z do pai, como as Propriedades mostram.
    --- Escrever gira o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa. Para girar aos poucos ou combinar giros, use `orientation` e `quat`.
    rotation: vector,
    --- A rotação do objeto como quatérnio, nos eixos do pai.
    --- Escrever gira o objeto como `rotation`, com os ângulos mais perto dos de agora. As funções de `quat` combinam e interpolam giros.
    orientation: Quat,
    --- A escala do objeto em X, Y e Z, relativa ao pai, como as Propriedades mostram.
    --- Escrever muda o tamanho, como o nó Mover / girar / escalar objeto: cada eixo fica acima de 0,00001 em valor absoluto.
    scale: vector,
    --- O giro horizontal do objeto no mundo, em graus: para onde a frente (-Z) aponta.
    --- Só leitura, como o Giro horizontal do nó Ler posição e orientação: 0 olha para -Z e 90 para -X. Um personagem 3D gira com o giro do personagem.
    yaw: number,
    --- Leva o objeto para uma posição do mundo de uma vez, como um salto.
    --- Como escrever `world_position`, mas as plataformas móveis do objeto e do que está nele soltam quem está em cima, sem lançá-lo (o modo Salto do nó Mover / girar / escalar objeto).
    jump_to: (self: ScriptObject<A, S>, position: vector) -> (),
    --- Uma direção nos eixos do objeto, girada para os eixos do mundo.
    --- Só a rotação conta (a escala e a posição não): a frente do objeto no mundo é `self:to_world_direction(vector.create(0, 0, -1))`.
    to_world_direction: (self: ScriptObject<A, S>, direction: vector) -> vector,
    --- Uma direção do mundo, girada para os eixos do objeto.
    --- O contrário de `to_world_direction`: só a rotação conta.
    to_local_direction: (self: ScriptObject<A, S>, direction: vector) -> vector,
    --- Os atributos do objeto, pelo nome.
    --- `obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto lê o objeto, e um Superfície, a superfície (com `.name`), ou nil quando vazios, e recebem nil para ficar vazios. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`. Quando a chamada para num erro com nil, o erro diz o último atributo que ela leu como nil.
    attr: A & {},
    --- Diz se o objeto tem o atributo.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área.
    has_attribute: (self: ScriptObject<A, S>, name: string) -> boolean,
    --- A memória deste script neste objeto, com os campos de `Modulo.state`.
    --- Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto, superfície) é lido e gravado na engine na hora, conferido na linha; um campo de objeto ou de superfície recebe nil para ficar vazio. Uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.
    state: S & {},
    --- Chama `fn(self, ...)` depois de `seconds` segundos de jogo.
    --- No mínimo no passo seguinte, como o nó Esperar sem bloquear: os dois, começados no mesmo passo com o mesmo tempo, disparam no mesmo passo. A engine guarda o nome da função e os dados, então o temporizador atravessa a recarga do script. Devolve o número que `cancel` recebe.
    after: <X...>(self: ScriptObject<A, S>, seconds: number, fn: (ScriptObject<A, S>, X...) -> (), X...) -> number,
    --- Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.
    --- A primeira chamada sai depois de `seconds`. As regras de `after` valem aqui.
    every: <X...>(self: ScriptObject<A, S>, seconds: number, fn: (ScriptObject<A, S>, X...) -> (), X...) -> number,
    --- Chama `fn(self, item)` com cada item da lista, um a cada `interval` segundos.
    --- O item k sai aos (k-1) × `interval` segundos da chamada, como `self:after` chamaria: o primeiro no passo seguinte, como `self:after(0)`, e os outros na grade das esperas do grafo. O estado que precisa valer já neste passo, grave direto antes; o primeiro item o grava de novo, e um efeito dele (um som) sairia duas vezes.
    sequence: <T>(self: ScriptObject<A, S>, interval: number, fn: (ScriptObject<A, S>, T) -> (), items: { T }) -> number,
    --- Passos com atrasos próprios: `{ {atraso, M.f, dado?}, ... }`.
    --- Cada atraso conta do passo anterior; o primeiro, da chamada, no mínimo o passo seguinte.
    schedule: (self: ScriptObject<A, S>, steps: { Step }) -> number,
    --- Cancela o que falta de um temporizador do objeto; diz se ainda faltava algo.
    --- Vale para os temporizadores de qualquer script do mesmo objeto.
    cancel: (self: ScriptObject<A, S>, id: number) -> boolean,
}

--- Qualquer objeto, tipado pelo script ou não.
export type AnyObject = Object | ScriptObject<any, any>

--- Opções de `audio.play`.
export type PlayOptions = {
    --- Volume de 0 a 1 (padrão 1).
    volume: number?,
    --- O objeto cujo jogador ouve o som, com vários jogadores, a partir da 0.8. Hoje não muda nada: o único jogador ouve todo som; para tocar só para ele, confira um atributo do personagem (como `Jogador local`).
    audience: AnyObject?,
}

--- Opções de `obj:clone` e `scene.spawn_model`.
export type SpawnOptions = {
    --- Só em `clone`: quanto a cópia fica longe do original, nos eixos do pai (sem ele, no mesmo lugar).
    offset: vector?,
    --- Onde a cópia fica, nos eixos do pai, como `obj.position`.
    position: vector?,
    --- A rotação da cópia em graus, como `obj.rotation`.
    rotation: vector?,
    --- O pai da cópia; sem ele, o do original (`clone`) ou a raiz da cena.
    parent: Object?,
    --- Valores de atributos que a cópia já tem, antes do `on_start` dela.
    attributes: { [string]: any }?,
}
```
