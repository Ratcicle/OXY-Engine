-- Eventos: funções do módulo com estes nomes, que a engine chama sozinha.
--   on_start(self)                       a cena começou, ou uma cópia com o script surgiu
--   on_step(self, dt)                    a cada passo de 1/60 s
--   on_input(self, acao, fase)           uma ação de entrada, pelo nome; fase "pressed" ou "released"
--   on_click(self)                       um clique no objeto ou no botão da interface
--   on_area_enter(self, other)           outro objeto entrou na área (sensor) do objeto
--   on_area_exit(self, other)            outro objeto saiu da área
--   on_marker(self, marcador, clipe)     uma animação do objeto passou por um marcador
--   on_character(self, evento)           pulo, pouso e outros eventos do personagem do objeto
--   on_pause(self, pausado)              o jogo entrou ou saiu da pausa dele
--   on_message(self, nome, valor, de)    uma mensagem que outro objeto mandou com obj:send
-- Parâmetros por objeto: Modulo.attributes = { Velocidade = 90 } (anexar o script cria os
-- atributos que faltam). Memória do script: Modulo.state = { vida = 3 }, lida em self.state.

--- Um passo de `schedule`: `{ atraso, Modulo.f, dado? }`.
export type Step = { any }

--- Valor de uma variável do jogo.
export type VarValue = boolean | number | string | vector

--- Um objeto da cena. Chega aos scripts como `self` e nos argumentos dos eventos.
export type Object = {
    --- Nome do objeto na cena (só leitura).
    name: string,
    --- Atributos do objeto, pelo nome: ler um que ele não tem dá nil; escrever pede um atributo que ele tem, com um valor do mesmo tipo.
    attr: { [string]: any },
    --- Memória deste script neste objeto, com os campos de `Modulo.state`. Só em `self`.
    state: any,
    --- Posição em metros, nos eixos do pai, como as Propriedades mostram. Escrever move o objeto.
    position: vector,
    --- Posição do pivô no mundo, em metros. Escrever move o objeto.
    world_position: vector,
    --- Diz se o objeto ainda existe na cena.
    exists: (self: Object) -> boolean,
    --- Diz se o objeto tem o atributo.
    has_attribute: (self: Object, name: string) -> boolean,
    --- Chama `fn(self, ...)` daqui a `seconds` segundos de jogo; `fn` é uma função do módulo, como `Modulo.fechar`. Devolve o número que `cancel` recebe.
    after: <X...>(self: Object, seconds: number, fn: (Object, X...) -> (), X...) -> number,
    --- Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.
    every: <X...>(self: Object, seconds: number, fn: (Object, X...) -> (), X...) -> number,
    --- Chama `fn(self, item)` para cada item: o primeiro no passo seguinte, cada outro `interval` segundos depois.
    sequence: <T>(self: Object, interval: number, fn: (Object, T) -> (), items: { T }) -> number,
    --- Passos com atrasos próprios, `{ {atraso, Modulo.f, dado?}, ... }`, cada atraso contado do passo anterior.
    schedule: (self: Object, steps: { Step }) -> number,
    --- Cancela o que falta de um temporizador deste objeto; diz se ainda faltava algo.
    cancel: (self: Object, id: number) -> boolean,
    --- Manda a mensagem `name` ao objeto: `on_message(self, name, value, from)` dos scripts dele a recebe.
    send: (self: Object, name: string, value: any?) -> (),
}

--- Objeto com atributos e memória tipados pelo script: `type Self = ScriptObject<typeof(Modulo.attributes), typeof(Modulo.state)>`.
export type ScriptObject<A, S> = {
    --- Nome do objeto na cena (só leitura).
    name: string,
    --- Atributos do objeto, pelo nome: ler um que ele não tem dá nil; escrever pede um atributo que ele tem, com um valor do mesmo tipo.
    attr: A,
    --- Memória deste script neste objeto, com os campos de `Modulo.state`. Só em `self`.
    state: S,
    --- Posição em metros, nos eixos do pai, como as Propriedades mostram. Escrever move o objeto.
    position: vector,
    --- Posição do pivô no mundo, em metros. Escrever move o objeto.
    world_position: vector,
    --- Diz se o objeto ainda existe na cena.
    exists: (self: ScriptObject<A, S>) -> boolean,
    --- Diz se o objeto tem o atributo.
    has_attribute: (self: ScriptObject<A, S>, name: string) -> boolean,
    --- Chama `fn(self, ...)` daqui a `seconds` segundos de jogo; `fn` é uma função do módulo, como `Modulo.fechar`. Devolve o número que `cancel` recebe.
    after: <X...>(self: ScriptObject<A, S>, seconds: number, fn: (ScriptObject<A, S>, X...) -> (), X...) -> number,
    --- Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.
    every: <X...>(self: ScriptObject<A, S>, seconds: number, fn: (ScriptObject<A, S>, X...) -> (), X...) -> number,
    --- Chama `fn(self, item)` para cada item: o primeiro no passo seguinte, cada outro `interval` segundos depois.
    sequence: <T>(self: ScriptObject<A, S>, interval: number, fn: (ScriptObject<A, S>, T) -> (), items: { T }) -> number,
    --- Passos com atrasos próprios, `{ {atraso, Modulo.f, dado?}, ... }`, cada atraso contado do passo anterior.
    schedule: (self: ScriptObject<A, S>, steps: { Step }) -> number,
    --- Cancela o que falta de um temporizador deste objeto; diz se ainda faltava algo.
    cancel: (self: ScriptObject<A, S>, id: number) -> boolean,
    --- Manda a mensagem `name` ao objeto: `on_message(self, name, value, from)` dos scripts dele a recebe.
    send: (self: ScriptObject<A, S>, name: string, value: any?) -> (),
}

export type AnyObject = Object | ScriptObject<any, any>

--- Opções de `audio.play`.
export type PlayOptions = {
    --- Volume de 0 a 1 (padrão 1).
    volume: number?,
    --- O objeto cujo jogador ouve o som (com vários jogadores, a partir da 0.8; hoje, o único jogador).
    audience: AnyObject?,
}

declare audio: {
    --- Toca uma vez o som `name`, pelo nome que ele tem na Biblioteca.
    play: (name: string, options: PlayOptions?) -> (),
}

declare vars: {
    --- Lê uma variável do jogo pelo nome; com `key`, o valor daquela chave. nil quando não há valor.
    get: (name: string, key: string?) -> any,
    --- Grava uma variável do jogo pelo nome, com um valor do tipo dela; com `key`, o valor daquela chave.
    set: (name: string, value: VarValue, key: string?) -> (),
}

--- Escreve um aviso no Console, com o nome do objeto e a linha do script.
declare function warn(...: any): ()
