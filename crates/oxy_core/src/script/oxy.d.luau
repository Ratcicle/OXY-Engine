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
