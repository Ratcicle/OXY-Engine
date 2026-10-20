-- Eventos: funções do módulo com estes nomes, que a engine chama sozinha.
--   on_start(self)                       A cena começou, ou uma cópia com o script surgiu.
--   on_step(self, dt)                    A cada passo de jogo, 60 vezes por segundo.
--   on_input(self, action, phase)        Uma ação de entrada foi apertada ou solta.
--   on_click(self)                       Um clique no objeto ou no botão de interface dele.
--   on_area_enter(self, other)           Outro objeto entrou na área do objeto.
--   on_area_exit(self, other)            Outro objeto saiu da área do objeto.
--   on_marker(self, marker, clip)        Uma animação do objeto passou por um marcador.
--   on_character(self, event)            Um evento do personagem 3D do objeto: pulo, pouso e outros.
--   on_pause(self, paused)               O jogo entrou ou saiu da pausa dele.
--   on_message(self, name, value, from)  Uma mensagem que um objeto mandou com `obj:send`.
-- Campos do módulo que a engine lê ao carregar o script:
--   Modulo.attributes                    Os atributos que o script espera no objeto, com o valor padrão de cada um.
--   Modulo.state                         Os campos da memória do script em cada objeto, com o valor inicial de cada um.

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

declare param: {
    --- Declara um atributo Objeto, vazio até alguém escolher o objeto.
    --- Em `Modulo.attributes = { Alvo = param.object() }`: anexar cria o atributo vazio, e o objeto se escolhe nas Propriedades (ou com `set_attribute` pelo MCP), o que sobrevive a renomear o objeto. Lido no script, é o objeto ou nil. Ainda não serve em `Modulo.state`.
    object: () -> Object?,
    --- Declara um atributo Vetor2 com o valor padrão (x, y).
    --- Em `Modulo.attributes = { Tamanho = param.vector2(1, 1) }`. Lido no script, um Vetor2 é um vetor com z = 0; escrito, recebe o x e o y de um vetor.
    vector2: (x: number, y: number) -> vector,
}

declare audio: {
    --- Toca uma vez o som `name`, pelo nome que ele tem na Biblioteca.
    --- O nome vale também sem diferenciar maiúsculas nem os espaços das pontas. `audience` é aceito desde já, mas só escolhe quem ouve a partir da 0.8, com vários jogadores: hoje o único jogador ouve todo som.
    play: (name: string, options: PlayOptions?) -> (),
}

declare vars: {
    --- Lê uma variável do jogo pelo nome.
    --- O nome vale sem diferenciar maiúsculas. Com `key`, o valor daquela chave; numa chave nunca gravada, o valor inicial da variável, como o nó Ler variável do jogo, e nunca nil. Um Vetor2 lê como vetor com z = 0. As variáveis guardam o valor quando a cena reinicia: zere no `on_start` o que precisa recomeçar.
    get: (name: string, key: string?) -> any,
    --- Grava uma variável do jogo pelo nome, com um valor do tipo dela.
    --- Com `key`, grava o valor daquela chave, até 1.024 chaves por variável. Uma variável Vetor2 recebe o x e o y de um vetor.
    set: (name: string, value: VarValue, key: string?) -> (),
}

--- Escreve uma linha no Console, com o nome do objeto e a linha do script.
--- Objetos saem pelo nome e tabelas com o primeiro nível. O `oxy_simulate` e o `oxy_console` trazem as linhas com o arquivo e a linha.
declare function print(...: any): ()

--- Escreve um aviso no Console, com o nome do objeto e a linha do script.
--- Como `print`, com "aviso:" antes do texto.
declare function warn(...: any): ()
