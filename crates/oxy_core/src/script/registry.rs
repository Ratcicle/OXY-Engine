//! The single registry of the scripts' API (design §5.7): one row for each name a script reaches
//! or the engine reads from a module, with its signature, its help in Portuguese (the Guide, the
//! types' hover) and in English (agents), what makes it fail, an example that a test runs, the
//! domain it will have in 0.8 and the version it came in. The host installs from it the global
//! tables, the base functions and the objects' methods, and the handles resolve the objects'
//! properties through it, so nothing a script reaches exists outside it; `generate` writes from
//! it the types for luau-lsp, the plan-B module, the Guide's pages and docs/api-scripts.md.
//!
//! Types in the rows are Luau's, with three marks the generator replaces: `@self` is the object
//! (`Object`, or `ScriptObject<A, S>` in that type), `@attr` its attributes and `@state` its
//! memory, which `ScriptObject` types from the script's declarations.
use super::Callback;
use super::host::Shared;
use super::{api, handles, math, messages, objects, sandbox, timers};
use mlua::{Function, Lua};
use std::rc::Rc;
use std::sync::LazyLock;

/// Makes a function of the API for one VM.
pub(crate) type Make = fn(&Lua, &Rc<Shared>) -> mlua::Result<Function>;

/// How a row reaches the scripts.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Imp {
    /// A function of a global table or of the base.
    Function(Make),
    /// A method of objects written with mlua, kept in the objects' table of methods.
    Method(Make),
    /// A method of objects that runs in C, without a call of its own.
    FastMethod(handles::FastMethod),
    /// A property of objects, which the handles' `__index` and `__newindex` resolve.
    Property(handles::Property),
    /// A function of the module the engine calls.
    Callback(Callback),
    /// A field of the module the engine reads when it loads it.
    Declaration,
}

/// What a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiKind {
    /// A function of a global table (`audio.play`) or of the base (`print`).
    Function,
    /// A method of objects, called with `:` (`obj:exists()`).
    Method,
    /// A property of objects; `write` when a script may assign it.
    Property { write: bool },
    /// A function of the module, with a fixed name, that the engine calls (`on_start`).
    Callback,
    /// A field of the module the engine reads when it loads it (`Modulo.attributes`).
    Declaration,
}

/// The areas of the API (design §5.4), one page of the Guide each, in the Guide's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Events,
    Object,
    Scene,
    Transform,
    Math,
    Attributes,
    Memory,
    Timers,
    Audio,
    Variables,
    Diagnostics,
    Modules,
}

impl Area {
    pub const ALL: [Self; 12] = [
        Self::Events,
        Self::Object,
        Self::Scene,
        Self::Transform,
        Self::Math,
        Self::Attributes,
        Self::Memory,
        Self::Timers,
        Self::Audio,
        Self::Variables,
        Self::Diagnostics,
        Self::Modules,
    ];

    /// The page's title in the Guide.
    pub fn title(self) -> &'static str {
        match self {
            Self::Events => "Eventos",
            Self::Object => "Objeto",
            Self::Scene => "Cena",
            Self::Transform => "Transformação",
            Self::Math => "Matemática",
            Self::Attributes => "Atributos",
            Self::Memory => "Memória do script",
            Self::Timers => "Temporizadores",
            Self::Audio => "Áudio",
            Self::Variables => "Variáveis do jogo",
            Self::Diagnostics => "Diagnóstico",
            Self::Modules => "Módulos",
        }
    }

    /// What the page is about, before its rows.
    pub fn intro(self) -> &'static str {
        match self {
            Self::Events => {
                "Funções do módulo com nomes fixos, que a engine chama sozinha; `self` é o objeto \
                 que anexa o script. Rodam na ordem da cena, depois da lógica de nós do objeto e \
                 na ordem da lista de scripts dele. Uma função com nome de evento que não existe \
                 (`on_aera_enter`) nunca roda, e a conferência avisa."
            }
            Self::Object => {
                "Os objetos chegam aos scripts como valores estáveis: `self`, o `other` dos \
                 eventos, os atributos Objeto, o remetente das mensagens e o que `scene` acha. \
                 Um objeto removido continua sendo um valor no script, mas ler ou escrever nele \
                 é um erro (`obj:exists()` diz se ele ainda está na cena)."
            }
            Self::Scene => {
                "A cena que está rodando: achar objetos pelo nome ou por um atributo, pôr modelos \
                 da Biblioteca nela e trocar de cena. As listas vêm na ordem da cena."
            }
            Self::Transform => {
                "Onde o objeto está e como está girado, em metros e graus, como vetores do Luau \
                 (`vector.create(x, y, z)`) e quatérnios (`quat`). Escrever passa pelas mesmas \
                 regras do nó Mover / girar / escalar objeto."
            }
            Self::Math => {
                "O que a OXY soma à biblioteca do Luau (`vector`, `math`): `vmath`, para vetores, \
                 e `quat`, para giros. Um quatérnio nunca muda: toda função devolve um novo."
            }
            Self::Attributes => {
                "Os atributos do objeto, os mesmos das Propriedades: a ponte com a interface, o \
                 grafo e os outros objetos. O script declara os que espera em \
                 `Modulo.attributes`, e anexá-lo cria no objeto os que faltam."
            }
            Self::Memory => {
                "O que o script guarda em cada objeto e só ele lê, declarado em `Modulo.state`. \
                 A engine guarda a memória: ela atravessa a recarga do script. O que outro \
                 objeto, a interface ou o grafo leem vai num atributo."
            }
            Self::Timers => {
                "Esperar sem parar a função: a engine guarda o nome da função do módulo e os \
                 dados, e a chama no tempo de jogo certo, que para com o jogo pausado. Só no \
                 próprio objeto; caem quando o objeto sai, quando a cena troca e quando o script \
                 para no objeto."
            }
            Self::Audio => "Sons da Biblioteca, pelo nome que eles têm lá.",
            Self::Variables => {
                "As variáveis do jogo do projeto (Lógica > Variáveis do jogo), pelo nome. Elas \
                 guardam o valor quando a cena reinicia."
            }
            Self::Diagnostics => {
                "Linhas no Console com o nome do objeto e a linha do script. `error(texto)` e \
                 `assert` do Luau param a chamada com o texto, também na linha."
            }
            Self::Modules => {
                "Funções e dados que vários scripts usam, em outros arquivos `.luau` da pasta \
                 `scripts`: tabelas de cartas, de fases, de inimigos. Um módulo só guarda \
                 funções, constantes e dados; o que muda no jogo fica nos atributos e na memória."
            }
        }
    }
}

/// Where a row will run in 0.8, with several players (api.md §4): only the gate of 0.8 reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    /// M: on the authority, predicted on the clients.
    Match,
    /// L: only on the client; in a flow of the match, once per predicted tick.
    Local,
    /// A: only on the authority.
    Authority,
    /// T: the domain of the object it acts on.
    Target,
    /// J: the data of one player.
    Player,
    /// The domain of the object whose script it is.
    Owner,
    /// The variable's scope decides: of the match (A/M) or of the client (L).
    Scope,
    /// The rules of messages: from local to the match a player's request, from the match to
    /// local a confirmed notice.
    Message,
    /// A pure read.
    Pure,
}

impl Domain {
    /// The domain as the Guide writes it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Match => "M (partida)",
            Self::Local => "L (local)",
            Self::Authority => "A (autoridade)",
            Self::Target => "T (do objeto alvo)",
            Self::Player => "J (do jogador)",
            Self::Owner => "do objeto dono",
            Self::Scope => "do escopo da variável",
            Self::Message => "regras das mensagens",
            Self::Pure => "leitura pura",
        }
    }
}

/// A parameter: its name, its Luau type (`?` when it may be left out) and what it is, in
/// Portuguese. `...` is the rest of the arguments; a type ending in `...` is a generic pack.
#[derive(Clone, Copy, Debug)]
pub struct ApiParam {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
}

const fn param(name: &'static str, ty: &'static str, doc: &'static str) -> ApiParam {
    ApiParam { name, ty, doc }
}

/// One row of the API.
#[derive(Clone, Copy, Debug)]
pub struct ApiFn {
    /// As scripts write it: `audio.play` and `print` for functions, `Object.position` for
    /// properties, `Object:after` for methods, `on_start` for callbacks and `attributes` for what
    /// a module declares (`Modulo.attributes`).
    pub name: &'static str,
    pub area: Area,
    pub kind: ApiKind,
    /// The type parameters, as `<X...>`; empty without.
    pub generics: &'static str,
    /// The parameters, after `self` for methods and callbacks.
    pub params: &'static [ApiParam],
    /// The Luau type it returns, `()` for nothing; a property's or a declaration's type.
    pub returns: &'static str,
    /// One line in Portuguese: the first of the hover and the Guide's list.
    pub summary: &'static str,
    /// The rest of the help in Portuguese.
    pub doc: &'static str,
    /// One line in English, for agents.
    pub help_en: &'static str,
    /// What stops the call, in Portuguese.
    pub errors: &'static [&'static str],
    /// Luau that uses it, in a module named `M`: module-level code (a callback, a declaration)
    /// starts with `M.` or `function M.`; any other runs in `on_start` (see
    /// [`ApiFn::example_script`]).
    pub example: &'static str,
    pub domain: Domain,
    /// The engine's version that brought it.
    pub since: &'static str,
    /// The nodes that do the same, by operation ID.
    pub nodes: &'static [&'static str],
    pub(crate) imp: Imp,
}

impl ApiFn {
    /// The table or type it belongs to (`audio`, `Object`), or `Modulo` for callbacks and
    /// declarations; empty for a function of the base.
    pub fn holder(&self) -> &'static str {
        match self.kind {
            ApiKind::Callback | ApiKind::Declaration => "Modulo",
            _ => self
                .name
                .split_once(['.', ':'])
                .map_or("", |(holder, _)| holder),
        }
    }

    /// Its own name, without the holder (`play`, `position`, `on_start`).
    pub fn member(&self) -> &'static str {
        self.name
            .split_once(['.', ':'])
            .map_or(self.name, |(_, member)| member)
    }

    /// Whether objects have it (a property or a method).
    pub fn of_objects(&self) -> bool {
        matches!(self.kind, ApiKind::Method | ApiKind::Property { .. })
    }

    /// Whether the types and the plan-B module declare it: every row but those luau-lsp types
    /// itself.
    pub fn in_types(&self) -> bool {
        !TYPED_BY_LUAU.contains(&self.name)
    }

    /// Whether the example is module-level code rather than the body of `on_start`.
    pub fn example_at_top(&self) -> bool {
        self.example.starts_with("M.") || self.example.starts_with("function M.")
    }

    /// The example as a whole script, as the test of every row runs it and luau-lsp reads it:
    /// module-level code as it is, any other in `on_start`, with the attributes, the memory and
    /// the functions the examples use (`M.fechar(self)`, `M.mostrar(self, n)`).
    pub fn example_script(&self) -> String {
        if self.example_at_top() {
            return format!(
                "--!strict\nlocal M = {{}}\n\n{}\n\nreturn M\n",
                self.example
            );
        }
        let body: Vec<String> = self
            .example
            .lines()
            .map(|line| format!("    {line}"))
            .collect();
        format!(
            "--!strict\nlocal M = {{}}\nM.attributes = {{ Vida = 3, Aviso = \"\" }}\n\
             M.state = {{ contagem = 0 }}\n\n\
             function M.fechar(self: Object) end\n\
             function M.mostrar(self: Object, n: number) end\n\
             function M.on_message(self: Object, name: string, value: any, from: Object) end\n\n\
             function M.on_start(self: Object)\n{}\nend\n\nreturn M\n",
            body.join("\n")
        )
    }
}

/// A shared type of the API: its name, type parameters (`<A, S>`), help and definition.
#[derive(Clone, Copy, Debug)]
pub struct ApiType {
    pub name: &'static str,
    pub generics: &'static str,
    pub doc: &'static str,
    pub def: TypeDef,
}

#[derive(Clone, Copy, Debug)]
pub enum TypeDef {
    /// A type written as it is.
    Alias(&'static str),
    /// A table of documented fields.
    Table(&'static [ApiField]),
    /// The members of objects, the `Object.` and `Object:` rows; `typed` for `ScriptObject`,
    /// whose attributes and memory are its type parameters `A` and `S`, sealed so that a name
    /// the script does not declare is an error.
    Object { typed: bool },
}

/// A field of a table type.
#[derive(Clone, Copy, Debug)]
pub struct ApiField {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
}

/// The shared types, in the order the definitions declare them: `AnyObject` after
/// `ScriptObject`, which the old solver needs (S2 §8).
pub static TYPES: &[ApiType] = &[
    ApiType {
        name: "Step",
        generics: "",
        doc: "Um passo de `schedule`: `{ atraso, Modulo.f, dado? }`.",
        def: TypeDef::Alias("{ any }"),
    },
    ApiType {
        name: "VarValue",
        generics: "",
        doc: "Valor de uma variável do jogo.",
        def: TypeDef::Alias("boolean | number | string | vector"),
    },
    ApiType {
        name: "CharacterEvent",
        generics: "",
        doc: "Um evento do personagem 3D: `kind` é \"jump\", \"land\", \"leave\", \"surface\", \
              \"posture\" ou \"side\", e os outros campos trazem os valores do nó Evento de \
              personagem (`impact`, a velocidade do pouso, entre eles).",
        def: TypeDef::Alias("{ kind: string, [string]: any }"),
    },
    ApiType {
        name: "Surface",
        generics: "",
        doc: "Uma superfície física do projeto, como um atributo Superfície a aponta.",
        def: TypeDef::Table(&[ApiField {
            name: "name",
            ty: "string",
            doc: "O nome da superfície no projeto.",
        }]),
    },
    ApiType {
        name: "Quat",
        generics: "",
        doc: "Um giro (quatérnio), feito pelas funções de `quat`. Nunca muda.",
        def: TypeDef::Table(&[
            ApiField {
                name: "x",
                ty: "number",
                doc: "O componente x.",
            },
            ApiField {
                name: "y",
                ty: "number",
                doc: "O componente y.",
            },
            ApiField {
                name: "z",
                ty: "number",
                doc: "O componente z.",
            },
            ApiField {
                name: "w",
                ty: "number",
                doc: "O componente w.",
            },
        ]),
    },
    ApiType {
        name: "Object",
        generics: "",
        doc: "Um objeto da cena. Chega aos scripts como `self` e nos argumentos dos eventos.",
        def: TypeDef::Object { typed: false },
    },
    ApiType {
        name: "ScriptObject",
        generics: "<A, S>",
        doc: "Objeto com atributos e memória tipados pelo script: `type Self = \
              ScriptObject<typeof(Modulo.attributes), typeof(Modulo.state)>`. Um nome que o \
              script não declara é acusado.",
        def: TypeDef::Object { typed: true },
    },
    ApiType {
        name: "AnyObject",
        generics: "",
        doc: "Qualquer objeto, tipado pelo script ou não.",
        def: TypeDef::Alias("Object | ScriptObject<any, any>"),
    },
    ApiType {
        name: "PlayOptions",
        generics: "",
        doc: "Opções de `audio.play`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "volume",
                ty: "number?",
                doc: "Volume de 0 a 1 (padrão 1).",
            },
            ApiField {
                name: "audience",
                ty: "AnyObject?",
                doc: "O objeto cujo jogador ouve o som, com vários jogadores, a partir da 0.8. \
                      Hoje não muda nada: o único jogador ouve todo som; para tocar só para ele, \
                      confira um atributo do personagem (como `Jogador local`).",
            },
        ]),
    },
    ApiType {
        name: "SpawnOptions",
        generics: "",
        doc: "Opções de `obj:clone` e `scene.spawn_model`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "offset",
                ty: "vector?",
                doc: "Só em `clone`: quanto a cópia fica longe do original, nos eixos do pai \
                      (sem ele, no mesmo lugar).",
            },
            ApiField {
                name: "position",
                ty: "vector?",
                doc: "Onde a cópia fica, nos eixos do pai, como `obj.position`.",
            },
            ApiField {
                name: "rotation",
                ty: "vector?",
                doc: "A rotação da cópia em graus, como `obj.rotation`.",
            },
            // Object, not AnyObject: `clone` is a member of ScriptObject, and a ScriptObject in
            // its own parameters is "Recursive type being used with different parameters" in
            // the old solver (S2 §8).
            ApiField {
                name: "parent",
                ty: "Object?",
                doc: "O pai da cópia; sem ele, o do original (`clone`) ou a raiz da cena.",
            },
            ApiField {
                name: "attributes",
                ty: "{ [string]: any }?",
                doc: "Valores de atributos que a cópia já tem, antes do `on_start` dela.",
            },
        ]),
    },
];

const V07: &str = "0.7.0";

/// Errors of timers shared by `after`, `every`, `sequence` and `schedule`.
const TIMER_ERRORS: &[&str] = &[
    "chamado em outro objeto que não `self`",
    "tempo que não é um número de 0 a 86400 segundos",
    "função criada na hora em vez de uma da tabela do módulo (`M.fechar`)",
    "dado que não é nil, número, texto (até 4.096 bytes), booleano, vetor, objeto nem superfície, \
     ou mais de 16 dados",
    "a fila sem lugar para mais temporizadores: o script para no objeto",
];

/// The API, area by area.
pub static API: &[ApiFn] = &[
    // Events.
    ApiFn {
        name: "on_start",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[],
        returns: "()",
        summary: "A cena começou, ou uma cópia com o script surgiu.",
        doc: "Roda uma vez em cada objeto que anexa o script: no começo da cena, na ordem da \
              cena, depois que todos os objetos dela existem; numa cópia criada durante o jogo, \
              quando ela surge. As variáveis do jogo guardam o valor quando a cena reinicia: \
              zere aqui o que precisa recomeçar.",
        help_en: "Runs once per object when the scene starts (in scene order, once every object \
                  exists) or when a copy with the script appears.",
        errors: &[],
        example: "function M.on_start(self: Object)\n    self.attr.Vida = 3\nend",
        domain: Domain::Owner,
        since: V07,
        nodes: &["event.scene_start"],
        imp: Imp::Callback(Callback::Start),
    },
    ApiFn {
        name: "on_step",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[param("dt", "number", "a duração do passo, 1/60 s")],
        returns: "()",
        summary: "A cada passo de jogo, 60 vezes por segundo.",
        doc: "Não roda com o jogo pausado. Para esperar, use um temporizador (`self:after`) em \
              vez de somar `dt` todo passo.",
        help_en: "Runs every fixed step (1/60 s), not while the game is paused.",
        errors: &[],
        example: "function M.on_step(self: Object, dt: number)\n    self.position += \
                  vector.create(0, 0, 2 * dt)\nend",
        domain: Domain::Owner,
        since: V07,
        nodes: &["event.step"],
        imp: Imp::Callback(Callback::Step),
    },
    ApiFn {
        name: "on_input",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[
            param(
                "action",
                "string",
                "o nome da ação, como em Lógica > Ações de entrada",
            ),
            param(
                "phase",
                "\"pressed\" | \"released\"",
                "\"pressed\" ao apertar, \"released\" ao soltar",
            ),
        ],
        returns: "()",
        summary: "Uma ação de entrada foi apertada ou solta.",
        doc: "Chega a todos os objetos cujos scripts definem a função.",
        help_en: "An input action was pressed or released: its name as the person gave it and \
                  \"pressed\" or \"released\".",
        errors: &[],
        example: "function M.on_input(self: Object, action: string, phase: string)\n    if action \
                  == \"Pular\" and phase == \"pressed\" then\n        print(\"pulou\")\n    \
                  end\nend",
        domain: Domain::Player,
        since: V07,
        nodes: &["event.input"],
        imp: Imp::Callback(Callback::Input),
    },
    ApiFn {
        name: "on_click",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[],
        returns: "()",
        summary: "Um clique no objeto ou no botão de interface dele.",
        doc: "O objeto precisa ser clicável: um objeto visual ou um botão de interface.",
        help_en: "A click on the object or on its interface button.",
        errors: &[],
        example: "function M.on_click(self: Object)\n    audio.play(\"Bipe\")\nend",
        domain: Domain::Local,
        since: V07,
        nodes: &["event.click"],
        imp: Imp::Callback(Callback::Click),
    },
    ApiFn {
        name: "on_area_enter",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[param("other", "Object", "o objeto que entrou")],
        returns: "()",
        summary: "Outro objeto entrou na área do objeto.",
        doc: "A área é um colisor marcado como área (sensor), e o outro objeto precisa de um \
              colisor também. Qualquer objeto com colisor entra, como uma caixa de física: \
              confira um atributo dele antes de agir.",
        help_en: "Another object entered the object's area (a trigger collider).",
        errors: &[],
        example: "function M.on_area_enter(self: Object, other: Object)\n    if \
                  other:has_attribute(\"Vida\") then\n        other.attr.Vida -= 1\n    \
                  end\nend",
        domain: Domain::Match,
        since: V07,
        nodes: &["event.area_enter"],
        imp: Imp::Callback(Callback::AreaEnter),
    },
    ApiFn {
        name: "on_area_exit",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[param("other", "Object", "o objeto que saiu")],
        returns: "()",
        summary: "Outro objeto saiu da área do objeto.",
        doc: "Como `on_area_enter`, na saída.",
        help_en: "Another object left the object's area.",
        errors: &[],
        example: "function M.on_area_exit(self: Object, other: Object)\n    print(other.name, \
                  \"saiu\")\nend",
        domain: Domain::Match,
        since: V07,
        nodes: &["event.area_exit"],
        imp: Imp::Callback(Callback::AreaExit),
    },
    ApiFn {
        name: "on_marker",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[
            param("marker", "string", "o nome do marcador"),
            param("clip", "string", "o nome da animação"),
        ],
        returns: "()",
        summary: "Uma animação do objeto passou por um marcador.",
        doc: "Os nomes são os do Estúdio do modelo.",
        help_en: "An animation of the object passed a marker: the marker's and the clip's names.",
        errors: &[],
        example: "function M.on_marker(self: Object, marker: string, clip: string)\n    \
                  print(clip, marker)\nend",
        domain: Domain::Owner,
        since: V07,
        nodes: &["event.animation"],
        imp: Imp::Callback(Callback::Marker),
    },
    ApiFn {
        name: "on_character",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[param(
            "event",
            "CharacterEvent",
            "o que houve (`event.kind`) e os valores do evento",
        )],
        returns: "()",
        summary: "Um evento do personagem 3D do objeto: pulo, pouso e outros.",
        doc: "Só do personagem do próprio objeto: quem quer saber do personagem de outro objeto \
              recebe uma mensagem dele.",
        help_en: "An event of the object's own 3D character: event.kind is jump, land, leave, \
                  surface, posture or side, with the values of the Evento de personagem node.",
        errors: &[],
        example: "function M.on_character(self: Object, event: CharacterEvent)\n    if \
                  event.kind == \"land\" then\n        print(\"pousou\", event.impact)\n    \
                  end\nend",
        domain: Domain::Match,
        since: V07,
        nodes: &["event.character"],
        imp: Imp::Callback(Callback::Character),
    },
    ApiFn {
        name: "on_pause",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[param(
            "paused",
            "boolean",
            "true ao pausar, false ao retomar",
        )],
        returns: "()",
        summary: "O jogo entrou ou saiu da pausa dele.",
        doc: "Os temporizadores param com o jogo pausado e voltam a contar quando ele retoma.",
        help_en: "The game was paused (true) or resumed (false).",
        errors: &[],
        example: "function M.on_pause(self: Object, paused: boolean)\n    self.attr.Aviso = if \
                  paused then \"Pausado\" else \"\"\nend",
        domain: Domain::Local,
        since: V07,
        nodes: &["event.pause"],
        imp: Imp::Callback(Callback::Pause),
    },
    ApiFn {
        name: "on_message",
        area: Area::Events,
        kind: ApiKind::Callback,
        generics: "",
        params: &[
            param("name", "string", "o nome da mensagem"),
            param("value", "any", "uma cópia do valor mandado; nil sem valor"),
            param("from", "Object", "o objeto que mandou"),
        ],
        returns: "()",
        summary: "Uma mensagem que um objeto mandou com `obj:send`.",
        doc: "Chega na fase seguinte da lógica, depois dos eventos e temporizadores dela, na \
              ordem em que as mensagens foram mandadas. Mandar a um objeto sem `on_message` em \
              nenhum script gera um aviso, uma vez por linha que manda.",
        help_en: "A message another object (or the object itself) sent with obj:send: its name, \
                  a copy of its value and the sender.",
        errors: &[],
        example: "function M.on_message(self: Object, name: string, value: any, from: \
                  Object)\n    print(from.name, name, value)\nend",
        domain: Domain::Message,
        since: V07,
        nodes: &[],
        imp: Imp::Callback(Callback::Message),
    },
    // Objects.
    ApiFn {
        name: "Object.id",
        area: Area::Object,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "string",
        summary: "O ID do objeto: fixo no projeto e, numa cópia feita no jogo, derivado do \
                  original.",
        doc: "Só leitura. Dois objetos podem ter o mesmo nome, nunca o mesmo ID: serve de chave \
              para guardar algo por objeto. Para mostrar um objeto a quem joga, use `name`.",
        help_en: "The object's ID (read only): fixed in the project and, for a copy made in the \
                  game, derived from the original; a key, never shown to players.",
        errors: &["o objeto não existe mais"],
        example: "print(self.name, self.id)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Property(handles::Property::Id),
    },
    ApiFn {
        name: "Object.name",
        area: Area::Object,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "string",
        summary: "O nome do objeto na cena.",
        doc: "Só leitura. `print` e `tostring` mostram um objeto por esse nome. A raiz de uma \
              cópia de `obj:clone` leva \" (cópia)\" no fim do nome, como as do nó Criar cópia \
              de objeto, e o que está nela fica com os nomes do original; as cópias de \
              `scene.spawn_model` ficam com os nomes do modelo.",
        help_en: "The object's name in the scene (read only); the root of an obj:clone copy ends \
                  in \" (cópia)\", while scene.spawn_model copies keep the model's names.",
        errors: &["o objeto não existe mais"],
        example: "print(self.name)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Property(handles::Property::Name),
    },
    ApiFn {
        name: "Object.parent",
        area: Area::Object,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "Object?",
        summary: "O pai do objeto na Hierarquia, ou nil num objeto da raiz.",
        doc: "Só leitura. Um script compartilhado por vários objetos alcança o grupo de cada um \
              por aqui, sem uma referência fixa.",
        help_en: "The object's parent in the hierarchy, or nil at the root (read only).",
        errors: &["o objeto não existe mais"],
        example: "local pai = self.parent\nif pai then\n    print(pai.name)\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Property(handles::Property::Parent),
    },
    ApiFn {
        name: "Object:children",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[],
        returns: "{ Object }",
        summary: "Os filhos diretos do objeto, na ordem da cena.",
        doc: "Uma lista nova a cada chamada: guardá-la ou mudá-la não muda a cena.",
        help_en: "The object's direct children, in scene order, as a new list.",
        errors: &["o objeto não existe mais"],
        example: "for _, filho in self:children() do\n    print(filho.name)\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Method(objects::children),
    },
    ApiFn {
        name: "Object:find_child",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[
            param("name", "string", "o nome do filho"),
            param(
                "deep",
                "boolean?",
                "true procura em tudo o que está abaixo do objeto, não só nos filhos",
            ),
        ],
        returns: "Object?",
        summary: "O primeiro filho com o nome, na ordem da cena, ou nil.",
        doc: "Para quando a ausência é esperada; um filho fixo de um modelo se pega com \
              `get_child`, que dá um erro claro quando ele falta.",
        help_en: "The first child with that name in scene order (with deep, anything below), or \
                  nil.",
        errors: &["o nome não é um texto", "o objeto não existe mais"],
        example: "local alca = self:find_child(\"Maçaneta\")\nif alca then\n    alca.visible = \
                  false\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Method(objects::find_child),
    },
    ApiFn {
        name: "Object:get_child",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[
            param("name", "string", "o nome do filho"),
            param(
                "deep",
                "boolean?",
                "true procura em tudo o que está abaixo do objeto, não só nos filhos",
            ),
        ],
        returns: "Object",
        summary: "O primeiro filho com o nome, na ordem da cena; um erro se não houver.",
        doc: "Como `find_child`, mas o filho é certo: o erro diz o nome mais parecido e os \
              filhos que existem (um filho renomeado quebra o script na linha, e não depois).",
        help_en: "The first child with that name (with deep, anything below); an error naming \
                  the children there are when none has it.",
        errors: &[
            "nenhum filho com o nome (o erro lista os filhos)",
            "o nome não é um texto",
            "o objeto não existe mais",
        ],
        example: "self:get_child(\"Maçaneta\").visible = false",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Method(objects::get_child),
    },
    ApiFn {
        name: "Object:exists",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[],
        returns: "boolean",
        summary: "Diz se o objeto ainda está na cena.",
        doc: "Um objeto removido continua sendo um valor no script (num atributo, na memória ou \
              numa mensagem), mas ler ou escrever nele é um erro; confira antes com `exists`.",
        help_en: "Whether the object is still in the scene.",
        errors: &[],
        example: "if self:exists() then\n    print(\"ainda aqui\")\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::FastMethod(handles::FastMethod::Exists),
    },
    ApiFn {
        name: "Object.visible",
        area: Area::Object,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "boolean",
        summary: "Se o objeto aparece: false o esconde, true o mostra.",
        doc: "Como o nó Ativar / desativar componente (Visibilidade). Esconder um grupo esconde \
              o que está nele, mas o `visible` dos filhos continua true; um botão escondido (ou \
              num grupo escondido) não recebe cliques.",
        help_en: "Whether the object shows: false hides it, as action.component visible; a \
                  hidden group hides its children without changing theirs.",
        errors: &[
            "escrever outra coisa que true ou false",
            "o objeto não existe mais",
        ],
        example: "self.visible = false",
        domain: Domain::Target,
        since: V07,
        nodes: &["action.component"],
        imp: Imp::Property(handles::Property::Visible),
    },
    ApiFn {
        name: "Object.logic_enabled",
        area: Area::Object,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "boolean",
        summary: "Se a lógica de nós (o grafo) do objeto roda; os scripts dele seguem.",
        doc: "Como o nó Ativar / desativar componente (Comportamento visual): false para o grafo \
              e descarta as esperas dele; um erro no grafo também o põe em false. Os scripts têm \
              o próprio liga e desliga.",
        help_en: "Whether the object's node graph runs (action.component behavior); its scripts \
                  run either way.",
        errors: &[
            "escrever outra coisa que true ou false",
            "o objeto não existe mais",
        ],
        example: "self.logic_enabled = false",
        domain: Domain::Target,
        since: V07,
        nodes: &["action.component"],
        imp: Imp::Property(handles::Property::LogicEnabled),
    },
    ApiFn {
        name: "Object:has_component",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "kind",
            "\"animation\" | \"area\" | \"camera\" | \"character\" | \"collider\" | \
             \"controller\" | \"platform\" | \"ui\"",
            "o componente: animation (tem animações), area (colisor marcado como área), camera, \
             character (Personagem 3D), collider (colisor 2D ou 3D, área incluída), controller \
             (Movimento 2D), platform (plataforma móvel) ou ui (elemento de interface)",
        )],
        returns: "boolean",
        summary: "Diz se o objeto tem o componente, ligado ou não.",
        doc: "Confira antes de agir sobre outro objeto, como o que entrou numa área: um \
              personagem, uma caixa de física.",
        help_en: "Whether the object has the component (animation, area, camera, character, \
                  collider, controller, platform or ui), on or off.",
        errors: &[
            "um componente que não é um dos da lista (o erro lista os nomes)",
            "o objeto não existe mais",
        ],
        example: "if self:has_component(\"collider\") then\n    print(\"tem colisor\")\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Method(objects::has_component),
    },
    ApiFn {
        name: "Object:destroy",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[],
        returns: "()",
        summary: "Tira o objeto e tudo o que está nele da cena, na hora.",
        doc: "Como o nó Remover objeto. Os temporizadores, as mensagens e a memória dele vão \
              junto. `self:destroy()` encerra a chamada em silêncio no próximo uso de `self`; \
              o que vem antes vale.",
        help_en: "Removes the object and everything under it from the scene at once, as \
                  action.remove; self:destroy() ends the call quietly at the next use of self.",
        errors: &["o objeto não existe mais"],
        example: "self:destroy()",
        domain: Domain::Target,
        since: V07,
        nodes: &["action.remove"],
        imp: Imp::Method(objects::destroy),
    },
    ApiFn {
        name: "Object:clone",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "options",
            "SpawnOptions?",
            "`{ offset = v }` ou `{ position = v }`, e `rotation`, `parent` e `attributes`",
        )],
        returns: "Object",
        summary: "Copia o objeto e tudo o que está nele, como está agora, e devolve a cópia.",
        doc: "Como o nó Criar cópia de objeto: a cópia fica no mesmo pai e no mesmo lugar do \
              original (`offset` a desloca nos eixos do pai; `position` a põe num lugar), com os \
              atributos, os scripts e a lógica dele; a memória dos scripts nasce com os valores \
              declarados, e o `on_start` deles roda depois desta chamada. Os IDs da cópia saem \
              deste objeto, deste script e de quantas cópias ele já fez: a mesma partida dá os \
              mesmos IDs.",
        help_en: "Copies the object and its hierarchy as it is now (action.spawn) and returns \
                  the copy; options offset or position, rotation, parent, attributes. The \
                  copy's on_start runs after this call.",
        errors: &[
            "uma opção desconhecida, offset com position, ou um valor de outro tipo",
            "um atributo em `attributes` que a cópia não tem, ou de outro tipo (nada é criado)",
            "o objeto não existe mais",
        ],
        example: "if self.attr.Vida == 3 then\n    local copia = self:clone({ offset = \
                  vector.create(2, 0, 0) })\n    copia.attr.Vida = 1\nend",
        domain: Domain::Match,
        since: V07,
        nodes: &["action.spawn"],
        imp: Imp::Method(objects::clone),
    },
    ApiFn {
        name: "Object:first_hit",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[param("other", "Object", "o objeto atingido")],
        returns: "boolean",
        summary: "true só na primeira vez que `other` é atingido nesta ativação da área do \
                  objeto.",
        doc: "A regra \"Uma vez por ativação\" do nó Aplicar dano, sem o dano: a área conta quem \
              já atingiu até ser desligada e ligada de novo, e os acertos do grafo e dos \
              scripts somam juntos. Em `on_area_enter` e `on_area_exit` da própria área, conta \
              na ativação do evento, como o nó, mesmo que a área tenha desligado depois; fora \
              deles, na ativação de agora, e uma área desligada dá false. O dano vira uma \
              conta: `other.attr.Vida -= 25`.",
        help_en: "True only the first time other is hit in this activation of the object's area \
                  (action.damage's once rule, without the damage); in the area's own \
                  on_area_enter/on_area_exit it counts in that event's activation, and an area \
                  that is off gives false elsewhere.",
        errors: &[
            "o objeto não é uma área (um colisor marcado como área)",
            "other não é um objeto ou não existe mais",
        ],
        example: "function M.on_area_enter(self: Object, other: Object)\n    if \
                  other:has_attribute(\"Vida\") and self:first_hit(other) then\n        \
                  other.attr.Vida -= 1\n    end\nend",
        domain: Domain::Match,
        since: V07,
        nodes: &["action.damage"],
        imp: Imp::Method(objects::first_hit),
    },
    ApiFn {
        name: "Object:send",
        area: Area::Object,
        kind: ApiKind::Method,
        generics: "",
        params: &[
            param("name", "string", "o nome da mensagem, de 1 a 256 bytes"),
            param(
                "value",
                "any?",
                "o valor, copiado: nil, números, textos, booleanos, vetores, objetos e tabelas \
                 desses valores, até 4 KB",
            ),
        ],
        returns: "()",
        summary: "Manda uma mensagem ao objeto: `on_message` dos scripts dele a recebe.",
        doc: "A mensagem chega na fase seguinte da lógica, como `on_message(self, name, value, \
              from)`. Um script pode mandar até 1.024 mensagens de um objeto por passo, e os \
              scripts da cena, 4.096.",
        help_en: "Sends a message to the object: on_message(self, name, value, from) of its \
                  scripts gets it in the next logic phase, the value copied.",
        errors: &[
            "o alvo não é um objeto ou não existe mais",
            "nome vazio ou com mais de 256 bytes",
            "valor que uma mensagem não leva (função, tabela com metatabela, mais de 4 KB)",
            "mais de 1.024 mensagens do objeto no passo: o script para no objeto",
            "mais de 4.096 mensagens na cena no passo: só esta chamada falha",
        ],
        example: "self:send(\"abrir\", 2)",
        domain: Domain::Message,
        since: V07,
        nodes: &[],
        imp: Imp::Method(messages::send),
    },
    // Scene.
    ApiFn {
        name: "scene.find",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[param(
            "name",
            "string",
            "o nome do objeto, como na Hierarquia",
        )],
        returns: "Object?",
        summary: "O primeiro objeto da cena com o nome, na ordem da cena, ou nil.",
        doc: "O nome é o exato, com maiúsculas e espaços. Para apontar um objeto que pode ser \
              renomeado, prefira um atributo Objeto (`param.object()`), escolhido nas \
              Propriedades.",
        help_en: "The first object of the scene with that exact name, in scene order, or nil.",
        errors: &["o nome não é um texto"],
        example: "local porta = scene.find(\"Porta\")\nif porta then\n    print(porta.name)\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(objects::scene_find),
    },
    ApiFn {
        name: "scene.get",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[param(
            "name",
            "string",
            "o nome do objeto, como na Hierarquia",
        )],
        returns: "Object",
        summary: "O primeiro objeto da cena com o nome; um erro se não houver.",
        doc: "Como `scene.find`, para um objeto que tem de existir: o erro diz o nome mais \
              parecido da cena.",
        help_en: "The first object of the scene with that exact name; an error with the nearest \
                  name when none has it.",
        errors: &[
            "nenhum objeto com o nome (com o nome mais parecido)",
            "o nome não é um texto",
        ],
        example: "scene.get(\"Porta\").attr.Vida = 2",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(objects::scene_get),
    },
    ApiFn {
        name: "scene.find_all",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[param(
            "name",
            "string",
            "o nome dos objetos, como na Hierarquia",
        )],
        returns: "{ Object }",
        summary: "Todos os objetos da cena com o nome, na ordem da cena.",
        doc: "Uma lista nova a cada chamada, vazia quando nenhum tem o nome.",
        help_en: "Every object of the scene with that exact name, in scene order.",
        errors: &["o nome não é um texto"],
        example: "for _, porta in scene.find_all(\"Porta\") do\n    porta.attr.Vida = 3\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(objects::scene_find_all),
    },
    ApiFn {
        name: "scene.with_attribute",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("name", "string", "o nome do atributo")],
        returns: "{ Object }",
        summary: "Todos os objetos da cena que têm o atributo, na ordem da cena.",
        doc: "Um atributo serve de marca: os inimigos de uma onda, os interruptores de uma porta.",
        help_en: "Every object of the scene that has the attribute, in scene order.",
        errors: &["o nome não é um texto"],
        example: "for _, alvo in scene.with_attribute(\"Vida\") do\n    alvo.attr.Vida -= 1\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(objects::scene_with_attribute),
    },
    ApiFn {
        name: "scene.spawn_model",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("model", "string", "o nome do modelo na Biblioteca"),
            param(
                "options",
                "SpawnOptions?",
                "`{ position = v }`, e `rotation`, `parent` e `attributes`",
            ),
        ],
        returns: "Object",
        summary: "Põe uma cópia de um modelo da Biblioteca na cena e devolve a raiz dela.",
        doc: "O nome do modelo vale sem diferenciar maiúsculas. A cópia, com os nomes que os \
              objetos têm no modelo, vai para a raiz da cena (ou para `parent`), na posição \
              salva no modelo (ou em `position`, nos eixos do pai); o `on_start` dos scripts dela roda depois desta chamada. Os IDs saem deste \
              objeto, deste script e de quantas cópias ele já fez, como em `obj:clone`.",
        help_en: "Places a copy of a model of the project (by name, any case) in the scene and \
                  returns its root; options position, rotation, parent, attributes. Its \
                  on_start runs after this call.",
        errors: &[
            "um modelo que o projeto não tem (o erro lista os modelos)",
            "um modelo de formas 2D numa cena 3D, ou o contrário",
            "uma opção desconhecida ou um valor de outro tipo",
            "um atributo em `attributes` que a raiz da cópia não tem (nada é criado)",
        ],
        example: "local bala = scene.spawn_model(\"Bala\", { position = vector.create(0, 1, 0) \
                  })\nprint(bala.name)",
        domain: Domain::Match,
        since: V07,
        nodes: &[],
        imp: Imp::Function(objects::scene_spawn_model),
    },
    ApiFn {
        name: "scene.current",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[],
        returns: "string",
        summary: "O nome da cena que está rodando.",
        doc: "O mesmo nome que `scene.change` recebe.",
        help_en: "The name of the scene running.",
        errors: &[],
        example: "print(scene.current())",
        domain: Domain::Pure,
        since: V07,
        nodes: &["scene.current"],
        imp: Imp::Function(objects::scene_current),
    },
    ApiFn {
        name: "scene.change",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("name", "string", "o nome da cena, como no projeto")],
        returns: "()",
        summary: "Abre outra cena quando a função termina.",
        doc: "O nome vale sem diferenciar maiúsculas. O resto da função ainda roda na cena \
              atual; depois ela fecha, com os temporizadores e as mensagens dela, e a nova \
              começa, como no nó Mudar de cena. Chamar de novo na mesma função troca o destino. \
              As variáveis do jogo continuam.",
        help_en: "Opens another scene (by name, any case) when the function ends; the rest of \
                  the function runs in this one.",
        errors: &[
            "uma cena que o projeto não tem (com o nome mais parecido e a lista)",
            "duas cenas com o mesmo nome",
            "o nome não é um texto",
        ],
        example: "scene.change(\"Menu\")",
        domain: Domain::Authority,
        since: V07,
        nodes: &["action.scene", "value.scene"],
        imp: Imp::Function(objects::scene_change),
    },
    ApiFn {
        name: "scene.restart",
        area: Area::Scene,
        kind: ApiKind::Function,
        generics: "",
        params: &[],
        returns: "()",
        summary: "Começa a cena atual de novo quando a função termina.",
        doc: "Como `scene.change(scene.current())`. As variáveis do jogo guardam o valor: zere \
              no `on_start` o que precisa recomeçar.",
        help_en: "Starts the current scene again when the function ends; game variables keep \
                  their values.",
        errors: &[],
        example: "if vars.get(\"Pontos\") == 0 then\n    vars.set(\"Pontos\", 1)\n    \
                  scene.restart()\nend",
        domain: Domain::Authority,
        since: V07,
        nodes: &["action.scene", "scene.current"],
        imp: Imp::Function(objects::scene_restart),
    },
    // Transform.
    ApiFn {
        name: "Object.position",
        area: Area::Transform,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "vector",
        summary: "A posição do objeto em metros, nos eixos do pai, como as Propriedades mostram.",
        doc: "Escrever move o objeto, como o nó Mover / girar / escalar objeto, e recusa o que \
              ele recusa: um personagem 3D (mova-o pelo personagem) e um objeto que uma \
              animação tocando posiciona.",
        help_en: "The object's position in metres, in its parent's axes, as Properties shows \
                  it; writing moves it.",
        errors: &[
            "escrever outra coisa que um vetor",
            "mover um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "self.position = vector.create(0, 2, 0)",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.read", "transform.write"],
        imp: Imp::Property(handles::Property::Position),
    },
    ApiFn {
        name: "Object.world_position",
        area: Area::Transform,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "vector",
        summary: "A posição do pivô do objeto no mundo, em metros.",
        doc: "Num personagem 3D, a origem dele. Escrever move o objeto como `position`.",
        help_en: "The position of the object's pivot in the world, in metres; writing moves it.",
        errors: &[
            "escrever outra coisa que um vetor",
            "mover um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "print(self.world_position)",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.read", "transform.write"],
        imp: Imp::Property(handles::Property::WorldPosition),
    },
    ApiFn {
        name: "Object.rotation",
        area: Area::Transform,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "vector",
        summary: "A rotação do objeto em graus, nos eixos X, Y e Z do pai, como as Propriedades \
                  mostram.",
        doc: "Escrever gira o objeto, como o nó Mover / girar / escalar objeto, e recusa o que \
              ele recusa. Para girar aos poucos ou combinar giros, use `orientation` e `quat`.",
        help_en: "The object's rotation in degrees about its parent's X, Y and Z, as Properties \
                  shows it; writing turns it.",
        errors: &[
            "escrever outra coisa que um vetor",
            "girar um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "self.rotation = vector.create(0, 90, 0)",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.read", "transform.write"],
        imp: Imp::Property(handles::Property::Rotation),
    },
    ApiFn {
        name: "Object.orientation",
        area: Area::Transform,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "Quat",
        summary: "A rotação do objeto como quatérnio, nos eixos do pai.",
        doc: "Escrever gira o objeto como `rotation`, com os ângulos mais perto dos de agora. \
              As funções de `quat` combinam e interpolam giros.",
        help_en: "The object's rotation in its parent's axes as a quaternion; writing turns it \
                  as rotation does.",
        errors: &[
            "escrever outra coisa que um quatérnio",
            "girar um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "self.orientation = quat.from_euler(0, 45, 0)",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.read", "transform.write"],
        imp: Imp::Property(handles::Property::Orientation),
    },
    ApiFn {
        name: "Object.scale",
        area: Area::Transform,
        kind: ApiKind::Property { write: true },
        generics: "",
        params: &[],
        returns: "vector",
        summary: "A escala do objeto em X, Y e Z, relativa ao pai, como as Propriedades mostram.",
        doc: "Escrever muda o tamanho, como o nó Mover / girar / escalar objeto: cada eixo fica \
              acima de 0,00001 em valor absoluto.",
        help_en: "The object's scale on X, Y and Z, relative to its parent; writing resizes it.",
        errors: &[
            "escrever outra coisa que um vetor",
            "uma escala que chega a zero",
            "escalar um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "self.scale = vector.create(2, 2, 2)",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.write"],
        imp: Imp::Property(handles::Property::Scale),
    },
    ApiFn {
        name: "Object.yaw",
        area: Area::Transform,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "number",
        summary: "O giro horizontal do objeto no mundo, em graus: para onde a frente (-Z) aponta.",
        doc: "Só leitura, como o Giro horizontal do nó Ler posição e orientação: 0 olha para -Z e \
              90 para -X. Um personagem 3D gira com o giro do personagem.",
        help_en: "The object's horizontal turn in the world in degrees (read only): where its \
                  front (-Z) points, 0 to -Z and 90 to -X.",
        errors: &["o objeto não existe mais"],
        example: "print(self.yaw)",
        domain: Domain::Pure,
        since: V07,
        nodes: &["transform.read"],
        imp: Imp::Property(handles::Property::Yaw),
    },
    ApiFn {
        name: "Object:jump_to",
        area: Area::Transform,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "position",
            "vector",
            "onde o pivô do objeto fica, no mundo, em metros",
        )],
        returns: "()",
        summary: "Leva o objeto para uma posição do mundo de uma vez, como um salto.",
        doc: "Como escrever `world_position`, mas as plataformas móveis do objeto e do que está \
              nele soltam quem está em cima, sem lançá-lo (o modo Salto do nó Mover / girar / \
              escalar objeto).",
        help_en: "Moves the object's pivot to a world position at once, as a jump: moving \
                  platforms in it drop their riders without launching them.",
        errors: &[
            "a posição não é um vetor",
            "mover um personagem 3D ou um objeto que uma animação tocando posiciona",
            "o objeto não existe mais",
        ],
        example: "self:jump_to(vector.create(0, 5, 0))",
        domain: Domain::Target,
        since: V07,
        nodes: &["transform.write"],
        imp: Imp::Method(objects::jump_to),
    },
    ApiFn {
        name: "Object:to_world_direction",
        area: Area::Transform,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "direction",
            "vector",
            "uma direção nos eixos do objeto, como vector.create(0, 0, -1), a frente",
        )],
        returns: "vector",
        summary: "Uma direção nos eixos do objeto, girada para os eixos do mundo.",
        doc: "Só a rotação conta (a escala e a posição não): a frente do objeto no mundo é \
              `self:to_world_direction(vector.create(0, 0, -1))`.",
        help_en: "A direction in the object's axes turned into the world's (rotation only), as \
                  vector.direction local to world.",
        errors: &["a direção não é um vetor", "o objeto não existe mais"],
        example: "print(self:to_world_direction(vector.create(0, 0, -1)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &["vector.direction"],
        imp: Imp::Method(objects::to_world_direction),
    },
    ApiFn {
        name: "Object:to_local_direction",
        area: Area::Transform,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "direction",
            "vector",
            "uma direção nos eixos do mundo",
        )],
        returns: "vector",
        summary: "Uma direção do mundo, girada para os eixos do objeto.",
        doc: "O contrário de `to_world_direction`: só a rotação conta.",
        help_en: "A world direction turned into the object's axes (rotation only), as \
                  vector.direction world to local.",
        errors: &["a direção não é um vetor", "o objeto não existe mais"],
        example: "print(self:to_local_direction(vector.create(0, 0, -1)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &["vector.direction"],
        imp: Imp::Method(objects::to_local_direction),
    },
    // Math.
    ApiFn {
        name: "vmath.lerp",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("a", "vector", "o vetor de partida"),
            param("b", "vector", "o vetor de chegada"),
            param("t", "number", "0 dá a, 1 dá b; fora disso, além deles"),
        ],
        returns: "vector",
        summary: "O vetor entre `a` e `b` na fração `t`: a + (b - a) * t.",
        doc: "O mesmo que o `vector.lerp` do Luau; `math.lerp` faz o mesmo com números.",
        help_en: "The vector between a and b at fraction t (a + (b - a) * t), as Luau's \
                  vector.lerp.",
        errors: &["a ou b que não é um vetor, t que não é um número"],
        example: "print(vmath.lerp(vector.zero, vector.one, 0.5))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::vmath_lerp),
    },
    ApiFn {
        name: "vmath.distance",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("a", "vector", "um ponto"),
            param("b", "vector", "outro ponto"),
        ],
        returns: "number",
        summary: "A distância entre dois pontos, em metros.",
        doc: "O comprimento de `b - a`, em precisão dupla.",
        help_en: "The distance between two points, in metres.",
        errors: &["a ou b que não é um vetor"],
        example: "print(vmath.distance(self.position, vector.create(0, 0, 0)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &["vector3.math"],
        imp: Imp::Function(math::vmath_distance),
    },
    ApiFn {
        name: "vmath.normalize_or_zero",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("v", "vector", "o vetor")],
        returns: "vector",
        summary: "O vetor com comprimento 1, ou zero quando ele não tem direção.",
        doc: "Como o A normalizado (zero seguro) do nó Operações com Vetor3: um vetor zero dá \
              zero, sem NaN (o `vector.normalize` do Luau dá NaN).",
        help_en: "The vector with length 1, or zero for a zero vector (Luau's vector.normalize \
                  gives NaN).",
        errors: &["v que não é um vetor"],
        example: "print(vmath.normalize_or_zero(vector.create(3, 0, 4)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &["vector3.math"],
        imp: Imp::Function(math::vmath_normalize_or_zero),
    },
    ApiFn {
        name: "vmath.flat",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("v", "vector", "o vetor")],
        returns: "vector",
        summary: "O vetor no chão: o mesmo com y = 0.",
        doc: "Para direções e distâncias no plano, sem a altura.",
        help_en: "The vector with y = 0.",
        errors: &["v que não é um vetor"],
        example: "print(vmath.flat(vector.create(1, 2, 3)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::vmath_flat),
    },
    ApiFn {
        name: "vmath.yaw_of",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("direction", "vector", "a direção, no mundo")],
        returns: "number",
        summary: "O giro horizontal, em graus, que aponta a frente de um objeto para a direção.",
        doc: "O mesmo giro de `obj.yaw`: 0 olha para -Z e 90 para -X; a altura não conta, e uma \
              direção vertical ou zero dá 0. `obj.rotation = vector.create(0, vmath.yaw_of(d), \
              0)` vira o objeto para `d`.",
        help_en: "The horizontal turn in degrees that points an object's front at a direction, \
                  as obj.yaw reads it (0 for a vertical or zero direction).",
        errors: &["direction que não é um vetor"],
        example: "self.rotation = vector.create(0, vmath.yaw_of(vector.create(-1, 0, 0)), 0)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::vmath_yaw_of),
    },
    ApiFn {
        name: "quat.identity",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[],
        returns: "Quat",
        summary: "O quatérnio sem giro.",
        doc: "Um quatérnio nunca muda; feito no topo do script, é uma constante.",
        help_en: "The quaternion with no rotation.",
        errors: &[],
        example: "self.orientation = quat.identity()",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_identity),
    },
    ApiFn {
        name: "quat.from_euler",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("x", "number", "graus em X"),
            param("y", "number", "graus em Y"),
            param("z", "number", "graus em Z"),
        ],
        returns: "Quat",
        summary: "O giro dos ângulos X, Y e Z em graus, na ordem das Propriedades.",
        doc: "`quat.from_euler(0, 90, 0)` é o mesmo giro que `rotation = vector.create(0, 90, \
              0)`.",
        help_en: "The rotation of the X, Y and Z angles in degrees, in Properties' order.",
        errors: &["um ângulo que não é um número"],
        example: "self.orientation = quat.from_euler(0, 90, 0)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_from_euler),
    },
    ApiFn {
        name: "quat.from_axis_angle",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("axis", "vector", "o eixo do giro (qualquer comprimento)"),
            param("degrees", "number", "o ângulo, em graus"),
        ],
        returns: "Quat",
        summary: "O giro de `degrees` graus em volta de um eixo.",
        doc: "Visto da ponta do eixo, um ângulo positivo gira no sentido anti-horário.",
        help_en: "The rotation of degrees about an axis.",
        errors: &[
            "axis que não é um vetor ou é zero",
            "degrees que não é um número",
        ],
        example: "self.orientation = quat.from_axis_angle(vector.create(0, 1, 0), 90)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_from_axis_angle),
    },
    ApiFn {
        name: "quat.look_at",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("forward", "vector", "para onde a frente (-Z) deve olhar"),
            param(
                "up",
                "vector?",
                "para onde o topo (+Y) deve ficar; +Y sem ele",
            ),
        ],
        returns: "Quat",
        summary: "O giro que aponta a frente de um objeto (-Z) para uma direção.",
        doc: "O topo fica o mais perto possível de `up`.",
        help_en: "The rotation that points an object's front (-Z) along forward, its top as \
                  close to up (default +Y) as it gets.",
        errors: &[
            "forward ou up que não é um vetor, forward zero",
            "forward na mesma direção de up",
        ],
        example: "self.orientation = quat.look_at(vector.create(1, 0, 0))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_look_at),
    },
    ApiFn {
        name: "quat.slerp",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("a", "Quat", "o giro de partida"),
            param("b", "Quat", "o giro de chegada"),
            param("t", "number", "0 dá a, 1 dá b"),
        ],
        returns: "Quat",
        summary: "O giro entre `a` e `b` na fração `t`, pelo caminho mais curto.",
        doc: "Para girar aos poucos: chame a cada passo com `t` crescendo.",
        help_en: "The rotation between a and b at fraction t, along the shortest way.",
        errors: &["a ou b que não é um quatérnio, t que não é um número"],
        example: "self.orientation = quat.slerp(quat.identity(), quat.from_euler(0, 90, 0), 0.5)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_slerp),
    },
    ApiFn {
        name: "quat.multiply",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("a", "Quat", "o giro de fora"),
            param("b", "Quat", "o giro aplicado primeiro"),
        ],
        returns: "Quat",
        summary: "O giro `b` seguido do giro `a`.",
        doc: "O operador `a * b` faz o mesmo, mas os tipos do VS Code não o conhecem: com \
              `--!strict`, use a função.",
        help_en: "The rotation b followed by a (also a * b at runtime).",
        errors: &["a ou b que não é um quatérnio"],
        example: "self.orientation = quat.multiply(self.orientation, quat.from_euler(0, 10, 0))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_multiply),
    },
    ApiFn {
        name: "quat.rotate",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("q", "Quat", "o giro"),
            param("v", "vector", "o vetor a girar"),
        ],
        returns: "vector",
        summary: "O vetor `v` girado por `q`.",
        doc: "O operador `q * v` faz o mesmo, mas os tipos do VS Code não o conhecem: com \
              `--!strict`, use a função.",
        help_en: "The vector v turned by q (also q * v at runtime).",
        errors: &["q que não é um quatérnio ou v que não é um vetor"],
        example: "print(quat.rotate(quat.from_euler(0, 90, 0), vector.create(0, 0, -1)))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_rotate),
    },
    ApiFn {
        name: "quat.inverse",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("q", "Quat", "o giro")],
        returns: "Quat",
        summary: "O giro contrário de `q`.",
        doc: "`quat.multiply(quat.inverse(q), q)` não gira nada.",
        help_en: "The rotation opposite to q.",
        errors: &["q que não é um quatérnio"],
        example: "self.orientation = quat.inverse(self.orientation)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_inverse),
    },
    ApiFn {
        name: "quat.to_euler",
        area: Area::Math,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("q", "Quat", "o giro")],
        returns: "vector",
        summary: "Os ângulos X, Y e Z em graus do giro, na ordem das Propriedades.",
        doc: "O contrário de `quat.from_euler`. É o que a memória, os temporizadores e as \
              mensagens guardam de um giro: eles não guardam quatérnios.",
        help_en: "The X, Y and Z angles in degrees of the rotation, in Properties' order.",
        errors: &["q que não é um quatérnio"],
        example: "print(quat.to_euler(self.orientation))",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(math::quat_to_euler),
    },
    // Attributes.
    ApiFn {
        name: "Object.attr",
        area: Area::Attributes,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "@attr",
        summary: "Os atributos do objeto, pelo nome.",
        doc: "`obj.attr.Vida` lê o atributo, ou nil quando o objeto não o tem; `obj.attr.Vida = \
              2` escreve um atributo que o objeto tem, com um valor do mesmo tipo. Um Vetor2 lê \
              como vetor com z = 0 e recebe o x e o y de um vetor; um atributo Objeto lê o \
              objeto, e um Superfície, a superfície (com `.name`), ou nil quando vazios, e \
              recebem nil para ficar vazios. Nomes com espaços vão entre colchetes: \
              `obj.attr[\"Ponto de retorno\"]`. Quando a chamada para num erro com nil, o erro \
              diz o último atributo que ela leu como nil.",
        help_en: "The object's attributes by name: reading one it lacks gives nil; writing needs \
                  an attribute it has, with a value of its type.",
        errors: &[
            "escrever num atributo que o objeto não tem (o erro diz como criá-lo)",
            "escrever um valor de outro tipo",
            "escrever infinito ou NaN",
            "o objeto não existe mais",
        ],
        example: "self.attr.Vida -= 1",
        domain: Domain::Target,
        since: V07,
        nodes: &["attribute.get", "attribute.set"],
        imp: Imp::Property(handles::Property::Attr),
    },
    ApiFn {
        name: "Object:has_attribute",
        area: Area::Attributes,
        kind: ApiKind::Method,
        generics: "",
        params: &[param("name", "string", "o nome do atributo")],
        returns: "boolean",
        summary: "Diz se o objeto tem o atributo.",
        doc: "Confira antes de agir sobre outro objeto, como o que entrou numa área.",
        help_en: "Whether the object has the attribute.",
        errors: &["o nome não é um texto", "o objeto não existe mais"],
        example: "if self:has_attribute(\"Vida\") then\n    print(\"tem vida\")\nend",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::FastMethod(handles::FastMethod::HasAttribute),
    },
    ApiFn {
        name: "attributes",
        area: Area::Attributes,
        kind: ApiKind::Declaration,
        generics: "",
        params: &[],
        returns: "{ [string]: any }",
        summary: "Os atributos que o script espera no objeto, com o valor padrão de cada um.",
        doc: "O tipo sai do padrão: número, texto, booleano ou vetor (Vetor3); para os tipos \
              sem valor literal, `param.object()`, `param.surface(nome?)` e `param.vector2(x, \
              y)`. Anexar o script cria no objeto, no mesmo passo de desfazer, os que ele não \
              tem; um que ele tem com outro tipo fica como está, com um aviso, e dois scripts do \
              objeto que declaram o mesmo nome com tipos diferentes também geram um aviso. Um \
              atributo declarado depois de anexar só aparece ao anexar de novo.",
        help_en: "Module.attributes = {Name = default}: the attributes attach_script gives an \
                  object that lacks them (param.object(), param.surface(name?) and \
                  param.vector2(x, y) for the types with no literal).",
        errors: &[
            "um padrão que não é número, texto, booleano, vetor nem marcador `param`: anexar \
             avisa e não cria nenhum",
            "`param.surface` com o nome de uma superfície que o projeto não tem: anexar avisa e \
             não cria nenhum",
        ],
        example: "M.attributes = { Velocidade = 90, Aviso = \"\", Alvo = param.object(), Piso = \
                  param.surface() }",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Declaration,
    },
    ApiFn {
        name: "param.object",
        area: Area::Attributes,
        kind: ApiKind::Function,
        generics: "",
        params: &[],
        returns: "Object?",
        summary: "Declara um atributo Objeto, vazio até alguém escolher o objeto.",
        doc: "Em `Modulo.attributes = { Alvo = param.object() }`: anexar cria o atributo vazio, \
              e o objeto se escolhe nas Propriedades (ou com `set_attribute` pelo MCP), o que \
              sobrevive a renomear o objeto. Lido no script, é o objeto ou nil. Em \
              `Modulo.state`, declara um campo de objeto que começa vazio.",
        help_en: "Declares an Object attribute in Module.attributes (or a memory field in \
                  Module.state), empty until one is picked; scripts read the object or nil.",
        errors: &[],
        example: "M.attributes = { Alvo = param.object() }",
        domain: Domain::Pure,
        since: V07,
        nodes: &["value.object"],
        imp: Imp::Function(api::param_object),
    },
    ApiFn {
        name: "param.surface",
        area: Area::Attributes,
        kind: ApiKind::Function,
        generics: "",
        params: &[param(
            "name",
            "string?",
            "o nome de uma superfície do projeto; sem ele, a superfície padrão",
        )],
        returns: "Surface?",
        summary: "Declara um atributo Superfície, com a superfície do projeto de nome `name`.",
        doc: "Em `Modulo.attributes = { Piso = param.surface(\"Gelo\") }`: anexar procura a \
              superfície pelo nome (sem diferenciar maiúsculas) e avisa, sem criar nada, quando \
              o projeto não a tem. Lido no script, é a superfície (com `.name`) ou nil para a \
              padrão. Em `Modulo.state`, só `param.surface()`: um campo que começa vazio.",
        help_en: "Declares a Surface attribute in Module.attributes, the project's surface of \
                  that name (none: the default); scripts read the surface or nil.",
        errors: &["um nome que não é um texto"],
        example: "M.attributes = { Piso = param.surface(\"Gelo\") }",
        domain: Domain::Pure,
        since: V07,
        nodes: &["value.surface"],
        imp: Imp::Function(api::param_surface),
    },
    ApiFn {
        name: "param.vector2",
        area: Area::Attributes,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("x", "number", "o x padrão"),
            param("y", "number", "o y padrão"),
        ],
        returns: "vector",
        summary: "Declara um atributo Vetor2 com o valor padrão (x, y).",
        doc: "Em `Modulo.attributes = { Tamanho = param.vector2(1, 1) }`, e num campo de \
              `Modulo.state`. Lido no script, um Vetor2 é um vetor com z = 0; escrito, recebe o x \
              e o y de um vetor.",
        help_en: "Declares a Vector2 attribute in Module.attributes with the default (x, y); \
                  scripts read it as a vector with z 0.",
        errors: &[
            "x ou y que não é um número, ou que um Vetor2 não guarda (infinito, NaN, além de \
             ±3,4e38)",
        ],
        example: "M.attributes = { Tamanho = param.vector2(1, 1) }",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(api::param_vector2),
    },
    // Memory.
    ApiFn {
        name: "state",
        area: Area::Memory,
        kind: ApiKind::Declaration,
        generics: "",
        params: &[],
        returns: "{ [string]: any }",
        summary: "Os campos da memória do script em cada objeto, com o valor inicial de cada um.",
        doc: "Valores: booleano, número, texto, vetor, objeto, superfície e tabelas desses \
              valores, com chaves inteiras ou de texto, até 4 níveis e 1.024 entradas por \
              tabela; até 64 KB por objeto. Um campo precisa de um valor inicial de verdade (um \
              nil some da tabela); um campo de objeto ou de superfície que começa vazio e um \
              Vetor2 se declaram com os marcadores: `alvo = param.object()`, `piso = \
              param.surface()`, `tamanho = param.vector2(1, 1)`.",
        help_en: "Module.state = {field = initial}: the script's memory on each object, kept by \
                  the engine (it outlives a reload), read and written in self.state.",
        errors: &[
            "um valor que a memória não guarda (função, tabela com metatabela, quatérnio, \
             marcador `param` dentro de uma tabela, `param.surface` com nome): o script não \
             carrega",
            "mais de 64 KB: o script não carrega",
        ],
        example: "M.state = { pulos = 0, nomes = {} :: { string }, alvo = param.object() }",
        domain: Domain::Owner,
        since: V07,
        nodes: &[],
        imp: Imp::Declaration,
    },
    ApiFn {
        name: "Object.state",
        area: Area::Memory,
        kind: ApiKind::Property { write: false },
        generics: "",
        params: &[],
        returns: "@state",
        summary: "A memória deste script neste objeto, com os campos de `Modulo.state`.",
        doc: "Só em `self`. Um campo simples (número, texto, booleano, vetor, objeto, \
              superfície) é lido e gravado na engine na hora, conferido na linha; um campo de \
              objeto ou de superfície recebe nil para ficar vazio. Uma tabela é montada na \
              primeira leitura da chamada e gravada de volta quando a chamada termina, com ou \
              sem erro. O primeiro nível não se percorre nem se mede (`pairs`, `#`, \
              `table.clone`): guarde listas num campo.",
        help_en: "This script's memory on the object, the fields Module.state declares (self \
                  only); tables are written back when the call ends.",
        errors: &[
            "o script não declara `Modulo.state`",
            "um campo que `Modulo.state` não declara",
            "um valor de outro tipo que o do campo",
            "a memória de outro objeto: só `self.state` existe",
            "mais de 64 KB no objeto: o script para no objeto",
        ],
        example: "self.state.contagem += 1",
        domain: Domain::Owner,
        since: V07,
        nodes: &[],
        imp: Imp::Property(handles::Property::State),
    },
    // Timers.
    ApiFn {
        name: "Object:after",
        area: Area::Timers,
        kind: ApiKind::Method,
        generics: "<X...>",
        params: &[
            param("seconds", "number", "segundos de jogo, de 0 a 86400"),
            param(
                "fn",
                "(@self, X...) -> ()",
                "uma função da tabela do módulo, como `M.fechar`",
            ),
            param("...", "X...", "os dados que `fn` recebe depois de `self`"),
        ],
        returns: "number",
        summary: "Chama `fn(self, ...)` depois de `seconds` segundos de jogo.",
        doc: "No mínimo no passo seguinte, como o nó Esperar sem bloquear: os dois, começados no \
              mesmo passo com o mesmo tempo, disparam no mesmo passo. A engine guarda o nome da \
              função e os dados, então o temporizador atravessa a recarga do script. Devolve o \
              número que `cancel` recebe.",
        help_en: "Calls Module.f(self, ...) after that many seconds of game time, at least on the \
                  next step, as control.wait; returns the id cancel takes.",
        errors: TIMER_ERRORS,
        example: "self:after(2, M.fechar)",
        domain: Domain::Owner,
        since: V07,
        nodes: &["control.wait"],
        imp: Imp::Method(timers::after),
    },
    ApiFn {
        name: "Object:every",
        area: Area::Timers,
        kind: ApiKind::Method,
        generics: "<X...>",
        params: &[
            param("seconds", "number", "o intervalo em segundos de jogo"),
            param(
                "fn",
                "(@self, X...) -> ()",
                "uma função da tabela do módulo, como `M.mostrar`",
            ),
            param("...", "X...", "os dados que `fn` recebe depois de `self`"),
        ],
        returns: "number",
        summary: "Chama `fn(self, ...)` a cada `seconds` segundos de jogo, até `cancel`.",
        doc: "A primeira chamada sai depois de `seconds`. As regras de `after` valem aqui.",
        help_en: "Calls Module.f(self, ...) every that many seconds of game time until cancel.",
        errors: TIMER_ERRORS,
        example: "self:every(1, M.mostrar, 1)",
        domain: Domain::Owner,
        since: V07,
        nodes: &["control.wait"],
        imp: Imp::Method(timers::every),
    },
    ApiFn {
        name: "Object:sequence",
        area: Area::Timers,
        kind: ApiKind::Method,
        generics: "<T>",
        params: &[
            param(
                "interval",
                "number",
                "segundos de jogo entre um item e o seguinte",
            ),
            param(
                "fn",
                "(@self, T) -> ()",
                "uma função da tabela do módulo, que recebe cada item",
            ),
            param("items", "{ T }", "a lista de itens, até 1.024"),
        ],
        returns: "number",
        summary: "Chama `fn(self, item)` com cada item da lista, um a cada `interval` segundos.",
        doc: "O item k sai aos (k-1) × `interval` segundos da chamada, como `self:after` \
              chamaria: o primeiro no passo seguinte, como `self:after(0)`, e os outros na grade \
              das esperas do grafo. O estado que precisa valer já neste passo, grave direto \
              antes; o primeiro item o grava de novo, e um efeito dele (um som) sairia duas \
              vezes.",
        help_en: "Calls Module.f(self, item) with item k (k-1) x interval seconds after the \
                  call, the first on the next step.",
        errors: TIMER_ERRORS,
        example: "self:sequence(1, M.mostrar, { 3, 2, 1 })",
        domain: Domain::Owner,
        since: V07,
        nodes: &["control.wait"],
        imp: Imp::Method(timers::sequence),
    },
    ApiFn {
        name: "Object:schedule",
        area: Area::Timers,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "steps",
            "{ Step }",
            "os passos `{ atraso, M.f, dado? }`, até 1.024",
        )],
        returns: "number",
        summary: "Passos com atrasos próprios: `{ {atraso, M.f, dado?}, ... }`.",
        doc: "Cada atraso conta do passo anterior; o primeiro, da chamada, no mínimo o passo \
              seguinte.",
        help_en: "Steps with delays of their own, { {delay, Module.f, data?}, ... }, each delay \
                  counted from the step before.",
        errors: TIMER_ERRORS,
        example: "self:schedule({ { 0.5, M.mostrar, 1 }, { 1, M.fechar } })",
        domain: Domain::Owner,
        since: V07,
        nodes: &["control.wait"],
        imp: Imp::Method(timers::schedule),
    },
    ApiFn {
        name: "Object:cancel",
        area: Area::Timers,
        kind: ApiKind::Method,
        generics: "",
        params: &[param(
            "id",
            "number",
            "o número que `after`, `every`, `sequence` ou `schedule` devolveu",
        )],
        returns: "boolean",
        summary: "Cancela o que falta de um temporizador do objeto; diz se ainda faltava algo.",
        doc: "Vale para os temporizadores de qualquer script do mesmo objeto.",
        help_en: "Cancels what is left of one of the object's timers; whether anything was left.",
        errors: &[
            "chamado em outro objeto que não `self`",
            "o id não é um número",
        ],
        example: "local id = self:after(5, M.fechar)\nself:cancel(id)",
        domain: Domain::Owner,
        since: V07,
        nodes: &[],
        imp: Imp::Method(timers::cancel),
    },
    // Audio.
    ApiFn {
        name: "audio.play",
        area: Area::Audio,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("name", "string", "o nome do som na Biblioteca"),
            param(
                "options",
                "PlayOptions?",
                "`{ volume = 0.5, audience = obj }`",
            ),
        ],
        returns: "()",
        summary: "Toca uma vez o som `name`, pelo nome que ele tem na Biblioteca.",
        doc: "O nome vale também sem diferenciar maiúsculas nem os espaços das pontas. \
              `audience` é aceito desde já, mas só escolhe quem ouve a partir da 0.8, com vários \
              jogadores: hoje o único jogador ouve todo som.",
        help_en: "Plays a sound of the project once, by its name in the Biblioteca \
                  ({volume?, audience?}; audience changes nothing before 0.8).",
        errors: &[
            "o nome não é um texto",
            "um som que o projeto não tem (o erro lista os sons)",
            "dois sons com o mesmo nome",
            "uma opção que não é volume nem audience, ou um volume que não é número",
            "audience que não é um objeto ou não existe mais",
        ],
        example: "audio.play(\"Bipe\", { volume = 0.5 })",
        domain: Domain::Local,
        since: V07,
        nodes: &["action.sound"],
        imp: Imp::Function(api::audio_play),
    },
    // Game variables.
    ApiFn {
        name: "vars.get",
        area: Area::Variables,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("name", "string", "o nome da variável do jogo"),
            param(
                "key",
                "string?",
                "a chave, numa variável com um valor por chave",
            ),
        ],
        returns: "any",
        summary: "Lê uma variável do jogo pelo nome.",
        doc: "O nome vale sem diferenciar maiúsculas. Com `key`, o valor daquela chave; numa \
              chave nunca gravada, o valor inicial da variável, como o nó Ler variável do jogo, \
              e nunca nil. Um Vetor2 lê como vetor com z = 0. As variáveis guardam o valor \
              quando a cena reinicia: zere no `on_start` o que precisa recomeçar.",
        help_en: "Reads a game variable by name (any case); a key never written reads the \
                  variable's initial value.",
        errors: &[
            "uma variável que o projeto não tem (com o nome mais próximo e a lista)",
            "nome ou chave que não é texto",
        ],
        example: "print(vars.get(\"Pontos\"))",
        domain: Domain::Pure,
        since: V07,
        nodes: &["variable.get"],
        imp: Imp::Function(api::vars_get),
    },
    ApiFn {
        name: "vars.set",
        area: Area::Variables,
        kind: ApiKind::Function,
        generics: "",
        params: &[
            param("name", "string", "o nome da variável do jogo"),
            param("value", "VarValue", "um valor do tipo da variável"),
            param(
                "key",
                "string?",
                "a chave, numa variável com um valor por chave",
            ),
        ],
        returns: "()",
        summary: "Grava uma variável do jogo pelo nome, com um valor do tipo dela.",
        doc: "Com `key`, grava o valor daquela chave, até 1.024 chaves por variável. Uma \
              variável Vetor2 recebe o x e o y de um vetor.",
        help_en: "Writes a game variable by name with a value of its type; with key, that key's \
                  value.",
        errors: &[
            "uma variável que o projeto não tem (com o nome mais próximo e a lista)",
            "um valor de outro tipo que o da variável",
            "nome ou chave que não é texto, ou chave de mais de 4.096 bytes",
            "mais de 1.024 chaves na variável",
        ],
        example: "vars.set(\"Pontos\", 10)",
        domain: Domain::Scope,
        since: V07,
        nodes: &["variable.set"],
        imp: Imp::Function(api::vars_set),
    },
    // Diagnostics.
    ApiFn {
        name: "print",
        area: Area::Diagnostics,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("...", "any", "os valores, separados por espaço")],
        returns: "()",
        summary: "Escreve uma linha no Console, com o nome do objeto e a linha do script.",
        doc: "Objetos saem pelo nome e tabelas com o primeiro nível. O `oxy_simulate` e o \
              `oxy_console` trazem as linhas com o arquivo e a linha.",
        help_en: "Logs a line with the object's name and the script line.",
        errors: &[],
        example: "print(\"vida\", self.attr.Vida)",
        domain: Domain::Local,
        since: V07,
        nodes: &["debug.message"],
        imp: Imp::Function(sandbox::print),
    },
    ApiFn {
        name: "warn",
        area: Area::Diagnostics,
        kind: ApiKind::Function,
        generics: "",
        params: &[param("...", "any", "os valores, separados por espaço")],
        returns: "()",
        summary: "Escreve um aviso no Console, com o nome do objeto e a linha do script.",
        doc: "Como `print`, com \"aviso:\" antes do texto.",
        help_en: "Logs a warning line with the object's name and the script line.",
        errors: &[],
        example: "warn(\"sem alvo\")",
        domain: Domain::Local,
        since: V07,
        nodes: &["debug.message"],
        imp: Imp::Function(sandbox::warn),
    },
    // Modules.
    ApiFn {
        name: "require",
        area: Area::Modules,
        kind: ApiKind::Function,
        generics: "",
        params: &[param(
            "path",
            "string",
            "o caminho do arquivo sem `.luau`, a partir da pasta deste script: `./` nela, \
             `../` na de cima",
        )],
        returns: "any",
        summary: "Carrega um módulo da pasta `scripts` e devolve o que ele devolve.",
        doc: "`require(\"./dados/cartas\")` num script de `scripts/fases/` lê \
              `scripts/fases/dados/cartas.luau`; o caminho nunca sai da pasta `scripts`. O \
              módulo roda na primeira vez que um script o pede, e todos recebem a mesma tabela, \
              travada: ninguém a muda depois. Costuma ficar no topo do script (`local \
              Cartas = require(\"./dados/cartas\")`). `@native/` fica reservado para os módulos \
              em Rust de um jogo, que ainda não existem.",
        help_en: "Loads a module of the scripts folder, the path from this script's folder \
                  (./ or ../, no .luau); every script gets the same frozen table.",
        errors: &[
            "um caminho que não começa com `./` nem `../`, ou que sai da pasta `scripts`",
            "um arquivo que não existe",
            "um erro no módulo, com o arquivo e a linha dele",
            "um módulo que pede, direta ou indiretamente, o script que o pediu (require \
             circular)",
            "um apelido com `@` (`@native/` está reservado)",
        ],
        example: "local dados = require(\"./dados\")\nprint(dados.custo)",
        domain: Domain::Pure,
        since: V07,
        nodes: &[],
        imp: Imp::Function(sandbox::require),
    },
];

/// Rows luau-lsp types itself, which the types and the plan-B module leave out: a declaration
/// would hide what it knows (the type of the module `require` loads).
const TYPED_BY_LUAU: [&str; 1] = ["require"];

/// Names of members longer than this go in the last bucket of [`OBJECT`].
const LONGEST: usize = 31;

/// The rows whose member names have one length (the last bucket, any longer), with the names.
type Bucket = Vec<(&'static [u8], &'static ApiFn)>;

/// The rows objects have, by the length of their member names, as the handles look them up on
/// every access: properties before methods, and the attributes first, the most read.
static OBJECT: LazyLock<Vec<Bucket>> = LazyLock::new(|| {
    let mut buckets = vec![Vec::new(); LONGEST + 1];
    let mut rows: Vec<&'static ApiFn> = API.iter().filter(|f| f.of_objects()).collect();
    rows.sort_by_key(|f| match f.imp {
        Imp::Property(handles::Property::Attr) => 0,
        Imp::Property(_) => 1,
        _ => 2,
    });
    for f in rows {
        let name = f.member().as_bytes();
        buckets[name.len().min(LONGEST)].push((name, f));
    }
    buckets
});

/// The property or method objects have under `name`.
pub(crate) fn object_member(name: &[u8]) -> Option<&'static ApiFn> {
    OBJECT[name.len().min(LONGEST)]
        .iter()
        .find(|(member, _)| *member == name)
        .map(|(_, f)| *f)
}

/// The names objects have, properties and methods.
pub(crate) fn object_members() -> impl Iterator<Item = &'static str> {
    API.iter().filter(|f| f.of_objects()).map(|f| f.member())
}

/// Names scripts written for other engines, or in Portuguese, give members of objects, with the
/// member meant (lowercase, without accents where people drop them).
const ALIASES: &[(&str, &str)] = &[
    ("visivel", "visible"),
    ("visível", "visible"),
    ("hidden", "visible"),
    ("active", "visible"),
    ("shown", "visible"),
    ("enabled", "logic_enabled"),
    ("logic", "logic_enabled"),
    ("behavior", "logic_enabled"),
    ("behaviour", "logic_enabled"),
    ("pai", "parent"),
    ("nome", "name"),
    ("filhos", "children"),
    ("posicao", "position"),
    ("posição", "position"),
    ("pos", "position"),
    ("location", "position"),
    ("localposition", "position"),
    ("globalposition", "world_position"),
    ("global_position", "world_position"),
    ("worldposition", "world_position"),
    ("rotacao", "rotation"),
    ("rotação", "rotation"),
    ("rotation_degrees", "rotation"),
    ("angles", "rotation"),
    ("quaternion", "orientation"),
    ("escala", "scale"),
    ("giro", "yaw"),
    ("heading", "yaw"),
    ("remove", "destroy"),
    ("delete", "destroy"),
    ("destruir", "destroy"),
    ("remover", "destroy"),
    ("copy", "clone"),
    ("duplicate", "clone"),
    ("copiar", "clone"),
    ("atributos", "attr"),
    ("attributes", "attr"),
    ("memoria", "state"),
    ("memória", "state"),
];

/// The member of objects people coming from elsewhere call `name` (lowercase).
pub(crate) fn member_alias(name: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map(|(_, member)| *member)
}

/// The callback a module's function named `name` is.
pub(crate) fn callback(name: &str) -> Option<Callback> {
    API.iter().find_map(|f| match f.imp {
        Imp::Callback(callback) if f.name == name => Some(callback),
        _ => None,
    })
}

/// The row named `name`.
pub fn find(name: &str) -> Option<&'static ApiFn> {
    API.iter().find(|f| f.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::{ScriptHost, sandbox};
    use mlua::{LuaOptions, Table, Value as LuaValue};
    use std::collections::BTreeSet;

    /// A VM as the sandbox starts from: Luau's libraries as mlua opens them, without the `pcall`
    /// and `xpcall` of its own that catching Rust panics would put in.
    fn stock() -> mlua::Lua {
        let options = LuaOptions::new().catch_rust_panics(false);
        mlua::Lua::new_with(sandbox::libraries(), options).unwrap()
    }

    fn keys(table: &Table) -> BTreeSet<String> {
        table
            .pairs::<String, LuaValue>()
            .map(|pair| pair.unwrap().0)
            .collect()
    }

    /// Whatever a script reaches of the engine is a row: the globals beyond Luau's own, the
    /// functions of the global tables and the methods of objects; and every row is there.
    #[test]
    fn nothing_scripts_reach_is_outside_the_table() {
        let host = ScriptHost::new(std::sync::Arc::default()).unwrap();
        let lua = host.lua();
        let base: Table = lua.named_registry_value("oxy.base").unwrap();
        let luau = stock();
        let standard = keys(&luau.globals());
        let mut holders: BTreeSet<&str> = BTreeSet::new();
        for f in API.iter().filter(|f| f.kind == ApiKind::Function) {
            holders.insert(if f.holder().is_empty() {
                f.name
            } else {
                f.holder()
            });
        }
        for name in keys(&base) {
            assert!(
                standard.contains(&name) || holders.contains(name.as_str()),
                "{name} está nos globais dos scripts e não está na tabela da API"
            );
        }
        for holder in &holders {
            let value: LuaValue = base.raw_get(*holder).unwrap();
            match value {
                LuaValue::Table(table) => {
                    let rows: BTreeSet<String> = API
                        .iter()
                        .filter(|f| f.kind == ApiKind::Function && f.holder() == *holder)
                        .map(|f| f.member().to_owned())
                        .collect();
                    assert_eq!(keys(&table), rows, "{holder}");
                }
                LuaValue::Function(_) => assert!(find(holder).is_some(), "{holder}"),
                other => panic!("{holder}: {other:?}"),
            }
        }
        let methods: Table = lua.named_registry_value("oxy.methods").unwrap();
        let rows: BTreeSet<String> = API
            .iter()
            .filter(|f| f.kind == ApiKind::Method)
            .map(|f| f.member().to_owned())
            .collect();
        assert_eq!(keys(&methods), rows);
    }

    /// The C function behind `f`: the same for a function of Luau's library in every VM, mlua's
    /// one trampoline for every function written in Rust, none for a Luau function.
    fn c_function(lua: &mlua::Lua, f: &Function) -> usize {
        // SAFETY: reads the function the call pushed and leaves one light userdata.
        let mlua::LightUserData(found) = unsafe {
            lua.exec_raw::<mlua::LightUserData>(f.clone(), |l| {
                let found = mlua::ffi::lua_tocfunction(l, -1).map_or(0, |c| c as usize);
                mlua::ffi::lua_settop(l, 0);
                mlua::ffi::lua_pushlightuserdata(l, found as *mut std::ffi::c_void);
            })
        }
        .unwrap();
        found as usize
    }

    /// A function of Luau's library the engine replaced (a `tostring` without addresses, a
    /// `math.random` that refuses) is in `sandbox::SANDBOXED`, and every other function the
    /// engine wrote is a row: one with a standard name, as `require`, cannot slip past the table.
    #[test]
    fn the_standard_functions_the_engine_replaces_are_listed() {
        let host = ScriptHost::new(std::sync::Arc::default()).unwrap();
        let lua = host.lua();
        let base: Table = lua.named_registry_value("oxy.base").unwrap();
        let luau = stock();
        let rust = c_function(lua, &lua.create_function(|_, ()| Ok(())).unwrap());
        let mut replaced = BTreeSet::new();
        let mut check = |name: String, f: &Function, stock: Option<Function>| {
            let found = c_function(lua, f);
            if found == rust || stock.is_none_or(|stock| c_function(&luau, &stock) != found) {
                assert!(
                    find(&name).is_some() || sandbox::SANDBOXED.contains(&name.as_str()),
                    "{name} é uma função da engine e não está na tabela da API"
                );
                replaced.insert(name);
            }
        };
        let stock_globals = luau.globals();
        for pair in base.pairs::<String, LuaValue>() {
            let (name, value) = pair.unwrap();
            match value {
                LuaValue::Function(f) => check(name.clone(), &f, stock_globals.raw_get(name).ok()),
                // The engine's own tables are rows to the last name (see above).
                LuaValue::Table(table) if name != "_G" => {
                    let Ok(stock) = stock_globals.raw_get::<Table>(name.as_str()) else {
                        continue;
                    };
                    for pair in table.pairs::<String, LuaValue>() {
                        let (member, value) = pair.unwrap();
                        if let LuaValue::Function(f) = value {
                            let stock = stock.raw_get(member.as_str()).ok();
                            check(format!("{name}.{member}"), &f, stock);
                        }
                    }
                }
                _ => {}
            }
        }
        for name in sandbox::SANDBOXED {
            assert!(replaced.contains(name), "{name} não é trocada pela engine");
        }
        assert!(replaced.contains("require") && replaced.contains("print"));
    }

    #[test]
    fn every_row_is_complete_and_says_how_it_is_reached() {
        let mut names = BTreeSet::new();
        let nodes: BTreeSet<&str> = crate::graph::registry().iter().map(|op| op.id).collect();
        for f in API {
            assert!(names.insert(f.name), "{} repetido", f.name);
            assert!(!f.summary.is_empty() && !f.help_en.is_empty(), "{}", f.name);
            assert!(
                f.summary.ends_with('.') && f.doc.ends_with('.'),
                "{}",
                f.name
            );
            assert!(f.example.contains(f.member()), "{}: {}", f.name, f.example);
            assert_eq!(f.since, "0.7.0", "{}", f.name);
            for node in f.nodes {
                assert!(nodes.contains(node), "{}: {node}", f.name);
            }
            // The type parameters never meet ScriptObject's own (S2 §12).
            assert!(
                !f.generics.contains('A') && !f.generics.contains('S'),
                "{}",
                f.name
            );
            let fits = match (f.kind, f.imp) {
                (ApiKind::Function, Imp::Function(_)) => {
                    !f.example_at_top() || f.holder() == "param"
                }
                // A method used in a callback shows the callback.
                (ApiKind::Method, Imp::Method(_) | Imp::FastMethod(_)) => {
                    !f.example_at_top() || f.example.starts_with("function M.on_")
                }
                (ApiKind::Property { write }, Imp::Property(property)) => {
                    write == property.writable() && !f.example_at_top()
                }
                (ApiKind::Callback, Imp::Callback(callback)) => {
                    callback.name() == f.name && f.example_at_top()
                }
                (ApiKind::Declaration, Imp::Declaration) => f.example_at_top(),
                _ => false,
            };
            assert!(fits, "{}: {:?} com {:?}", f.name, f.kind, f.imp);
            assert_eq!(
                f.of_objects(),
                f.name.starts_with("Object.") || f.name.starts_with("Object:"),
                "{}",
                f.name
            );
        }
        // The callbacks are the runtime's, in its order.
        let callbacks: Vec<&str> = API
            .iter()
            .filter(|f| f.kind == ApiKind::Callback)
            .map(|f| f.name)
            .collect();
        assert_eq!(callbacks, Callback::ALL.map(Callback::name));
        // Every area has rows, and the rows come area by area.
        let areas: Vec<Area> = API.iter().map(|f| f.area).collect();
        let mut sorted = areas.clone();
        sorted.sort();
        assert_eq!(areas, sorted);
        for area in Area::ALL {
            assert!(areas.contains(&area), "{area:?}");
        }
        assert!(object_member(b"position").is_some() && object_member(b"posicao").is_none());
        // Every alias names a member.
        for (alias, member) in ALIASES {
            assert!(
                object_member(member.as_bytes()).is_some(),
                "{alias}: {member}"
            );
            assert_eq!(alias.to_lowercase(), *alias);
        }
    }
}
