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

## Personagem 3D

O Personagem 3D de um objeto, por `obj.character`: o estado dele (onde pisa, a velocidade, a mira) e os comandos, os mesmos dos nós de personagem. Os comandos de movimento valem no próximo passo de movimento: renove `move` a cada passo (`on_step`). Num objeto sem Personagem 3D, `obj.character` é um erro.

### `obj.character: Character`

O Personagem 3D do objeto: o estado e os comandos dele.

Só leitura, num objeto com Personagem 3D; num objeto sem ele, é um erro (confira antes com `obj:has_component("character")`). O personagem anda pelos comandos (`move`, `jump`, `teleport`…), não pela `position` do objeto.

- Para com erro: o objeto não tem Personagem 3D; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.grounded)
```

### `obj.character.position: vector`

A posição dos pés do personagem no mundo, em metros.

Só leitura, como a Posição de Ler personagem 3D: a origem do corpo, nos pés. Para mover o personagem, use `move`, `set_velocity` ou `teleport`.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.position)
```

### `obj.character.velocity: vector`

A velocidade total do personagem, em m/s, com a do apoio.

Só leitura, como a Velocidade total de Ler personagem 3D: sobre uma plataforma que anda, inclui a velocidade dela.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.velocity)
```

### `obj.character.own_velocity: vector`

A velocidade própria do personagem, em m/s: o embalo, sem a do apoio.

Só leitura, como a Velocidade própria de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.own_velocity)
```

### `obj.character.horizontal_velocity: vector`

A velocidade total do personagem no plano, em m/s: a mesma com y = 0.

Só leitura, como a Velocidade horizontal de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.horizontal_velocity)
```

### `obj.character.speed: number`

A rapidez horizontal do personagem, em m/s.

Só leitura, como a Rapidez horizontal de Ler personagem 3D: o comprimento de `horizontal_velocity`.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.attr.Aviso = string.format("%.1f m/s", self.character.speed)
```

### `obj.character.grounded: boolean`

Se o personagem está no chão, num apoio em que pode ficar em pé.

Só leitura, como No chão de Ler personagem 3D; uma parede não é apoio.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self.character.grounded then
    print("no chão")
end
```

### `obj.character.rising: boolean`

Se o personagem está no ar, subindo.

Só leitura, como Subindo de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.rising)
```

### `obj.character.falling: boolean`

Se o personagem está no ar, caindo.

Só leitura, como Caindo de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.falling)
```

### `obj.character.posture: "standing" | "crouched" | "sliding"`

A postura do personagem: "standing" (em pé), "crouched" (agachado) ou "sliding" (deslizando).

Só leitura; muda com `set_posture`. A Postura de Ler personagem 3D é a mesma, em português.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self.character.posture == "crouched" then
    print("agachado")
end
```

### `obj.character.support: Object?`

O objeto em que o personagem pisa, ou nil no ar.

Só leitura, como Apoio de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local chao = self.character.support
if chao then
    print(chao.name)
end
```

### `obj.character.surface: Surface?`

A superfície física em que o personagem pisa, ou nil no ar ou na padrão.

Só leitura, como Superfície de Ler personagem 3D: a do apoio, e não a do corpo do personagem (`body_surface`).

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local piso = self.character.surface
if piso then
    print(piso.name)
end
```

### `obj.character.body_surface: Surface?`

A superfície física do corpo do personagem, ou nil para a padrão. Leitura e escrita.

Como Ler superfície do colisor e Aplicar superfície ao colisor num Personagem 3D: a do corpo, e não a do chão em que ele pisa (`surface`). Recebe uma superfície (de um atributo Superfície, de um colisor) ou nil; o nome de uma superfície do projeto também serve, mas os tipos do VS Code só conhecem a superfície. Vale só durante a partida.

- Para com erro: uma superfície que o projeto não tem; escrever outra coisa que uma superfície, um nome ou nil; o objeto não existe mais.
- Nós: Ler superfície do colisor (surface.get), Aplicar superfície ao colisor (surface.apply).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
local antes = self.character.body_surface
self.character.body_surface = nil
self.character.body_surface = antes
```

### `obj.character.support_point: vector`

O ponto de apoio sob os pés, no mundo, em metros; zero no ar.

Só leitura, como Ponto de apoio de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.support_point)
```

### `obj.character.support_normal: vector`

Para onde o chão sob os pés aponta (a normal do apoio); zero no ar.

Só leitura, como Normal de apoio de Ler personagem 3D: num chão plano, (0, 1, 0).

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.support_normal)
```

### `obj.character.support_velocity: vector`

A velocidade do apoio, em m/s, como a de uma plataforma que anda; zero no ar.

Só leitura, como Velocidade do apoio de Ler personagem 3D.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.support_velocity)
```

### `obj.character.movement_blocked: boolean`

Se algum motivo bloqueia o movimento do personagem.

Só leitura; os motivos vêm de `block`, e `block_reasons("movement")` os lista.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.movement_blocked)
```

### `obj.character.look_blocked: boolean`

Se algum motivo bloqueia o olhar do personagem.

Só leitura; os motivos vêm de `block`, e `block_reasons("look")` os lista.

- Para com erro: o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.look_blocked)
```

### `obj.character.aim: vector`

A direção da mira do personagem no mundo, de comprimento 1, vinda da entrada do jogo.

Só leitura: o giro e a inclinação do olhar que a entrada deu ao personagem (o mouse, pela câmera que o acompanha), e não a imagem da câmera, que pode estar suavizada ou em transição. Para mirar e atirar, dá o mesmo em toda máquina.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local alvo = physics.raycast(self.character.position + vector.create(0, 1.6, 0), self.character.aim, 50, { exclude = self })
if alvo then
    print(alvo.object.name)
end
```

### `obj.character.look: vector`

O olhar do personagem em graus: x é o giro (0 olha para -Z, 90 para -X) e y a inclinação (positiva para cima).

Só leitura; z é 0. É o que `teleport` recebe em `look`: guarde-o com a posição para o jogador voltar a um ponto olhando para o mesmo lado.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character.look)
```

### `obj.character:block_reasons(channel: "movement" | "look"): { string }`

Os motivos que bloqueiam o movimento ou o olhar do personagem, em ordem.

Uma lista nova a cada chamada, vazia sem bloqueio; os motivos vêm de `block`. Ler personagem 3D dá os mesmos, juntos num texto.

- `channel` (`"movement" | "look"`): "movement" (o movimento) ou "look" (o olhar)
- Para com erro: um canal que não é movement nem look; o objeto não existe mais.
- Nós: Ler personagem 3D (character.state).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
for _, motivo in self.character:block_reasons("movement") do
    print(motivo)
end
```

### `obj.character:move(axis: vector)`

Pede ao personagem que ande no próximo passo: x para a direita, y para a frente.

Como Definir intenção para este passo: vale só no próximo passo de movimento, então renove a cada passo (`on_step`); um vetor mais longo que 1 é encurtado. A direção segue a referência do personagem (eixos da cena, do corpo ou da câmera). O pedido vence as teclas naquele passo; `input.movement()` dá o eixo das teclas, e com as ações automáticas desligadas (`set_option`) o personagem só anda assim.

- `axis` (`vector`): x para a direita e y para a frente, de comprimento até 1 (z não conta)
- Para com erro: axis que não é um vetor finito; o objeto não existe mais.
- Nós: Definir intenção para este passo (character.intent).
- 0.8: M (partida). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    self.character:move(input.movement())
end
```

### `obj.character:jump()`

Pede um pulo ao personagem, como a tecla de pular.

Como Pedir pulo: pula no chão ou logo depois da borda; no ar, o pedido espera pela antecipação do pulo. Vários pedidos no mesmo passo são um pulo só; a altura vem da velocidade de pulo (`get_param("jump_speed")`).

- Para com erro: o objeto não existe mais.
- Nós: Pedir pulo (character.jump).
- 0.8: M (partida). Desde a 0.7.0.

```lua
if self.character.grounded then
    self.character:jump()
end
```

### `obj.character:set_posture(kind: "sprint" | "crouch" | "slide", on: boolean)`

Liga ou desliga correr, agachar ou deslizar.

Como Correr / agachar / deslizar, e vale até mudar de novo. Ficar em pé pede espaço acima; deslizar pede o deslize permitido no personagem e embalo, e desligá-lo levanta o personagem se couber.

- `kind` (`"sprint" | "crouch" | "slide"`): "sprint" (correr), "crouch" (agachar) ou "slide" (deslizar)
- `on` (`boolean`): true liga, false desliga
- Para com erro: um comando que não é sprint, crouch nem slide; deslizar num personagem com o deslize desligado; o objeto não existe mais.
- Nós: Correr / agachar / deslizar (character.posture).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:set_posture("crouch", true)
```

### `obj.character:add_velocity(velocity: vector)`

Soma uma velocidade em m/s, no mundo, ao embalo do personagem.

Como o modo Adicionar de Adicionar / definir velocidade: preserva o embalo, e para cima tira o personagem do chão. Não é força: `vector.create(0, 8, 0)` é um impulso de 8 m/s para cima.

- `velocity` (`vector`): a velocidade somada, no mundo, em m/s
- Para com erro: velocity que não é um vetor finito; o objeto não existe mais.
- Nós: Adicionar / definir velocidade (m/s) (character.velocity).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:add_velocity(vector.create(0, 8, 0))
```

### `obj.character:set_velocity(velocity: vector)`

Troca a velocidade total do personagem por esta, em m/s, no mundo.

Como o modo Definir de Adicionar / definir velocidade: o embalo vira o que falta para a velocidade total, contando a do apoio.

- `velocity` (`vector`): a nova velocidade total, no mundo, em m/s
- Para com erro: velocity que não é um vetor finito; o objeto não existe mais.
- Nós: Adicionar / definir velocidade (m/s) (character.velocity).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:set_velocity(vector.zero)
```

### `obj.character:block(channel: "movement" | "look", reason: string, on: boolean)`

Bloqueia ou libera o movimento ou o olhar do personagem por um motivo.

Como Bloquear / liberar entrada por motivo: os motivos se somam, e liberar um não libera os outros. Não desliga a gravidade, a colisão nem a câmera.

- `channel` (`"movement" | "look"`): "movement" (o movimento) ou "look" (o olhar)
- `reason` (`string`): o motivo, de 1 a 256 bytes
- `on` (`boolean`): true bloqueia, false libera o motivo
- Para com erro: um canal que não é movement nem look; um motivo vazio ou de mais de 256 bytes; o objeto não existe mais.
- Nós: Bloquear / liberar entrada por motivo (character.block).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:block("movement", "diálogo", true)
```

### `obj.character:apply_profile(profile: "direct" | "agile" | "chained", allow_limit: boolean?)`

Aplica ao personagem um perfil de movimento pronto.

Como Aplicar perfil de movimento, com os valores do botão do editor; o nome antigo `"parkour"` vale como `"agile"`. Não teleporta nem zera a velocidade; um limite horizontal menor que o embalo de agora é recusado sem `allow_limit`. Vale só durante a partida.

- `profile` (`"direct" | "agile" | "chained"`): "direct" (Direto), "agile" (Ágil com embalo) ou "chained" (Saltos encadeados)
- `allow_limit` (`boolean?`): true deixa um limite horizontal novo reduzir o embalo de agora
- Para com erro: um perfil que não é direct, agile nem chained; um limite horizontal que reduziria o embalo, sem allow_limit; o objeto não existe mais.
- Nós: Aplicar perfil de movimento (character.profile).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:apply_profile("agile")
```

### `obj.character:set_param(name: "speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed", value: number, allow_limit: boolean?)`

Muda um valor do motor do personagem durante a partida.

Como Alterar parâmetro do personagem: velocidades em m/s (`speed`, `sprint_speed`, `crouch_speed`, `jump_speed`, `air_projected_limit`, `horizontal_limit`), acelerações em m/s² (`gravity`, `ground_acceleration`, `ground_braking`, `air_acceleration`, `slide_control`), atritos e resistência por segundo, conservações de 0 a 1, tolerâncias em milissegundos (`coyote_ms`, `jump_buffer_ms`), o deslize em segundos e o giro em graus por segundo. Um valor que o personagem não aceita é um erro e nada muda. Vale só durante a partida.

- `name` (`"speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed"`): o parâmetro, pelo nome que Alterar parâmetro do personagem dá
- `value` (`number`): o valor, na unidade do parâmetro
- `allow_limit` (`boolean?`): true deixa um limite horizontal novo reduzir o embalo de agora
- Para com erro: um parâmetro que o personagem não tem (o erro lista os nomes); um valor que o personagem não aceita; um limite horizontal que reduziria o embalo, sem allow_limit; o objeto não existe mais.
- Nós: Alterar parâmetro do personagem (character.parameter).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:set_param("speed", 3)
```

### `obj.character:get_param(name: "speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed"): number`

Lê um valor do motor do personagem, como está na partida.

Os nomes e as unidades de `set_param`; dá o valor de agora, com o que `set_param` e `apply_profile` mudaram.

- `name` (`"speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed"`): o parâmetro, como `set_param` o nomeia
- Para com erro: um parâmetro que o personagem não tem (o erro lista os nomes); o objeto não existe mais.
- Nós: Alterar parâmetro do personagem (character.parameter).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.character:get_param("jump_speed"))
```

### `obj.character:set_option(name: "automatic_input" | "slide_enabled" | "crouch_toggle" | "automatic_jump" | "face_look", on: boolean)`

Liga ou desliga uma opção do personagem durante a partida.

Como Alterar opção do personagem: `automatic_input` (as teclas movem o personagem; desligada, só `move`, `jump` e `set_posture`), `slide_enabled` (deslize permitido), `crouch_toggle` (a tecla de agachar alterna), `automatic_jump` (pular ao segurar) e `face_look` (o corpo vira para o olhar). Vale só durante a partida.

- `name` (`"automatic_input" | "slide_enabled" | "crouch_toggle" | "automatic_jump" | "face_look"`): a opção
- `on` (`boolean`): true liga, false desliga
- Para com erro: uma opção que o personagem não tem (o erro lista os nomes); o objeto não existe mais.
- Nós: Alterar opção do personagem (character.option).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:set_option("automatic_input", false)
```

### `obj.character:set_yaw(degrees: number)`

Gira o corpo do personagem para um giro horizontal do mundo, em graus.

Como Definir giro horizontal do personagem: 0 olha para -Z e 90 para -X, como `obj.yaw` lê. Se o corpo girado ocupar um obstáculo, nada muda e é um erro; em primeira pessoa, a câmera que o segue gira junto.

- `degrees` (`number`): o giro no mundo, em graus
- Para com erro: degrees que não é um número finito; o corpo girado ocuparia um obstáculo; o objeto não existe mais.
- Nós: Definir giro horizontal do personagem (character.yaw).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.character:set_yaw(90)
```

### `obj.character:teleport(feet: vector, options: TeleportOptions?): (boolean, string?, vector?)`

Põe os pés do personagem num ponto do mundo, se houver espaço; devolve ok, o motivo da recusa e o destino usado.

Como Teleportar com segurança. Opções: `search`, até quantos metros procurar um lugar livre perto (0 a 2; sem ela, um destino ocupado é recusado); `keep_velocity` (sem ela, a velocidade zera); `yaw`, o giro do corpo em graus, e `look`, o olhar como `character.look` o dá (sem eles, ficam como estão). Uma recusa não é erro: `ok` é false, o segundo valor diz por quê e o personagem fica onde estava. Os eventos do trajeto abandonado que ainda não rodaram caem, como no nó.

- `feet` (`vector`): onde os pés ficam, no mundo, em metros
- `options` (`TeleportOptions?`): `{ search = 1, keep_velocity = true, yaw = 90, look = character.look }`
- Para com erro: feet que não é um vetor; uma opção desconhecida ou de outro tipo; o objeto não existe mais.
- Nós: Teleportar com segurança (character.teleport).
- 0.8: M (partida). Desde a 0.7.0.

```lua
local ok, motivo = self.character:teleport(vector.create(0, 2, 0), { search = 1 })
if not ok then
    warn(motivo)
end
```

## Física

Os colisores, as plataformas móveis e o Movimento 2D de um objeto, que se ligam e desligam, e as consultas à física 3D (raios e formas varridas), que não movem nada. Uma consulta sem contato devolve nil; uma recusada (cena 2D, mais de 256 consultas no passo) devolve nil e o motivo.

### `obj.collider: Collider`

O colisor do objeto: 2D, 3D ou área.

Só leitura, num objeto com colisor ou área; num objeto sem eles, é um erro (confira antes com `obj:has_component("collider")`). O corpo de um Personagem 3D não é um colisor: use `obj.character` (a superfície do corpo é `obj.character.body_surface`).

- Para com erro: o objeto não tem colisor nem área; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.collider.enabled)
```

### `obj.collider.enabled: boolean`

Se o colisor (ou a área) está ligado. Leitura e escrita.

Como Ativar / desativar componente (Colisor): desligado, não colide nem sente quem entra. Religar uma área começa uma ativação nova: quem está dentro entra de novo (`on_area_enter`), e `first_hit` recomeça.

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.collider.enabled = false
```

### `obj.collider.is_area: boolean`

Se o colisor é uma área (sensor), que sente quem entra sem colidir.

Só leitura, como as Propriedades marcam.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.collider.is_area)
```

### `obj.collider.surface: Surface?`

A superfície física do colisor 3D, ou nil para a padrão. Leitura e escrita.

Como Ler superfície do colisor e Aplicar superfície ao colisor: recebe uma superfície (de um atributo Superfície, de outro colisor) ou nil; o nome de uma superfície do projeto também serve, mas os tipos do VS Code só conhecem a superfície. Só um colisor 3D tem superfície; a do corpo de um personagem é `obj.character.body_surface`. Vale só durante a partida.

- Para com erro: um colisor 2D, que não tem superfície; uma superfície que o projeto não tem; escrever outra coisa que uma superfície, um nome ou nil; o objeto não existe mais.
- Nós: Ler superfície do colisor (surface.get), Aplicar superfície ao colisor (surface.apply).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
local antes = self.collider.surface
self.collider.surface = nil
self.collider.surface = antes
```

### `obj.platform: Platform`

A plataforma móvel do objeto.

Só leitura, num objeto com plataforma móvel; num objeto sem ela, é um erro (confira antes com `obj:has_component("platform")`).

- Para com erro: o objeto não tem plataforma móvel; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.platform.enabled)
```

### `obj.platform.enabled: boolean`

Se a plataforma móvel leva quem está em cima. Leitura e escrita.

Como Ativar / desativar componente (Plataforma móvel).

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.platform.enabled = false
```

### `obj.controller: Controller`

O Movimento 2D do objeto.

Só leitura, num objeto com Movimento 2D; num objeto sem ele, é um erro (confira antes com `obj:has_component("controller")`).

- Para com erro: o objeto não tem Movimento 2D; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.controller.enabled)
```

### `obj.controller.enabled: boolean`

Se o Movimento 2D do objeto obedece às teclas. Leitura e escrita.

Como Ativar / desativar componente (Controlador).

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.controller.enabled = false
```

### `physics.raycast(origin: vector, direction: vector, distance: number, options: QueryOptions?): (Hit?, string?)`

Lança um raio e devolve o primeiro contato, ou nil.

Como Consultar raio. Sem contato, devolve nil; uma consulta recusada (cena 2D, mais de 256 consultas no passo, direção zero) devolve nil e o motivo. Um raio que sai de dentro de um corpo o atinge na distância 0: ponha o objeto em `exclude`. Não move nada.

- `origin` (`vector`): de onde o raio sai, no mundo, em metros
- `direction` (`vector`): para onde ele vai, no mundo (qualquer comprimento)
- `distance` (`number`): até onde ele vai, em metros
- `options` (`QueryOptions?`): `{ exclude = obj, areas = true, mask = { "Personagens" } }`
- Para com erro: origin ou direction que não é um vetor, distance que não é um número de 0 ou mais; uma opção desconhecida ou de outro tipo, um grupo de colisão que não existe.
- Nós: Consultar raio (query.ray).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local hit = physics.raycast(self.world_position, vector.create(0, -1, 0), 10, { exclude = self })
if hit then
    print(hit.object.name, hit.distance)
end
```

### `physics.sphere_cast(origin: vector, radius: number, direction: vector, distance: number, options: QueryOptions?): (Hit?, string?)`

Varre uma esfera e devolve o primeiro contato, ou nil.

Como Varrer esfera, com as regras de `raycast`: uma esfera que já toca algo na partida o atinge na distância 0.

- `origin` (`vector`): o centro da esfera na partida, no mundo
- `radius` (`number`): o raio da esfera, em metros
- `direction` (`vector`): para onde ela vai, no mundo
- `distance` (`number`): até onde ela vai, em metros
- `options` (`QueryOptions?`): as opções de `raycast`
- Para com erro: um argumento de outro tipo, uma opção desconhecida, um grupo de colisão que não existe.
- Nós: Varrer esfera (query.sphere).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local hit = physics.sphere_cast(self.world_position, 0.5, vector.create(0, 0, -1), 5, { exclude = self })
print(hit ~= nil)
```

### `physics.capsule_cast(origin: vector, radius: number, height: number, direction: vector, distance: number, options: QueryOptions?): (Hit?, string?)`

Varre uma cápsula em pé e devolve o primeiro contato, ou nil.

Como Varrer cápsula, com as regras de `raycast`: serve para saber se um corpo do tamanho de um personagem passa por um vão.

- `origin` (`vector`): o centro da cápsula na partida, no mundo
- `radius` (`number`): o raio da cápsula, em metros
- `height` (`number`): a altura total da cápsula em pé, em metros
- `direction` (`vector`): para onde ela vai, no mundo
- `distance` (`number`): até onde ela vai, em metros
- `options` (`QueryOptions?`): as opções de `raycast`
- Para com erro: um argumento de outro tipo, uma opção desconhecida, um grupo de colisão que não existe.
- Nós: Varrer cápsula (query.capsule).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local hit = physics.capsule_cast(self.world_position + vector.create(0, 1, 0), 0.3, 1.8, vector.create(0, 0, -1), 2, { exclude = self })
print(hit == nil)
```

### `physics.capsule_fits(position: vector, radius: number, height: number, options: QueryOptions?): (boolean, Hit?, string?)`

Diz se uma cápsula em pé cabe num lugar, sem mover nada.

Como Testar espaço livre para cápsula: true quando ela não toca nada; senão, false e o contato (a distância é a penetração, negativa). Uma consulta recusada dá false, nil e o motivo.

- `position` (`vector`): o centro da cápsula, no mundo
- `radius` (`number`): o raio da cápsula, em metros
- `height` (`number`): a altura total da cápsula em pé, em metros
- `options` (`QueryOptions?`): as opções de `raycast`
- Para com erro: um argumento de outro tipo, uma opção desconhecida, um grupo de colisão que não existe.
- Nós: Testar espaço livre para cápsula (query.space).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local cabe = physics.capsule_fits(self.world_position + vector.create(0, 3, 0), 0.3, 1.8, { exclude = self })
print(cabe)
```

## Animação

Os clipes de animação de um objeto, por `obj.animation`, pelos nomes que eles têm no Estúdio. Os marcadores dos clipes chegam a `on_marker`.

### `obj.animation: Animation`

As animações (clipes) do objeto.

Só leitura, num objeto com clipes; num objeto sem eles, é um erro (confira antes com `obj:has_component("animation")`).

- Para com erro: o objeto não tem animações; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.animation.clip)
```

### `obj.animation:play(clip: string, restart: boolean?)`

Toca um clipe do objeto pelo nome.

Como Reproduzir animação: o nome vale sem diferenciar maiúsculas. Os marcadores do clipe chegam a `on_marker`; um objeto que um clipe tocando posiciona não aceita `position` até ele parar.

- `clip` (`string`): o nome do clipe, como no Estúdio
- `restart` (`boolean?`): false deixa um clipe que já toca seguir de onde está (padrão: recomeça)
- Para com erro: um clipe que o objeto não tem (o erro lista os clipes); o objeto não existe mais.
- Nós: Reproduzir animação (action.animation).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.animation:play("Abrir")
```

### `obj.animation.playing: boolean`

Se o clipe do objeto está tocando: false pausa onde está, true retoma. Leitura e escrita.

Como Ativar / desativar componente (Animação). Antes de algum clipe tocar, é false e true não faz nada: comece com `play`.

- Para com erro: escrever outra coisa que true ou false; o objeto não existe mais.
- Nós: Ativar / desativar componente (action.component).
- 0.8: T (do objeto alvo). Desde a 0.7.0.

```lua
self.animation.playing = false
```

### `obj.animation.clip: string?`

O nome do clipe que toca (ou que parou), ou nil se nenhum tocou.

Só leitura; um clipe que acabou ou foi pausado continua sendo o clipe, com `playing` false.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self.animation.clip == "Abrir" then
    print("abrindo")
end
```

## Câmera

A câmera de jogo: qual está ativa, para onde olha, e os comandos dos nós de câmera. As funções recebem o objeto câmera; os modos e o olhar assistido pedem uma câmera com Controle de câmera.

### `camera.active(): Object?`

A câmera de jogo ativa, ou nil.

A que `camera.activate` escolheu, ou a primeira marcada como ativa na ordem da cena.

- Nós: Ler câmera de jogo (camera.read).
- 0.8: L (local). Desde a 0.7.0.

```lua
local ativa = camera.active()
if ativa then
    print(ativa.name)
end
```

### `camera.look(): vector`

O olhar da câmera ativa em graus: x é o giro (0 olha para -Z) e y a inclinação.

O da simulação, sem a suavização da imagem; z é 0. Numa câmera Fixa, vem da rotação do objeto câmera.

- Para com erro: nenhuma câmera ativa, ou o Controle de câmera dela ainda não começou.
- Nós: Ler câmera de jogo (camera.read).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    self.attr.Aviso = tostring(camera.look())
end
```

### `camera.forward(): vector`

A direção do olhar da câmera ativa no mundo, de comprimento 1.

A da simulação, como `camera.look`. Para mirar pelo personagem, que vale igual em toda máquina, use `obj.character.aim`.

- Para com erro: nenhuma câmera ativa, ou o Controle de câmera dela ainda não começou.
- Nós: Ler câmera de jogo (camera.read).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    self.attr.Aviso = tostring(camera.forward())
end
```

### `camera.mode(): string?`

O modo da câmera ativa: "first_person", "third_person" ou "fixed"; nil sem câmera ativa.

Uma câmera sem Controle de câmera é sempre "fixed".

- Nós: Ler câmera de jogo (camera.read).
- 0.8: L (local). Desde a 0.7.0.

```lua
if camera.mode() == "first_person" then
    print("primeira pessoa")
end
```

### `camera.activate(camera: AnyObject, seconds: number?)`

Torna uma câmera a câmera de jogo ativa, com uma transição.

Como Ativar câmera de jogo: a câmera mantém o modo e quem acompanha.

- `camera` (`AnyObject`): o objeto com Câmera
- `seconds` (`number?`): a transição, em segundos (padrão 0,25; 0 corta seco)
- Para com erro: um objeto sem Câmera; seconds negativo ou que não é um número.
- Nós: Ativar câmera de jogo (camera.activate).
- 0.8: L (local). Desde a 0.7.0.

```lua
camera.activate(self, 0)
```

### `camera.set_mode(camera: AnyObject, mode: "first_person" | "third_person" | "fixed" | "fixed_object", seconds: number?)`

Troca o modo de uma câmera com Controle de câmera.

Como Trocar modo da câmera: `"fixed"` para a câmera onde a vista está (mover o objeto câmera depois move a vista), e `"fixed_object"` leva a vista ao objeto câmera como ele está.

- `camera` (`AnyObject`): o objeto com Controle de câmera
- `mode` (`"first_person" | "third_person" | "fixed" | "fixed_object"`): "first_person", "third_person", "fixed" (parada na vista de agora) ou "fixed_object" (parada no objeto câmera)
- `seconds` (`number?`): a transição, em segundos (padrão 0,25)
- Para com erro: um objeto sem Controle de câmera; um modo desconhecido; seconds negativo ou que não é um número.
- Nós: Trocar modo da câmera (camera.mode).
- 0.8: L (local). Desde a 0.7.0.

```lua
camera.set_mode(self, "third_person", 0.5)
```

### `camera.follow(camera: AnyObject, target: AnyObject)`

Troca o objeto que uma câmera com Controle de câmera acompanha.

Como Definir personagem acompanhado, normalmente um personagem 3D; a câmera segue no modo em que está. Vale só durante a partida.

- `camera` (`AnyObject`): o objeto com Controle de câmera
- `target` (`AnyObject`): o objeto que ela passa a acompanhar
- Para com erro: um objeto sem Controle de câmera; a própria câmera como alvo; um alvo que não existe mais.
- Nós: Definir personagem acompanhado (camera.target).
- 0.8: L (local). Desde a 0.7.0.

```lua
local jogador = scene.find("Jogador")
if jogador then
    camera.follow(self, jogador)
end
```

### `camera.look_at(camera: AnyObject, target: vector | AnyObject)`

Vira uma câmera para um ponto ou um objeto e a mantém assim, até `release_look`.

Como Olhar assistido: o mouse não gira a câmera até `camera.release_look`. A câmera precisa estar ativa.

- `camera` (`AnyObject`): o objeto com Controle de câmera, ativo
- `target` (`vector | AnyObject`): um ponto do mundo, em metros, ou um objeto
- Para com erro: um objeto sem Controle de câmera; uma câmera que não está ativa; um alvo que não é vetor nem objeto.
- Nós: Olhar assistido / liberar olhar (camera.look_at).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    camera.look_at(self, vector.create(0, 0, -10))
end
```

### `camera.release_look(camera: AnyObject)`

Devolve o olhar de uma câmera ao jogador, da vista de agora.

Como Liberar olhar ao jogador, do nó Olhar assistido / liberar olhar.

- `camera` (`AnyObject`): o objeto com Controle de câmera
- Para com erro: um objeto sem Controle de câmera; uma câmera que não está ativa.
- Nós: Olhar assistido / liberar olhar (camera.look_at).
- 0.8: L (local). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    camera.release_look(self)
end
```

### `camera.set(camera: AnyObject, setting: "fov" | "distance" | "shoulder", value: number, seconds: number?)`

Ajusta o enquadramento de uma câmera: campo de visão, distância ou ombro.

Como Ajustar enquadramento da câmera: a distância e o ombro pedem Controle de câmera, e o campo de visão vale para qualquer câmera. Diante de uma parede, a câmera chega mais perto que a distância pedida.

- `camera` (`AnyObject`): o objeto com Câmera
- `setting` (`"fov" | "distance" | "shoulder"`): "fov" (campo de visão vertical, 10° a 150°), "distance" (distância em terceira pessoa, m) ou "shoulder" (deslocamento do ombro, m)
- `value` (`number`): o valor, na unidade do ajuste
- `seconds` (`number?`): a suavização, em segundos (padrão 0,15)
- Para com erro: um objeto sem Câmera (ou sem Controle de câmera, para distance e shoulder); um ajuste desconhecido; um valor que a câmera não aceita.
- Nós: Ajustar enquadramento da câmera (camera.setting).
- 0.8: L (local). Desde a 0.7.0.

```lua
camera.set(self, "fov", 75, 0.3)
```

## Interface

O elemento de interface de um objeto (texto, imagem, botão ou barra), por `obj.ui`: o texto e a cor dele mudam durante a partida. Esconder um elemento é `obj.visible = false`; um clique num botão chega a `on_click`.

### `obj.ui: UiElement`

O elemento de interface do objeto: texto, imagem, botão ou barra.

Só leitura, num elemento de interface; num objeto que não é um, é um erro (confira antes com `obj:has_component("ui")`).

- Para com erro: o objeto não é um elemento de interface; o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.ui.text)
```

### `obj.ui.text: string`

O texto do elemento; um valor vinculado ainda entra no lugar de `{valor}`. Leitura e escrita.

Escrever troca o texto durante a partida, até 4.096 bytes. Num elemento vinculado a um atributo, o valor dele continua aparecendo em `{valor}` (ou depois do texto, sem a marca); ler dá o texto escrito, sem o valor. Com `time.format`, dispensa um atributo só para o texto.

- Para com erro: escrever outra coisa que um texto, ou mais de 4.096 bytes; o objeto não existe mais.
- 0.8: L (local). Desde a 0.7.0.

```lua
self.ui.text = "Tempo: " .. time.format(time.now(), "mm:ss")
```

### `obj.ui.color: vector`

A cor do elemento: vermelho, verde e azul de 0 a 1, em x, y e z. Leitura e escrita.

Escrever recusa um componente fora de 0 a 1; a opacidade fica como está.

- Para com erro: escrever outra coisa que um vetor, ou um componente fora de 0 a 1; o objeto não existe mais.
- 0.8: L (local). Desde a 0.7.0.

```lua
self.ui.color = vector.create(1, 0.2, 0.2)
```

## Áudio

Sons da Biblioteca, pelo nome que eles têm lá: uma vez nos efeitos, em loop na música ou nos efeitos, e os volumes dos canais (geral, música e efeitos).

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

### `audio.loop(name: string, options: LoopOptions?)`

Toca um som em loop, na música ou nos efeitos.

Como Tocar em loop: na música (o padrão), o mesmo som continua ao trocar ou reiniciar a cena, e outro entra com a transição enquanto o anterior sai; nos efeitos, o loop acaba com a cena (até 32, um por som). Pedir um loop que já toca só muda o volume dele. A pausa do jogo congela os efeitos e deixa a música.

- `name` (`string`): o nome do som na Biblioteca
- `options` (`LoopOptions?`): `{ channel = "effects", volume = 0.5, fade = 1 }`
- Para com erro: um som que o projeto não tem (o erro lista os sons); uma opção desconhecida, um canal que não é music nem effects, fade fora de 0 a 60; mais de 32 loops nos efeitos.
- Nós: Tocar em loop (action.sound_loop).
- 0.8: L (local). Desde a 0.7.0.

```lua
audio.loop("Bipe", { channel = "music", fade = 1 })
```

### `audio.stop(options: StopOptions?)`

Para os loops de um som, ou todos os de um canal.

Como Parar som: sem `sound`, para todos os loops do canal; o canal padrão é "all", a música e os efeitos. Parar um som que não toca não faz nada, e os sons de `audio.play` não param: acabam sozinhos.

- `options` (`StopOptions?`): `{ sound = "Trilha", channel = "music", fade = 2 }`
- Para com erro: um som que o projeto não tem; uma opção desconhecida, um canal que não é music, effects nem all, fade fora de 0 a 60.
- Nós: Parar som (action.sound_stop).
- 0.8: L (local). Desde a 0.7.0.

```lua
audio.stop({ channel = "music", fade = 2 })
```

### `audio.set_volume(channel: "master" | "music" | "effects", volume: number)`

Muda o volume geral, da música ou dos efeitos, de 0 a 1.

Como Volume do canal: vale para a partida toda, também depois de trocar a cena, e fora de 0 a 1 fica no limite. Cada som toca no volume geral vezes o do canal vezes o dele; para lembrar a escolha, guarde-a numa variável salva entre partidas.

- `channel` (`"master" | "music" | "effects"`): "master" (geral), "music" ou "effects"
- `volume` (`number`): de 0 a 1
- Para com erro: um canal que não é master, music nem effects; um volume que não é um número finito.
- Nós: Volume do canal (action.volume).
- 0.8: L (local). Desde a 0.7.0.

```lua
audio.set_volume("music", 0.5)
```

### `audio.volume(channel: "master" | "music" | "effects"): number`

O volume de agora de um canal, de 0 a 1.

Como Ler volumes: começa em 1 em cada partida e muda só com `audio.set_volume`.

- `channel` (`"master" | "music" | "effects"`): "master" (geral), "music" ou "effects"
- Para com erro: um canal que não é master, music nem effects.
- Nós: Ler volumes (audio.read).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
audio.set_volume("effects", math.clamp(audio.volume("effects") + 0.1, 0, 1))
```

## Jogo

O jogo como um todo: a pausa do jogo (o mundo para, a interface e a lógica seguem), o cursor do mouse e sair do jogo.

### `game.pause()`

Pausa o jogo: o mundo para; a interface, a entrada e a lógica seguem.

Como Pausar / retomar jogo (pausar): `on_pause` roda nos objetos que o ouvem, e `on_step`, os temporizadores, os personagens e as animações param até `game.resume`; cliques, ações de entrada e mensagens seguem chegando. Pausar um jogo já pausado não faz nada.

- Nós: Pausar / retomar jogo (game.pause).
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
game.pause()
```

### `game.resume()`

Retoma o jogo pausado.

Como Pausar / retomar jogo (retomar): `on_pause` roda de novo, com `paused` false. O que a lógica pediu aos personagens durante a pausa cai.

- Nós: Pausar / retomar jogo (game.pause).
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
game.resume()
```

### `game.paused(): boolean`

Se o jogo está pausado.

Como Ler estado do jogo. Alternar a pausa, como o modo Alternar do nó, é `if game.paused() then game.resume() else game.pause() end`.

- Nós: Ler estado do jogo (game.read).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if game.paused() then
    game.resume()
else
    game.pause()
end
```

### `game.show_cursor()`

Mostra o cursor do mouse e o solta da câmera.

Como Mostrar / esconder cursor (mostrar): o mouse fica livre e visível, para clicar na interface, e o olhar com o mouse para. Uma cena começa com ele escondido.

- Nós: Mostrar / esconder cursor (game.cursor).
- 0.8: L (local). Desde a 0.7.0.

```lua
game.show_cursor()
```

### `game.hide_cursor()`

Esconde o cursor do mouse e o devolve à câmera.

Como Mostrar / esconder cursor (esconder).

- Nós: Mostrar / esconder cursor (game.cursor).
- 0.8: L (local). Desde a 0.7.0.

```lua
game.hide_cursor()
```

### `game.quit()`

Sai do jogo: o jogo exportado fecha e o teste do editor para.

Como Sair do jogo; uma simulação termina. O resto da função ainda roda.

- Nós: Sair do jogo (game.quit).
- 0.8: L (local). Desde a 0.7.0.

```lua
if self.attr.Vida <= 0 then
    game.quit()
end
```

## Variáveis e progresso

As variáveis do jogo do projeto (Lógica > Variáveis do jogo), pelo nome, e o progresso salvo: as variáveis marcadas como salvas entre partidas. Elas guardam o valor quando a cena reinicia.

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

### `save.write()`

Salva o progresso: os valores de agora das variáveis salvas entre partidas.

Como Salvar progresso, com os valores das chaves: a próxima partida começa com eles. As variáveis que não são salvas entre partidas (Lógica > Variáveis do jogo) ficam de fora.

- Nós: Salvar progresso (save.write).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
vars.set("Pontos", 10)
save.write()
```

### `save.clear()`

Apaga o progresso salvo e volta as variáveis salvas ao valor inicial.

Como Apagar progresso: as chaves delas também somem.

- Nós: Apagar progresso (save.clear).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
save.clear()
```

### `save.exists(): boolean`

Se há progresso salvo.

Como Ler progresso salvo: o que a partida recebeu ao começar, depois do que `save.write` e `save.clear` fizeram.

- Nós: Ler progresso salvo (save.state).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
if save.exists() then
    print("continuar")
end
```

## Entrada

As ações de entrada do projeto (Lógica > Ações de entrada), pelo nome, e os eixos do passo. É a entrada do jogador que controla o objeto: na 0.7, o único jogador; o argumento `player` chega na 0.8.

### `input.pressed(action: string): boolean`

Se a ação de entrada foi apertada neste passo.

Como Ler ação de entrada (Pressionada): verdadeiro só no passo em que a tecla desce. Lê sem consumir: outros scripts, o grafo e o personagem veem o mesmo. O nome vale sem diferenciar maiúsculas; um segundo argumento, o jogador, chega na 0.8 e hoje é um erro.

- `action` (`string`): o nome da ação, como em Lógica > Ações de entrada
- Para com erro: uma ação que o projeto não tem (o erro lista as ações); um segundo argumento (o jogador chega na 0.8).
- Nós: Ler ação de entrada (input.read).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    if input.pressed("Pular") then
        print("pulou")
    end
end
```

### `input.held(action: string): boolean`

Se a ação de entrada está segura neste passo.

Como Ler ação de entrada (Mantida), com as regras de `input.pressed`.

- `action` (`string`): o nome da ação, como em Lógica > Ações de entrada
- Para com erro: uma ação que o projeto não tem (o erro lista as ações); um segundo argumento (o jogador chega na 0.8).
- Nós: Ler ação de entrada (input.read).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    if input.held("Atacar") then
        self.attr.Aviso = "carregando"
    end
end
```

### `input.released(action: string): boolean`

Se a ação de entrada foi solta neste passo.

Como Ler ação de entrada (Solta): verdadeiro só no passo em que a tecla sobe, com as regras de `input.pressed`.

- `action` (`string`): o nome da ação, como em Lógica > Ações de entrada
- Para com erro: uma ação que o projeto não tem (o erro lista as ações); um segundo argumento (o jogador chega na 0.8).
- Nós: Ler ação de entrada (input.read).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    if input.released("Atacar") then
        self.attr.Aviso = ""
    end
end
```

### `input.movement(): vector`

O eixo de movimento deste passo: x para a direita e y para a frente, de comprimento até 1.

Como Ler eixos de entrada (Movimento), com z 0: as teclas de movimento do Personagem 3D do objeto que roda o script, ou as do projeto (Mover à esquerda, à direita, para frente e para trás) num objeto sem ele. É o que `character:move` recebe.

- Para com erro: um argumento (o jogador chega na 0.8).
- Nós: Ler eixos de entrada (input.axes).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    local eixo = input.movement()
    self.position += vector.create(eixo.x, 0, -eixo.y) * 0.1
end
```

### `input.look(): vector`

Quanto o mouse andou neste passo: x para a direita e y para baixo.

Como Ler eixos de entrada (Olhar), com z 0: em unidades do dispositivo, não em graus. Com o cursor à mostra ou o jogo pausado, o olhar fica parado.

- Para com erro: um argumento (o jogador chega na 0.8).
- Nós: Ler eixos de entrada (input.axes).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    self.attr.Aviso = tostring(input.look().x)
end
```

### `input.wheel(): number`

Quanto a roda do mouse girou neste passo, em linhas: positivo para cima.

Como Ler eixos de entrada (Roda).

- Para com erro: um argumento (o jogador chega na 0.8).
- Nós: Ler eixos de entrada (input.axes).
- 0.8: J (do jogador). Desde a 0.7.0.

```lua
function M.on_step(self: Object)
    if input.wheel() > 0 then
        print("roda para cima")
    end
end
```

## Tempo e sorteio

O tempo de jogo, que para na pausa do jogo, e os sorteios da engine: cada objeto e script tem a própria sequência, e a mesma semente dá os mesmos números em toda máquina. `math.random` não existe nos scripts.

### `time.now(): number`

O tempo de jogo, em segundos desde o começo da partida.

Como Ler tempo da simulação: para na pausa do jogo e atravessa o reinício da cena. Guarde momentos (`self.state.pronto_em = time.now() + 0.5`) em vez de somar `dt` a cada passo.

- Nós: Ler tempo da simulação (time.read).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.attr.Aviso = time.format(time.now(), "mm:ss")
```

### `time.dt: number`

A duração de um passo de jogo, 1/60 s.

Um número, não uma função: o mesmo `dt` que `on_step` recebe, como a Duração do passo de Ler tempo da simulação.

- Nós: Ler tempo da simulação (time.read).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.position += vector.create(0, 0, 2 * time.dt)
```

### `time.format(seconds: number, format: "mm:ss" | "mm:ss.cc" | "m:ss.cc" | "ss.cc" | "mm:ss.mmm" | "h:mm:ss"): string`

Escreve segundos como tempo: `time.format(75.5, "mm:ss.cc")` dá "01:15.50".

Como Formatar tempo: corta (não arredonda) no último dígito do formato, e os minutos passam de 59 sem `h:mm:ss`. Não mexe no mundo: o topo do script pode usá-la.

- `seconds` (`number`): os segundos
- `format` (`"mm:ss" | "mm:ss.cc" | "m:ss.cc" | "ss.cc" | "mm:ss.mmm" | "h:mm:ss"`): o formato, como em Formatar tempo
- Para com erro: seconds que não é um número finito; um formato desconhecido (o erro lista os formatos).
- Nós: Formatar tempo (text.time).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
self.attr.Aviso = time.format(75.5, "mm:ss.cc")
```

### `random.number(min: number, max: number): number`

Sorteia um número real de min até max, sem max.

Como Sortear número (Real), da sequência do objeto e do script: outro script ou outro objeto sorteando a mais não muda estes números, e a mesma semente dá os mesmos em toda máquina.

- `min` (`number`): o menor valor
- `max` (`number`): o maior, que nunca sai (salvo min = max)
- Para com erro: min ou max que não é um número finito; min maior que max.
- Nós: Sortear número (random.number).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.attr.Vida = random.number(1, 10)
```

### `random.integer(min: number, max: number): number`

Sorteia um inteiro de min até max, os dois incluídos.

Como Sortear número (Inteiro), com as regras de `random.number`: `random.integer(1, 6)` é um dado.

- `min` (`number`): o menor inteiro
- `max` (`number`): o maior inteiro, que também pode sair
- Para com erro: min ou max que não é um número finito; nenhum inteiro de min a max; min ou max além de 2^53.
- Nós: Sortear número (random.number).
- 0.8: M (partida). Desde a 0.7.0.

```lua
self.attr.Vida = random.integer(1, 6)
```

### `random.seed(seed: number)`

Troca a semente dos sorteios: todas as sequências recomeçam.

Como Definir semente aleatória: vale para o grafo e para todos os scripts, e a mesma semente repete os mesmos números.

- `seed` (`number`): um inteiro de 0 a 2^53
- Para com erro: uma semente que não é um inteiro de 0 a 2^53.
- Nós: Definir semente aleatória (random.seed).
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
random.seed(42)
```

### `random.stream(name: string): RandomStream`

Uma sequência de sorteios com nome, independente da do script.

Os sorteios dela não mudam os de `random.number` nem os de outra sequência: um sorteio a mais nos inimigos não muda as cartas. A sequência é do objeto e do script que sorteiam. Não mexe no mundo: o topo do script pode guardá-la num local.

- `name` (`string`): o nome da sequência, de 1 a 256 bytes
- Para com erro: um nome vazio ou de mais de 256 bytes.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
local ondas = random.stream("ondas")
self.attr.Vida = ondas:integer(1, 3)
```

### `stream:number(min: number, max: number): number`

Sorteia um número real de min até max, sem max, desta sequência.

Como `random.number`, na sequência de `random.stream`.

- `min` (`number`): o menor valor
- `max` (`number`): o maior, que nunca sai (salvo min = max)
- Para com erro: min ou max que não é um número finito; min maior que max.
- Nós: Sortear número (random.number).
- 0.8: M (partida). Desde a 0.7.0.

```lua
local cartas = random.stream("cartas")
print(cartas:number(0, 1))
```

### `stream:integer(min: number, max: number): number`

Sorteia um inteiro de min até max, os dois incluídos, desta sequência.

Como `random.integer`, na sequência de `random.stream`.

- `min` (`number`): o menor inteiro
- `max` (`number`): o maior inteiro, que também pode sair
- Para com erro: min ou max que não é um número finito; nenhum inteiro de min a max.
- Nós: Sortear número (random.number).
- 0.8: M (partida). Desde a 0.7.0.

```lua
local cartas = random.stream("cartas")
print(cartas:integer(1, 52))
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

### `error(message: string)`

Para a chamada com um erro: a mensagem vai ao Console com o arquivo e a linha.

A função do próprio Luau. Como qualquer erro de script, só aquela chamada para: o que ela fez até ali fica, e o erro aparece uma vez, com um contador. Use para o que nunca deveria acontecer; uma falha esperada (um teleporte sem espaço) se confere pelo que a função devolve.

- `message` (`string`): o que deu errado
- 0.8: leitura pura. Desde a 0.7.0.

```lua
if self.attr.Vida < 0 then
    error("Vida negativa em " .. self.name)
end
```

### `assert(value: any, message: string?): any`

Para a chamada com um erro quando o valor é false ou nil; senão, devolve-o.

A função do próprio Luau, com as regras de `error`: `assert(scene.find("Porta"), "falta a Porta")` dá a Porta ou para ali.

- `value` (`any`): o que deve ser verdadeiro
- `message` (`string?`): a mensagem do erro
- 0.8: leitura pura. Desde a 0.7.0.

```lua
assert(self.attr.Vida >= 0, "Vida negativa")
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

## Reservado para a 0.8

Nomes que chegam na 0.8, com a rede e vários jogadores. Na 0.7, usar um deles é um erro que diz isso, e uma função do módulo com o nome de um evento da 0.8 gera um aviso: a engine ainda não a chama.

### `players`

Os jogadores da partida, a partir da 0.8.

Na 0.7 há um jogador só, o de quem joga: ler `players` é um erro que diz que ele chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: M (partida). Desde a 0.7.0.

```lua
print(pcall(function()
    return players
end))
```

### `Player`

Um jogador da partida, a partir da 0.8.

O nome fica guardado para o tipo e a tabela dos jogadores; na 0.7, ler `Player` é um erro que diz que ele chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: M (partida). Desde a 0.7.0.

```lua
print(pcall(function()
    return Player
end))
```

### `session`

A sessão em rede, a partir da 0.8.

Na 0.7 não há rede: ler `session` é um erro que diz que ela chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: M (partida). Desde a 0.7.0.

```lua
print(pcall(function()
    return session
end))
```

### `game.online`

Se a partida é em rede, a partir da 0.8.

Na 0.7 toda partida é local: ler `game.online` é um erro que diz que ele chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(pcall(function()
    return game.online
end))
```

### `game.is_host`

Se esta máquina hospeda a partida, a partir da 0.8.

Ler `game.is_host` na 0.7 é um erro que diz que ele chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(pcall(function()
    return game.is_host
end))
```

### `game.is_server`

Se o script roda na autoridade da partida, a partir da 0.8.

Ler `game.is_server` na 0.7 é um erro que diz que ele chega na 0.8.

- Para com erro: qualquer uso na 0.7 (chega na 0.8).
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(pcall(function()
    return game.is_server
end))
```

### `on_request(self: Object, player: any, name: string, value: any)`

Um jogador pediu algo à partida, a partir da 0.8.

Na 0.8, uma mensagem de um script local para a partida chega aqui. Na 0.7 a engine nunca chama esta função, e a conferência avisa.

- `player` (`any`): o jogador que pede
- `name` (`string`): o nome do pedido
- `value` (`any`): o valor do pedido
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
function M.on_request(self: Object, player: any, name: string, value: any)
    print(name)
end
```

### `on_player_join(self: Object, player: any)`

Um jogador entrou na partida, a partir da 0.8.

Na 0.7 a engine nunca chama esta função, e a conferência avisa.

- `player` (`any`): o jogador que entrou
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
function M.on_player_join(self: Object, player: any)
    print("entrou")
end
```

### `on_player_leave(self: Object, player: any)`

Um jogador saiu da partida, a partir da 0.8.

Na 0.7 a engine nunca chama esta função, e a conferência avisa.

- `player` (`any`): o jogador que saiu
- 0.8: A (autoridade). Desde a 0.7.0.

```lua
function M.on_player_leave(self: Object, player: any)
    print("saiu")
end
```

## Nós que viram Luau

O que estas operações dos nós fazem o próprio Luau faz, sem uma função da API; as outras estão nas páginas acima, no item **Nós**.

- Número (`value.number`): Um número escrito no código: `3`, `0.5`.
- Texto (`value.text`): Um texto escrito no código: `"Porta"`.
- Booleano (`value.bool`): `true` e `false`.
- Operação aritmética (`math.binary`): `a + b`, `a - b`, `a * b`, `a / b`, `a % b` (não negativo com divisor positivo, como o nó), `math.min(a, b)` e `math.max(a, b)`; dividir por zero dá `inf`, que a engine recusa ao gravar.
- Função matemática (`math.unary`): `math.floor`, `math.ceil`, `math.round` (metades para longe do zero, como o nó), `math.abs`, `math.sign`, `math.sqrt` e, para truncar, `(math.modf(x))`.
- Limitar entre mínimo e máximo (`math.clamp`): `math.clamp(x, min, max)`.
- Comparar (`condition.compare`): `==`, `~=`, `<`, `<=`, `>` e `>=`.
- Se / Senão (`condition.branch`): `if … then … else … end`.
- Sequência (`control.sequence`): Uma linha depois da outra, na mesma função.
- Vetor2 constante (`value.vector2`): `vector.create(x, y, 0)`: um Vetor2 chega aos scripts como vetor com z 0.
- Construir Vetor2 (`vector2.make`): `vector.create(x, y, 0)`.
- Decompor Vetor2 (`vector2.parts`): `v.x` e `v.y`.
- Operações com Vetor2 (`vector2.math`): `a + b`, `a - b`, `a * s`, `vector.magnitude(a)`, `vector.dot(a, b)` e `vmath.normalize_or_zero(a)`.
- Vetor3 constante (`value.vector3`): `vector.create(x, y, z)`.
- Construir Vetor3 (`vector3.make`): `vector.create(x, y, z)`.
- Decompor Vetor3 (`vector3.parts`): `v.x`, `v.y` e `v.z`.
- Operações com Vetor3 (`vector3.math`): `a + b`, `a - b`, `a * s`, `vector.magnitude(a)` e `vector.dot(a, b)`; o normalizado com zero seguro é `vmath.normalize_or_zero(a)`.

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
    --- O Personagem 3D do objeto: o estado e os comandos dele.
    --- Só leitura, num objeto com Personagem 3D; num objeto sem ele, é um erro (confira antes com `obj:has_component("character")`). O personagem anda pelos comandos (`move`, `jump`, `teleport`…), não pela `position` do objeto.
    character: Character,
    --- O colisor do objeto: 2D, 3D ou área.
    --- Só leitura, num objeto com colisor ou área; num objeto sem eles, é um erro (confira antes com `obj:has_component("collider")`). O corpo de um Personagem 3D não é um colisor: use `obj.character` (a superfície do corpo é `obj.character.body_surface`).
    collider: Collider,
    --- A plataforma móvel do objeto.
    --- Só leitura, num objeto com plataforma móvel; num objeto sem ela, é um erro (confira antes com `obj:has_component("platform")`).
    platform: Platform,
    --- O Movimento 2D do objeto.
    --- Só leitura, num objeto com Movimento 2D; num objeto sem ele, é um erro (confira antes com `obj:has_component("controller")`).
    controller: Controller,
    --- As animações (clipes) do objeto.
    --- Só leitura, num objeto com clipes; num objeto sem eles, é um erro (confira antes com `obj:has_component("animation")`).
    animation: Animation,
    --- O elemento de interface do objeto: texto, imagem, botão ou barra.
    --- Só leitura, num elemento de interface; num objeto que não é um, é um erro (confira antes com `obj:has_component("ui")`).
    ui: UiElement,
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
    --- O Personagem 3D do objeto: o estado e os comandos dele.
    --- Só leitura, num objeto com Personagem 3D; num objeto sem ele, é um erro (confira antes com `obj:has_component("character")`). O personagem anda pelos comandos (`move`, `jump`, `teleport`…), não pela `position` do objeto.
    character: Character,
    --- O colisor do objeto: 2D, 3D ou área.
    --- Só leitura, num objeto com colisor ou área; num objeto sem eles, é um erro (confira antes com `obj:has_component("collider")`). O corpo de um Personagem 3D não é um colisor: use `obj.character` (a superfície do corpo é `obj.character.body_surface`).
    collider: Collider,
    --- A plataforma móvel do objeto.
    --- Só leitura, num objeto com plataforma móvel; num objeto sem ela, é um erro (confira antes com `obj:has_component("platform")`).
    platform: Platform,
    --- O Movimento 2D do objeto.
    --- Só leitura, num objeto com Movimento 2D; num objeto sem ele, é um erro (confira antes com `obj:has_component("controller")`).
    controller: Controller,
    --- As animações (clipes) do objeto.
    --- Só leitura, num objeto com clipes; num objeto sem eles, é um erro (confira antes com `obj:has_component("animation")`).
    animation: Animation,
    --- O elemento de interface do objeto: texto, imagem, botão ou barra.
    --- Só leitura, num elemento de interface; num objeto que não é um, é um erro (confira antes com `obj:has_component("ui")`).
    ui: UiElement,
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

--- O Personagem 3D de um objeto, por `obj.character`: o estado e os comandos dele.
export type Character = {
    --- A posição dos pés do personagem no mundo, em metros.
    --- Só leitura, como a Posição de Ler personagem 3D: a origem do corpo, nos pés. Para mover o personagem, use `move`, `set_velocity` ou `teleport`.
    position: vector,
    --- A velocidade total do personagem, em m/s, com a do apoio.
    --- Só leitura, como a Velocidade total de Ler personagem 3D: sobre uma plataforma que anda, inclui a velocidade dela.
    velocity: vector,
    --- A velocidade própria do personagem, em m/s: o embalo, sem a do apoio.
    --- Só leitura, como a Velocidade própria de Ler personagem 3D.
    own_velocity: vector,
    --- A velocidade total do personagem no plano, em m/s: a mesma com y = 0.
    --- Só leitura, como a Velocidade horizontal de Ler personagem 3D.
    horizontal_velocity: vector,
    --- A rapidez horizontal do personagem, em m/s.
    --- Só leitura, como a Rapidez horizontal de Ler personagem 3D: o comprimento de `horizontal_velocity`.
    speed: number,
    --- Se o personagem está no chão, num apoio em que pode ficar em pé.
    --- Só leitura, como No chão de Ler personagem 3D; uma parede não é apoio.
    grounded: boolean,
    --- Se o personagem está no ar, subindo.
    --- Só leitura, como Subindo de Ler personagem 3D.
    rising: boolean,
    --- Se o personagem está no ar, caindo.
    --- Só leitura, como Caindo de Ler personagem 3D.
    falling: boolean,
    --- A postura do personagem: "standing" (em pé), "crouched" (agachado) ou "sliding" (deslizando).
    --- Só leitura; muda com `set_posture`. A Postura de Ler personagem 3D é a mesma, em português.
    posture: "standing" | "crouched" | "sliding",
    --- O objeto em que o personagem pisa, ou nil no ar.
    --- Só leitura, como Apoio de Ler personagem 3D.
    support: Object?,
    --- A superfície física em que o personagem pisa, ou nil no ar ou na padrão.
    --- Só leitura, como Superfície de Ler personagem 3D: a do apoio, e não a do corpo do personagem (`body_surface`).
    surface: Surface?,
    --- A superfície física do corpo do personagem, ou nil para a padrão.
    --- Como Ler superfície do colisor e Aplicar superfície ao colisor num Personagem 3D: a do corpo, e não a do chão em que ele pisa (`surface`). Recebe uma superfície (de um atributo Superfície, de um colisor) ou nil; o nome de uma superfície do projeto também serve, mas os tipos do VS Code só conhecem a superfície. Vale só durante a partida.
    body_surface: Surface?,
    --- O ponto de apoio sob os pés, no mundo, em metros; zero no ar.
    --- Só leitura, como Ponto de apoio de Ler personagem 3D.
    support_point: vector,
    --- Para onde o chão sob os pés aponta (a normal do apoio); zero no ar.
    --- Só leitura, como Normal de apoio de Ler personagem 3D: num chão plano, (0, 1, 0).
    support_normal: vector,
    --- A velocidade do apoio, em m/s, como a de uma plataforma que anda; zero no ar.
    --- Só leitura, como Velocidade do apoio de Ler personagem 3D.
    support_velocity: vector,
    --- Se algum motivo bloqueia o movimento do personagem.
    --- Só leitura; os motivos vêm de `block`, e `block_reasons("movement")` os lista.
    movement_blocked: boolean,
    --- Se algum motivo bloqueia o olhar do personagem.
    --- Só leitura; os motivos vêm de `block`, e `block_reasons("look")` os lista.
    look_blocked: boolean,
    --- A direção da mira do personagem no mundo, de comprimento 1, vinda da entrada do jogo.
    --- Só leitura: o giro e a inclinação do olhar que a entrada deu ao personagem (o mouse, pela câmera que o acompanha), e não a imagem da câmera, que pode estar suavizada ou em transição. Para mirar e atirar, dá o mesmo em toda máquina.
    aim: vector,
    --- O olhar do personagem em graus: x é o giro (0 olha para -Z, 90 para -X) e y a inclinação (positiva para cima).
    --- Só leitura; z é 0. É o que `teleport` recebe em `look`: guarde-o com a posição para o jogador voltar a um ponto olhando para o mesmo lado.
    look: vector,
    --- Os motivos que bloqueiam o movimento ou o olhar do personagem, em ordem.
    --- Uma lista nova a cada chamada, vazia sem bloqueio; os motivos vêm de `block`. Ler personagem 3D dá os mesmos, juntos num texto.
    block_reasons: (self: Character, channel: "movement" | "look") -> { string },
    --- Pede ao personagem que ande no próximo passo: x para a direita, y para a frente.
    --- Como Definir intenção para este passo: vale só no próximo passo de movimento, então renove a cada passo (`on_step`); um vetor mais longo que 1 é encurtado. A direção segue a referência do personagem (eixos da cena, do corpo ou da câmera). O pedido vence as teclas naquele passo; `input.movement()` dá o eixo das teclas, e com as ações automáticas desligadas (`set_option`) o personagem só anda assim.
    move: (self: Character, axis: vector) -> (),
    --- Pede um pulo ao personagem, como a tecla de pular.
    --- Como Pedir pulo: pula no chão ou logo depois da borda; no ar, o pedido espera pela antecipação do pulo. Vários pedidos no mesmo passo são um pulo só; a altura vem da velocidade de pulo (`get_param("jump_speed")`).
    jump: (self: Character) -> (),
    --- Liga ou desliga correr, agachar ou deslizar.
    --- Como Correr / agachar / deslizar, e vale até mudar de novo. Ficar em pé pede espaço acima; deslizar pede o deslize permitido no personagem e embalo, e desligá-lo levanta o personagem se couber.
    set_posture: (self: Character, kind: "sprint" | "crouch" | "slide", on: boolean) -> (),
    --- Soma uma velocidade em m/s, no mundo, ao embalo do personagem.
    --- Como o modo Adicionar de Adicionar / definir velocidade: preserva o embalo, e para cima tira o personagem do chão. Não é força: `vector.create(0, 8, 0)` é um impulso de 8 m/s para cima.
    add_velocity: (self: Character, velocity: vector) -> (),
    --- Troca a velocidade total do personagem por esta, em m/s, no mundo.
    --- Como o modo Definir de Adicionar / definir velocidade: o embalo vira o que falta para a velocidade total, contando a do apoio.
    set_velocity: (self: Character, velocity: vector) -> (),
    --- Bloqueia ou libera o movimento ou o olhar do personagem por um motivo.
    --- Como Bloquear / liberar entrada por motivo: os motivos se somam, e liberar um não libera os outros. Não desliga a gravidade, a colisão nem a câmera.
    block: (self: Character, channel: "movement" | "look", reason: string, on: boolean) -> (),
    --- Aplica ao personagem um perfil de movimento pronto.
    --- Como Aplicar perfil de movimento, com os valores do botão do editor; o nome antigo `"parkour"` vale como `"agile"`. Não teleporta nem zera a velocidade; um limite horizontal menor que o embalo de agora é recusado sem `allow_limit`. Vale só durante a partida.
    apply_profile: (self: Character, profile: "direct" | "agile" | "chained", allow_limit: boolean?) -> (),
    --- Muda um valor do motor do personagem durante a partida.
    --- Como Alterar parâmetro do personagem: velocidades em m/s (`speed`, `sprint_speed`, `crouch_speed`, `jump_speed`, `air_projected_limit`, `horizontal_limit`), acelerações em m/s² (`gravity`, `ground_acceleration`, `ground_braking`, `air_acceleration`, `slide_control`), atritos e resistência por segundo, conservações de 0 a 1, tolerâncias em milissegundos (`coyote_ms`, `jump_buffer_ms`), o deslize em segundos e o giro em graus por segundo. Um valor que o personagem não aceita é um erro e nada muda. Vale só durante a partida.
    set_param: (self: Character, name: "speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed", value: number, allow_limit: boolean?) -> (),
    --- Lê um valor do motor do personagem, como está na partida.
    --- Os nomes e as unidades de `set_param`; dá o valor de agora, com o que `set_param` e `apply_profile` mudaram.
    get_param: (self: Character, name: "speed" | "sprint_speed" | "crouch_speed" | "gravity" | "jump_speed" | "ground_acceleration" | "ground_braking" | "ground_friction" | "air_acceleration" | "air_projected_limit" | "air_resistance" | "horizontal_limit" | "jump_retention" | "landing_retention" | "coyote_ms" | "jump_buffer_ms" | "slide_duration" | "slide_friction" | "slide_control" | "angular_speed") -> number,
    --- Liga ou desliga uma opção do personagem durante a partida.
    --- Como Alterar opção do personagem: `automatic_input` (as teclas movem o personagem; desligada, só `move`, `jump` e `set_posture`), `slide_enabled` (deslize permitido), `crouch_toggle` (a tecla de agachar alterna), `automatic_jump` (pular ao segurar) e `face_look` (o corpo vira para o olhar). Vale só durante a partida.
    set_option: (self: Character, name: "automatic_input" | "slide_enabled" | "crouch_toggle" | "automatic_jump" | "face_look", on: boolean) -> (),
    --- Gira o corpo do personagem para um giro horizontal do mundo, em graus.
    --- Como Definir giro horizontal do personagem: 0 olha para -Z e 90 para -X, como `obj.yaw` lê. Se o corpo girado ocupar um obstáculo, nada muda e é um erro; em primeira pessoa, a câmera que o segue gira junto.
    set_yaw: (self: Character, degrees: number) -> (),
    --- Põe os pés do personagem num ponto do mundo, se houver espaço; devolve ok, o motivo da recusa e o destino usado.
    --- Como Teleportar com segurança. Opções: `search`, até quantos metros procurar um lugar livre perto (0 a 2; sem ela, um destino ocupado é recusado); `keep_velocity` (sem ela, a velocidade zera); `yaw`, o giro do corpo em graus, e `look`, o olhar como `character.look` o dá (sem eles, ficam como estão). Uma recusa não é erro: `ok` é false, o segundo valor diz por quê e o personagem fica onde estava. Os eventos do trajeto abandonado que ainda não rodaram caem, como no nó.
    teleport: (self: Character, feet: vector, options: TeleportOptions?) -> (boolean, string?, vector?),
}

--- O colisor (2D, 3D ou área) de um objeto, por `obj.collider`.
export type Collider = {
    --- Se o colisor (ou a área) está ligado.
    --- Como Ativar / desativar componente (Colisor): desligado, não colide nem sente quem entra. Religar uma área começa uma ativação nova: quem está dentro entra de novo (`on_area_enter`), e `first_hit` recomeça.
    enabled: boolean,
    --- Se o colisor é uma área (sensor), que sente quem entra sem colidir.
    --- Só leitura, como as Propriedades marcam.
    is_area: boolean,
    --- A superfície física do colisor 3D, ou nil para a padrão.
    --- Como Ler superfície do colisor e Aplicar superfície ao colisor: recebe uma superfície (de um atributo Superfície, de outro colisor) ou nil; o nome de uma superfície do projeto também serve, mas os tipos do VS Code só conhecem a superfície. Só um colisor 3D tem superfície; a do corpo de um personagem é `obj.character.body_surface`. Vale só durante a partida.
    surface: Surface?,
}

--- A plataforma móvel de um objeto, por `obj.platform`.
export type Platform = {
    --- Se a plataforma móvel leva quem está em cima.
    --- Como Ativar / desativar componente (Plataforma móvel).
    enabled: boolean,
}

--- O Movimento 2D de um objeto, por `obj.controller`.
export type Controller = {
    --- Se o Movimento 2D do objeto obedece às teclas.
    --- Como Ativar / desativar componente (Controlador).
    enabled: boolean,
}

--- As animações (clipes) de um objeto, por `obj.animation`.
export type Animation = {
    --- Toca um clipe do objeto pelo nome.
    --- Como Reproduzir animação: o nome vale sem diferenciar maiúsculas. Os marcadores do clipe chegam a `on_marker`; um objeto que um clipe tocando posiciona não aceita `position` até ele parar.
    play: (self: Animation, clip: string, restart: boolean?) -> (),
    --- Se o clipe do objeto está tocando: false pausa onde está, true retoma.
    --- Como Ativar / desativar componente (Animação). Antes de algum clipe tocar, é false e true não faz nada: comece com `play`.
    playing: boolean,
    --- O nome do clipe que toca (ou que parou), ou nil se nenhum tocou.
    --- Só leitura; um clipe que acabou ou foi pausado continua sendo o clipe, com `playing` false.
    clip: string?,
}

--- O elemento de interface de um objeto, por `obj.ui`.
export type UiElement = {
    --- O texto do elemento; um valor vinculado ainda entra no lugar de `{valor}`.
    --- Escrever troca o texto durante a partida, até 4.096 bytes. Num elemento vinculado a um atributo, o valor dele continua aparecendo em `{valor}` (ou depois do texto, sem a marca); ler dá o texto escrito, sem o valor. Com `time.format`, dispensa um atributo só para o texto.
    text: string,
    --- A cor do elemento: vermelho, verde e azul de 0 a 1, em x, y e z.
    --- Escrever recusa um componente fora de 0 a 1; a opacidade fica como está.
    color: vector,
}

--- Uma sequência de sorteios com nome, de `random.stream`: independente das outras.
export type RandomStream = {
    --- Sorteia um número real de min até max, sem max, desta sequência.
    --- Como `random.number`, na sequência de `random.stream`.
    number: (self: RandomStream, min: number, max: number) -> number,
    --- Sorteia um inteiro de min até max, os dois incluídos, desta sequência.
    --- Como `random.integer`, na sequência de `random.stream`.
    integer: (self: RandomStream, min: number, max: number) -> number,
}

--- Opções de `obj.character:teleport`.
export type TeleportOptions = {
    --- Até quantos metros procurar um lugar livre perto do destino, de 0 a 2; 0 (o padrão) recusa um destino ocupado.
    search: number?,
    --- true mantém a velocidade total; sem ela, o personagem chega parado.
    keep_velocity: boolean?,
    --- O giro do corpo no mundo, em graus (0 olha para -Z); sem ele, fica o de agora.
    yaw: number?,
    --- O olhar, como `character.look` dá: x o giro e y a inclinação, em graus; sem ele, fica o de agora.
    look: vector?,
}

--- O primeiro contato de uma consulta de `physics`.
export type Hit = {
    --- O objeto atingido.
    object: Object,
    --- O ponto do contato, no mundo, em metros.
    point: vector,
    --- A normal da superfície atingida, no mundo.
    normal: vector,
    --- A distância percorrida até o contato, em metros (em `capsule_fits`, a penetração, negativa).
    distance: number,
    --- A fração do percurso até o contato, de 0 a 1.
    fraction: number,
    --- A superfície física do que foi atingido, ou nil para a padrão.
    surface: Surface?,
}

--- Opções das consultas de `physics`.
export type QueryOptions = {
    --- true inclui as áreas (sensores); sem ela, só o que colide.
    areas: boolean?,
    --- Um objeto que a consulta ignora, com tudo o que está nele: o próprio personagem, para não se atingir.
    exclude: AnyObject?,
    --- Os grupos de colisão a que a consulta pertence: os nomes dos grupos (como no editor) ou a máscara em número (padrão 1, Cenário padrão).
    category: (number | string | { string })?,
    --- Os grupos de colisão que a consulta encontra: os nomes ou a máscara em número (padrão: todos).
    mask: (number | string | { string })?,
}

--- Opções de `audio.loop`.
export type LoopOptions = {
    --- O canal: "music" (o padrão) ou "effects".
    channel: ("music" | "effects")?,
    --- Volume de 0 a 1 (padrão 1).
    volume: number?,
    --- A entrada suave, em segundos, de 0 a 60 (padrão 0).
    fade: number?,
}

--- Opções de `audio.stop`.
export type StopOptions = {
    --- O som que para, pelo nome na Biblioteca; sem ele, todos os do canal.
    sound: string?,
    --- O canal: "music", "effects" ou "all" (o padrão), os dois.
    channel: ("music" | "effects" | "all")?,
    --- A saída suave, em segundos, de 0 a 60 (padrão 0).
    fade: number?,
}
```
