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

declare scene: {
    --- O primeiro objeto da cena com o nome, na ordem da cena, ou nil.
    --- O nome é o exato, com maiúsculas e espaços. Para apontar um objeto que pode ser renomeado, prefira um atributo Objeto (`param.object()`), escolhido nas Propriedades.
    find: (name: string) -> Object?,
    --- O primeiro objeto da cena com o nome; um erro se não houver.
    --- Como `scene.find`, para um objeto que tem de existir: o erro diz o nome mais parecido da cena.
    get: (name: string) -> Object,
    --- Todos os objetos da cena com o nome, na ordem da cena.
    --- Uma lista nova a cada chamada, vazia quando nenhum tem o nome.
    find_all: (name: string) -> { Object },
    --- Todos os objetos da cena que têm o atributo, na ordem da cena.
    --- Um atributo serve de marca: os inimigos de uma onda, os interruptores de uma porta.
    with_attribute: (name: string) -> { Object },
    --- Põe uma cópia de um modelo da Biblioteca na cena e devolve a raiz dela.
    --- O nome do modelo vale sem diferenciar maiúsculas. A cópia, com os nomes que os objetos têm no modelo, vai para a raiz da cena (ou para `parent`), na posição salva no modelo (ou em `position`, nos eixos do pai); o `on_start` dos scripts dela roda depois desta chamada. Os IDs saem deste objeto, deste script e de quantas cópias ele já fez, como em `obj:clone`.
    spawn_model: (model: string, options: SpawnOptions?) -> Object,
    --- O nome da cena que está rodando.
    --- O mesmo nome que `scene.change` recebe.
    current: () -> string,
    --- Abre outra cena quando a função termina.
    --- O nome vale sem diferenciar maiúsculas. O resto da função ainda roda na cena atual; depois ela fecha, com os temporizadores e as mensagens dela, e a nova começa, como no nó Mudar de cena. Chamar de novo na mesma função troca o destino. As variáveis do jogo continuam.
    change: (name: string) -> (),
    --- Começa a cena atual de novo quando a função termina.
    --- Como `scene.change(scene.current())`. As variáveis do jogo guardam o valor: zere no `on_start` o que precisa recomeçar.
    restart: () -> (),
}

declare vmath: {
    --- O vetor entre `a` e `b` na fração `t`: a + (b - a) * t.
    --- O mesmo que o `vector.lerp` do Luau; `math.lerp` faz o mesmo com números.
    lerp: (a: vector, b: vector, t: number) -> vector,
    --- A distância entre dois pontos, em metros.
    --- O comprimento de `b - a`, em precisão dupla.
    distance: (a: vector, b: vector) -> number,
    --- O vetor com comprimento 1, ou zero quando ele não tem direção.
    --- Como o A normalizado (zero seguro) do nó Operações com Vetor3: um vetor zero dá zero, sem NaN (o `vector.normalize` do Luau dá NaN).
    normalize_or_zero: (v: vector) -> vector,
    --- O vetor no chão: o mesmo com y = 0.
    --- Para direções e distâncias no plano, sem a altura.
    flat: (v: vector) -> vector,
    --- O giro horizontal, em graus, que aponta a frente de um objeto para a direção.
    --- O mesmo giro de `obj.yaw`: 0 olha para -Z e 90 para -X; a altura não conta, e uma direção vertical ou zero dá 0. `obj.rotation = vector.create(0, vmath.yaw_of(d), 0)` vira o objeto para `d`.
    yaw_of: (direction: vector) -> number,
}

declare quat: {
    --- O quatérnio sem giro.
    --- Um quatérnio nunca muda; feito no topo do script, é uma constante.
    identity: () -> Quat,
    --- O giro dos ângulos X, Y e Z em graus, na ordem das Propriedades.
    --- `quat.from_euler(0, 90, 0)` é o mesmo giro que `rotation = vector.create(0, 90, 0)`.
    from_euler: (x: number, y: number, z: number) -> Quat,
    --- O giro de `degrees` graus em volta de um eixo.
    --- Visto da ponta do eixo, um ângulo positivo gira no sentido anti-horário.
    from_axis_angle: (axis: vector, degrees: number) -> Quat,
    --- O giro que aponta a frente de um objeto (-Z) para uma direção.
    --- O topo fica o mais perto possível de `up`.
    look_at: (forward: vector, up: vector?) -> Quat,
    --- O giro entre `a` e `b` na fração `t`, pelo caminho mais curto.
    --- Para girar aos poucos: chame a cada passo com `t` crescendo.
    slerp: (a: Quat, b: Quat, t: number) -> Quat,
    --- O giro `b` seguido do giro `a`.
    --- O operador `a * b` faz o mesmo, mas os tipos do VS Code não o conhecem: com `--!strict`, use a função.
    multiply: (a: Quat, b: Quat) -> Quat,
    --- O vetor `v` girado por `q`.
    --- O operador `q * v` faz o mesmo, mas os tipos do VS Code não o conhecem: com `--!strict`, use a função.
    rotate: (q: Quat, v: vector) -> vector,
    --- O giro contrário de `q`.
    --- `quat.multiply(quat.inverse(q), q)` não gira nada.
    inverse: (q: Quat) -> Quat,
    --- Os ângulos X, Y e Z em graus do giro, na ordem das Propriedades.
    --- O contrário de `quat.from_euler`. É o que a memória, os temporizadores e as mensagens guardam de um giro: eles não guardam quatérnios.
    to_euler: (q: Quat) -> vector,
}

declare param: {
    --- Declara um atributo Objeto, vazio até alguém escolher o objeto.
    --- Em `Modulo.attributes = { Alvo = param.object() }`: anexar cria o atributo vazio, e o objeto se escolhe nas Propriedades (ou com `set_attribute` pelo MCP), o que sobrevive a renomear o objeto. Lido no script, é o objeto ou nil. Em `Modulo.state`, declara um campo de objeto que começa vazio.
    object: () -> Object?,
    --- Declara um atributo Superfície, com a superfície do projeto de nome `name`.
    --- Em `Modulo.attributes = { Piso = param.surface("Gelo") }`: anexar procura a superfície pelo nome (sem diferenciar maiúsculas) e avisa, sem criar nada, quando o projeto não a tem. Lido no script, é a superfície (com `.name`) ou nil para a padrão. Em `Modulo.state`, só `param.surface()`: um campo que começa vazio.
    surface: (name: string?) -> Surface?,
    --- Declara um atributo Vetor2 com o valor padrão (x, y).
    --- Em `Modulo.attributes = { Tamanho = param.vector2(1, 1) }`, e num campo de `Modulo.state`. Lido no script, um Vetor2 é um vetor com z = 0; escrito, recebe o x e o y de um vetor.
    vector2: (x: number, y: number) -> vector,
}

declare physics: {
    --- Lança um raio e devolve o primeiro contato, ou nil.
    --- Como Consultar raio. Sem contato, devolve nil; uma consulta recusada (cena 2D, mais de 256 consultas no passo, direção zero) devolve nil e o motivo. Um raio que sai de dentro de um corpo o atinge na distância 0: ponha o objeto em `exclude`. Não move nada.
    raycast: (origin: vector, direction: vector, distance: number, options: QueryOptions?) -> (Hit?, string?),
    --- Varre uma esfera e devolve o primeiro contato, ou nil.
    --- Como Varrer esfera, com as regras de `raycast`: uma esfera que já toca algo na partida o atinge na distância 0.
    sphere_cast: (origin: vector, radius: number, direction: vector, distance: number, options: QueryOptions?) -> (Hit?, string?),
    --- Varre uma cápsula em pé e devolve o primeiro contato, ou nil.
    --- Como Varrer cápsula, com as regras de `raycast`: serve para saber se um corpo do tamanho de um personagem passa por um vão.
    capsule_cast: (origin: vector, radius: number, height: number, direction: vector, distance: number, options: QueryOptions?) -> (Hit?, string?),
    --- Diz se uma cápsula em pé cabe num lugar, sem mover nada.
    --- Como Testar espaço livre para cápsula: true quando ela não toca nada; senão, false e o contato (a distância é a penetração, negativa). Uma consulta recusada dá false, nil e o motivo.
    capsule_fits: (position: vector, radius: number, height: number, options: QueryOptions?) -> (boolean, Hit?, string?),
}

declare camera: {
    --- A câmera de jogo ativa, ou nil.
    --- A que `camera.activate` escolheu, ou a primeira marcada como ativa na ordem da cena.
    active: () -> Object?,
    --- O olhar da câmera ativa em graus: x é o giro (0 olha para -Z) e y a inclinação.
    --- O da simulação, sem a suavização da imagem; z é 0. Numa câmera Fixa, vem da rotação do objeto câmera.
    look: () -> vector,
    --- A direção do olhar da câmera ativa no mundo, de comprimento 1.
    --- A da simulação, como `camera.look`. Para mirar pelo personagem, que vale igual em toda máquina, use `obj.character.aim`.
    forward: () -> vector,
    --- O modo da câmera ativa: "first_person", "third_person" ou "fixed"; nil sem câmera ativa.
    --- Uma câmera sem Controle de câmera é sempre "fixed".
    mode: () -> string?,
    --- Torna uma câmera a câmera de jogo ativa, com uma transição.
    --- Como Ativar câmera de jogo: a câmera mantém o modo e quem acompanha.
    activate: (camera: AnyObject, seconds: number?) -> (),
    --- Troca o modo de uma câmera com Controle de câmera.
    --- Como Trocar modo da câmera: `"fixed"` para a câmera onde a vista está (mover o objeto câmera depois move a vista), e `"fixed_object"` leva a vista ao objeto câmera como ele está.
    set_mode: (camera: AnyObject, mode: "first_person" | "third_person" | "fixed" | "fixed_object", seconds: number?) -> (),
    --- Troca o objeto que uma câmera com Controle de câmera acompanha.
    --- Como Definir personagem acompanhado, normalmente um personagem 3D; a câmera segue no modo em que está. Vale só durante a partida.
    follow: (camera: AnyObject, target: AnyObject) -> (),
    --- Vira uma câmera para um ponto ou um objeto e a mantém assim, até `release_look`.
    --- Como Olhar assistido: o mouse não gira a câmera até `camera.release_look`. A câmera precisa estar ativa.
    look_at: (camera: AnyObject, target: vector | AnyObject) -> (),
    --- Devolve o olhar de uma câmera ao jogador, da vista de agora.
    --- Como Liberar olhar ao jogador, do nó Olhar assistido / liberar olhar.
    release_look: (camera: AnyObject) -> (),
    --- Ajusta o enquadramento de uma câmera: campo de visão, distância ou ombro.
    --- Como Ajustar enquadramento da câmera: a distância e o ombro pedem Controle de câmera, e o campo de visão vale para qualquer câmera. Diante de uma parede, a câmera chega mais perto que a distância pedida.
    set: (camera: AnyObject, setting: "fov" | "distance" | "shoulder", value: number, seconds: number?) -> (),
}

declare audio: {
    --- Toca uma vez o som `name`, pelo nome que ele tem na Biblioteca.
    --- O nome vale também sem diferenciar maiúsculas nem os espaços das pontas. `audience` é aceito desde já, mas só escolhe quem ouve a partir da 0.8, com vários jogadores: hoje o único jogador ouve todo som.
    play: (name: string, options: PlayOptions?) -> (),
    --- Toca um som em loop, na música ou nos efeitos.
    --- Como Tocar em loop: na música (o padrão), o mesmo som continua ao trocar ou reiniciar a cena, e outro entra com a transição enquanto o anterior sai; nos efeitos, o loop acaba com a cena (até 32, um por som). Pedir um loop que já toca só muda o volume dele. A pausa do jogo congela os efeitos e deixa a música.
    loop: (name: string, options: LoopOptions?) -> (),
    --- Para os loops de um som, ou todos os de um canal.
    --- Como Parar som: sem `sound`, para todos os loops do canal; o canal padrão é "all", a música e os efeitos. Parar um som que não toca não faz nada, e os sons de `audio.play` não param: acabam sozinhos.
    stop: (options: StopOptions?) -> (),
    --- Muda o volume geral, da música ou dos efeitos, de 0 a 1.
    --- Como Volume do canal: vale para a partida toda, também depois de trocar a cena, e fora de 0 a 1 fica no limite. Cada som toca no volume geral vezes o do canal vezes o dele; para lembrar a escolha, guarde-a numa variável salva entre partidas.
    set_volume: (channel: "master" | "music" | "effects", volume: number) -> (),
    --- O volume de agora de um canal, de 0 a 1.
    --- Como Ler volumes: começa em 1 em cada partida e muda só com `audio.set_volume`.
    volume: (channel: "master" | "music" | "effects") -> number,
}

declare game: {
    --- Pausa o jogo: o mundo para; a interface, a entrada e a lógica seguem.
    --- Como Pausar / retomar jogo (pausar): `on_pause` roda nos objetos que o ouvem, e `on_step`, os temporizadores, os personagens e as animações param até `game.resume`; cliques, ações de entrada e mensagens seguem chegando. Pausar um jogo já pausado não faz nada.
    pause: () -> (),
    --- Retoma o jogo pausado.
    --- Como Pausar / retomar jogo (retomar): `on_pause` roda de novo, com `paused` false. O que a lógica pediu aos personagens durante a pausa cai.
    resume: () -> (),
    --- Se o jogo está pausado.
    --- Como Ler estado do jogo. Alternar a pausa, como o modo Alternar do nó, é `if game.paused() then game.resume() else game.pause() end`.
    paused: () -> boolean,
    --- Mostra o cursor do mouse e o solta da câmera.
    --- Como Mostrar / esconder cursor (mostrar): o mouse fica livre e visível, para clicar na interface, e o olhar com o mouse para. Uma cena começa com ele escondido.
    show_cursor: () -> (),
    --- Esconde o cursor do mouse e o devolve à câmera.
    --- Como Mostrar / esconder cursor (esconder).
    hide_cursor: () -> (),
    --- Sai do jogo: o jogo exportado fecha e o teste do editor para.
    --- Como Sair do jogo; uma simulação termina. O resto da função ainda roda.
    quit: () -> (),
}

declare vars: {
    --- Lê uma variável do jogo pelo nome.
    --- O nome vale sem diferenciar maiúsculas. Com `key`, o valor daquela chave; numa chave nunca gravada, o valor inicial da variável, como o nó Ler variável do jogo, e nunca nil. Um Vetor2 lê como vetor com z = 0. As variáveis guardam o valor quando a cena reinicia: zere no `on_start` o que precisa recomeçar.
    get: (name: string, key: string?) -> any,
    --- Grava uma variável do jogo pelo nome, com um valor do tipo dela.
    --- Com `key`, grava o valor daquela chave, até 1.024 chaves por variável. Uma variável Vetor2 recebe o x e o y de um vetor.
    set: (name: string, value: VarValue, key: string?) -> (),
}

declare save: {
    --- Salva o progresso: os valores de agora das variáveis salvas entre partidas.
    --- Como Salvar progresso, com os valores das chaves: a próxima partida começa com eles. As variáveis que não são salvas entre partidas (Lógica > Variáveis do jogo) ficam de fora.
    write: () -> (),
    --- Apaga o progresso salvo e volta as variáveis salvas ao valor inicial.
    --- Como Apagar progresso: as chaves delas também somem.
    clear: () -> (),
    --- Se há progresso salvo.
    --- Como Ler progresso salvo: o que a partida recebeu ao começar, depois do que `save.write` e `save.clear` fizeram.
    exists: () -> boolean,
}

declare input: {
    --- Se a ação de entrada foi apertada neste passo.
    --- Como Ler ação de entrada (Pressionada): verdadeiro só no passo em que a tecla desce. Lê sem consumir: outros scripts, o grafo e o personagem veem o mesmo. O nome vale sem diferenciar maiúsculas; um segundo argumento, o jogador, chega na 0.8 e hoje é um erro.
    pressed: (action: string) -> boolean,
    --- Se a ação de entrada está segura neste passo.
    --- Como Ler ação de entrada (Mantida), com as regras de `input.pressed`.
    held: (action: string) -> boolean,
    --- Se a ação de entrada foi solta neste passo.
    --- Como Ler ação de entrada (Solta): verdadeiro só no passo em que a tecla sobe, com as regras de `input.pressed`.
    released: (action: string) -> boolean,
    --- O eixo de movimento deste passo: x para a direita e y para a frente, de comprimento até 1.
    --- Como Ler eixos de entrada (Movimento), com z 0: as teclas de movimento do Personagem 3D do objeto que roda o script, ou as do projeto (Mover à esquerda, à direita, para frente e para trás) num objeto sem ele. É o que `character:move` recebe.
    movement: () -> vector,
    --- Quanto o mouse andou neste passo: x para a direita e y para baixo.
    --- Como Ler eixos de entrada (Olhar), com z 0: em unidades do dispositivo, não em graus. Com o cursor à mostra ou o jogo pausado, o olhar fica parado.
    look: () -> vector,
    --- Quanto a roda do mouse girou neste passo, em linhas: positivo para cima.
    --- Como Ler eixos de entrada (Roda).
    wheel: () -> number,
}

declare time: {
    --- O tempo de jogo, em segundos desde o começo da partida.
    --- Como Ler tempo da simulação: para na pausa do jogo e atravessa o reinício da cena. Guarde momentos (`self.state.pronto_em = time.now() + 0.5`) em vez de somar `dt` a cada passo.
    now: () -> number,
    --- A duração de um passo de jogo, 1/60 s.
    --- Um número, não uma função: o mesmo `dt` que `on_step` recebe, como a Duração do passo de Ler tempo da simulação.
    dt: number,
    --- Escreve segundos como tempo: `time.format(75.5, "mm:ss.cc")` dá "01:15.50".
    --- Como Formatar tempo: corta (não arredonda) no último dígito do formato, e os minutos passam de 59 sem `h:mm:ss`. Não mexe no mundo: o topo do script pode usá-la.
    format: (seconds: number, format: "mm:ss" | "mm:ss.cc" | "m:ss.cc" | "ss.cc" | "mm:ss.mmm" | "h:mm:ss") -> string,
}

declare random: {
    --- Sorteia um número real de min até max, sem max.
    --- Como Sortear número (Real), da sequência do objeto e do script: outro script ou outro objeto sorteando a mais não muda estes números, e a mesma semente dá os mesmos em toda máquina.
    number: (min: number, max: number) -> number,
    --- Sorteia um inteiro de min até max, os dois incluídos.
    --- Como Sortear número (Inteiro), com as regras de `random.number`: `random.integer(1, 6)` é um dado.
    integer: (min: number, max: number) -> number,
    --- Troca a semente dos sorteios: todas as sequências recomeçam.
    --- Como Definir semente aleatória: vale para o grafo e para todos os scripts, e a mesma semente repete os mesmos números.
    seed: (seed: number) -> (),
    --- Uma sequência de sorteios com nome, independente da do script.
    --- Os sorteios dela não mudam os de `random.number` nem os de outra sequência: um sorteio a mais nos inimigos não muda as cartas. A sequência é do objeto e do script que sorteiam. Não mexe no mundo: o topo do script pode guardá-la num local.
    stream: (name: string) -> RandomStream,
}

--- Escreve uma linha no Console, com o nome do objeto e a linha do script.
--- Objetos saem pelo nome e tabelas com o primeiro nível. O `oxy_simulate` e o `oxy_console` trazem as linhas com o arquivo e a linha.
declare function print(...: any): ()

--- Escreve um aviso no Console, com o nome do objeto e a linha do script.
--- Como `print`, com "aviso:" antes do texto.
declare function warn(...: any): ()
