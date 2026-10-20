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

Os objetos chegam aos scripts como valores estáveis: `self`, o `other` dos eventos, os atributos Objeto e o remetente das mensagens. Um objeto removido continua sendo um valor no script, mas ler ou escrever nele é um erro.

### `obj.name: string`

O nome do objeto na cena.

Só leitura. `print` e `tostring` mostram um objeto por esse nome.

- Para com erro: o objeto não existe mais.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
print(self.name)
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

## Transformação

Onde o objeto está, em metros, como vetores do Luau (`vector.create(x, y, z)`).

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

## Atributos

Os atributos do objeto, os mesmos das Propriedades: a ponte com a interface, o grafo e os outros objetos. O script declara os que espera em `Modulo.attributes`, e anexá-lo cria no objeto os que faltam.

### `obj.attr: { [string]: any }`

Os atributos do objeto, pelo nome.

`obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto vazio lê nil e recebe nil para ficar vazio; um atributo Superfície ainda lê nil. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`.

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

O tipo sai do padrão: número, texto, booleano ou vetor (Vetor3); para os tipos sem valor literal, `param.object()` e `param.vector2(x, y)` (um atributo Superfície ainda não se declara). Anexar o script cria no objeto, no mesmo passo de desfazer, os que ele não tem; um que ele tem com outro tipo fica como está, com um aviso, e dois scripts do objeto que declaram o mesmo nome com tipos diferentes também geram um aviso. Um atributo declarado depois de anexar só aparece ao anexar de novo.

- Para com erro: um padrão que não é número, texto, booleano, vetor nem marcador `param`: anexar avisa e não cria nenhum.
- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Velocidade = 90, Aviso = "", Alvo = param.object() }
```

### `param.object(): Object?`

Declara um atributo Objeto, vazio até alguém escolher o objeto.

Em `Modulo.attributes = { Alvo = param.object() }`: anexar cria o atributo vazio, e o objeto se escolhe nas Propriedades (ou com `set_attribute` pelo MCP), o que sobrevive a renomear o objeto. Lido no script, é o objeto ou nil. Ainda não serve em `Modulo.state`.

- 0.8: leitura pura. Desde a 0.7.0.

```lua
M.attributes = { Alvo = param.object() }
```

### `param.vector2(x: number, y: number): vector`

Declara um atributo Vetor2 com o valor padrão (x, y).

Em `Modulo.attributes = { Tamanho = param.vector2(1, 1) }`. Lido no script, um Vetor2 é um vetor com z = 0; escrito, recebe o x e o y de um vetor.

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

Valores: booleano, número, texto, vetor, objeto e tabelas desses valores, com chaves inteiras ou de texto, até 4 níveis e 1.024 entradas por tabela; até 64 KB por objeto. Um campo precisa de um valor inicial de verdade (um nil some da tabela), e um campo de objeto que começa vazio ainda não existe: use um atributo Objeto (`param.object()`).

- Para com erro: um valor que a memória não guarda (função, tabela com metatabela, marcador `param`): o script não carrega; mais de 64 KB: o script não carrega.
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
M.state = { pulos = 0, nomes = {} :: { string } }
```

### `obj.state: any`

A memória deste script neste objeto, com os campos de `Modulo.state`.

Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto) é lido e gravado na engine na hora, conferido na linha; uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.

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
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor nem objeto, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
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
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor nem objeto, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
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
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor nem objeto, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
- Nós: Esperar sem bloquear (control.wait).
- 0.8: do objeto dono. Desde a 0.7.0.

```lua
self:sequence(1, M.mostrar, { 3, 2, 1 })
```

### `obj:schedule(steps: { Step }): number`

Passos com atrasos próprios: `{ {atraso, M.f, dado?}, ... }`.

Cada atraso conta do passo anterior; o primeiro, da chamada, no mínimo o passo seguinte.

- `steps` (`{ Step }`): os passos `{ atraso, M.f, dado? }`, até 1.024
- Para com erro: chamado em outro objeto que não `self`; tempo que não é um número de 0 a 86400 segundos; função criada na hora em vez de uma da tabela do módulo (`M.fechar`); dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor nem objeto, ou mais de 16 dados; a fila sem lugar para mais temporizadores: o script para no objeto.
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

--- Um objeto da cena. Chega aos scripts como `self` e nos argumentos dos eventos.
export type Object = {
    --- O nome do objeto na cena.
    --- Só leitura. `print` e `tostring` mostram um objeto por esse nome.
    name: string,
    --- Diz se o objeto ainda está na cena.
    --- Um objeto removido continua sendo um valor no script (num atributo, na memória ou numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.
    exists: (self: Object) -> boolean,
    --- Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.
    --- A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os scripts da cena, 4.096.
    send: (self: Object, name: string, value: any?) -> (),
    --- A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram.
    --- Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma animação tocando posiciona.
    position: vector,
    --- A posição do pivô do objeto no mundo, em metros.
    --- Num personagem 3D, a origem dele. Escrever move o objeto como `position`.
    world_position: vector,
    --- Os atributos do objeto, pelo nome.
    --- `obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto vazio lê nil e recebe nil para ficar vazio; um atributo Superfície ainda lê nil. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`.
    attr: { [string]: any },
    --- Diz se o objeto tem o atributo.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área.
    has_attribute: (self: Object, name: string) -> boolean,
    --- A memória deste script neste objeto, com os campos de `Modulo.state`.
    --- Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto) é lido e gravado na engine na hora, conferido na linha; uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.
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
    --- O nome do objeto na cena.
    --- Só leitura. `print` e `tostring` mostram um objeto por esse nome.
    name: string,
    --- Diz se o objeto ainda está na cena.
    --- Um objeto removido continua sendo um valor no script (num atributo, na memória ou numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.
    exists: (self: ScriptObject<A, S>) -> boolean,
    --- Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.
    --- A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os scripts da cena, 4.096.
    send: (self: ScriptObject<A, S>, name: string, value: any?) -> (),
    --- A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram.
    --- Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma animação tocando posiciona.
    position: vector,
    --- A posição do pivô do objeto no mundo, em metros.
    --- Num personagem 3D, a origem dele. Escrever move o objeto como `position`.
    world_position: vector,
    --- Os atributos do objeto, pelo nome.
    --- `obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = 2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto vazio lê nil e recebe nil para ficar vazio; um atributo Superfície ainda lê nil. Nomes com espaços vão entre colchetes: `obj.attr["Ponto de retorno"]`.
    attr: A & {},
    --- Diz se o objeto tem o atributo.
    --- Confira antes de agir sobre outro objeto, como o que entrou numa área.
    has_attribute: (self: ScriptObject<A, S>, name: string) -> boolean,
    --- A memória deste script neste objeto, com os campos de `Modulo.state`.
    --- Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto) é lido e gravado na engine na hora, conferido na linha; uma tabela é montada na primeira leitura da chamada e gravada de volta quando a chamada termina, com ou sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, `table.clone`): guarde listas num campo.
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
```
