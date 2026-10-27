//! The single registry of the scripts' API (design §5.7): one row for each name a script reaches
//! or the engine reads from a module, with its signature, its help in Portuguese (the Guide, the
//! types' hover) and in English (agents), what makes it fail, an example that a test runs, the
//! domain it will have in 0.8 and the version it came in. The host installs from it the global
//! tables, the base functions and the methods of objects and of their views, and the handles
//! resolve the properties through it, so nothing a script reaches exists outside it; `generate`
//! writes from it the types for luau-lsp, the plan-B module, the Guide's pages and
//! docs/api-scripts.md. The rows themselves are in `rows`, area by area, with [`SYNTAX`]: what
//! plain Luau does of the nodes, so that every operation of the frozen node registry has its
//! equivalent (design §5.5); the shared types are in `types`.
//!
//! Types in the rows are Luau's, with three marks the generator replaces: `@self` is the object
//! (`Object`, or `ScriptObject<A, S>` in that type), `@attr` its attributes and `@state` its
//! memory, which `ScriptObject` types from the script's declarations.
use super::Callback;
use super::handles;
use super::host::Shared;
use mlua::{Function, Lua};
use std::rc::Rc;
use std::sync::LazyLock;

mod rows;
mod types;
pub use rows::{API, SYNTAX};
pub use types::TYPES;

/// Makes a function of the API for one VM.
pub(crate) type Make = fn(&Lua, &Rc<Shared>) -> mlua::Result<Function>;

/// How a row reaches the scripts.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Imp {
    /// A function of a global table or of the base.
    Function(Make),
    /// A method of objects or of a view, written with mlua, kept in the view's table of methods.
    Method(Make),
    /// A method of objects that runs in C, without a call of its own.
    FastMethod(handles::FastMethod),
    /// A property of objects or of a view, which the handles' `__index` and `__newindex`
    /// resolve.
    Property(handles::Property),
    /// A function of the module the engine calls.
    Callback(Callback),
    /// A field of the module the engine reads when it loads it.
    Declaration,
    /// A number of a global table (`time.dt`).
    Constant(f64),
    /// A function of Luau's own library that the engine keeps as it is (`error`).
    Luau,
    /// A name of 0.8: using it is an error that says so, and a module's function with it a
    /// warning.
    Reserved,
}

/// What a row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiKind {
    /// A function of a global table (`audio.play`) or of the base (`print`).
    Function,
    /// A method of objects or of a view, called with `:` (`obj:exists()`).
    Method,
    /// A property of objects or of a view; `write` when a script may assign it.
    Property { write: bool },
    /// A function of the module, with a fixed name, that the engine calls (`on_start`).
    Callback,
    /// A field of the module the engine reads when it loads it (`Modulo.attributes`).
    Declaration,
    /// A value of a global table (`time.dt`).
    Constant,
    /// A name kept for 0.8 (`players`, `game.online`, `on_request`).
    Reserved,
}

/// What a property or a method belongs to: objects, or a view of an object (`obj.character`,
/// `obj.collider`…, the object's handle with another mark), or a sequence of draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Object,
    Character,
    Collider,
    Platform,
    Controller,
    Animation,
    Ui,
    Stream,
}

impl View {
    pub const ALL: [Self; 8] = [
        Self::Object,
        Self::Character,
        Self::Collider,
        Self::Platform,
        Self::Controller,
        Self::Animation,
        Self::Ui,
        Self::Stream,
    ];

    /// The type its rows belong to, as row names and the types write it (`Character`).
    pub fn name(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Character => "Character",
            Self::Collider => "Collider",
            Self::Platform => "Platform",
            Self::Controller => "Controller",
            Self::Animation => "Animation",
            Self::Ui => "UiElement",
            Self::Stream => "RandomStream",
        }
    }

    /// How a script reaches it, as the Guide writes its members (`obj.character`).
    pub fn path(self) -> &'static str {
        match self {
            Self::Object => "obj",
            Self::Character => "obj.character",
            Self::Collider => "obj.collider",
            Self::Platform => "obj.platform",
            Self::Controller => "obj.controller",
            Self::Animation => "obj.animation",
            Self::Ui => "obj.ui",
            Self::Stream => "stream",
        }
    }

    /// The mark of its handles.
    pub(crate) fn mark(self) -> u64 {
        match self {
            Self::Object => handles::MARK_OBJECT,
            Self::Character => handles::MARK_CHARACTER,
            Self::Collider => handles::MARK_COLLIDER,
            Self::Platform => handles::MARK_PLATFORM,
            Self::Controller => handles::MARK_CONTROLLER,
            Self::Animation => handles::MARK_ANIMATION,
            Self::Ui => handles::MARK_UI,
            Self::Stream => handles::MARK_STREAM,
        }
    }

    /// The view whose handles have `mark`.
    pub(crate) fn of_mark(mark: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.mark() == mark)
    }

    /// What its handle is, in messages.
    pub(crate) fn kind(self) -> &'static str {
        match self {
            Self::Object => "objeto",
            Self::Character => "o personagem 3D de um objeto",
            Self::Collider => "o colisor de um objeto",
            Self::Platform => "a plataforma móvel de um objeto",
            Self::Controller => "o Movimento 2D de um objeto",
            Self::Animation => "as animações de um objeto",
            Self::Ui => "o elemento de interface de um objeto",
            Self::Stream => "uma sequência de sorteios",
        }
    }
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
    Character,
    Physics,
    Animation,
    Camera,
    Interface,
    Audio,
    Game,
    Variables,
    Input,
    Time,
    Diagnostics,
    Modules,
    Reserved,
}

impl Area {
    pub const ALL: [Self; 21] = [
        Self::Events,
        Self::Object,
        Self::Scene,
        Self::Transform,
        Self::Math,
        Self::Attributes,
        Self::Memory,
        Self::Timers,
        Self::Character,
        Self::Physics,
        Self::Animation,
        Self::Camera,
        Self::Interface,
        Self::Audio,
        Self::Game,
        Self::Variables,
        Self::Input,
        Self::Time,
        Self::Diagnostics,
        Self::Modules,
        Self::Reserved,
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
            Self::Character => "Personagem 3D",
            Self::Physics => "Física",
            Self::Animation => "Animação",
            Self::Camera => "Câmera",
            Self::Interface => "Interface",
            Self::Audio => "Áudio",
            Self::Game => "Jogo",
            Self::Variables => "Variáveis e progresso",
            Self::Input => "Entrada",
            Self::Time => "Tempo e sorteio",
            Self::Diagnostics => "Diagnóstico",
            Self::Modules => "Módulos",
            Self::Reserved => "Reservado para a 0.8",
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
            Self::Character => {
                "O Personagem 3D de um objeto, por `obj.character`: o estado dele (onde pisa, a \
                 velocidade, a mira) e os comandos, os mesmos dos nós de personagem. Os comandos \
                 de movimento valem no próximo passo de movimento: renove `move` a cada passo \
                 (`on_step`). Num objeto sem Personagem 3D, `obj.character` é um erro."
            }
            Self::Physics => {
                "Os colisores, as plataformas móveis e o Movimento 2D de um objeto, que se ligam \
                 e desligam, e as consultas à física 3D (raios e formas varridas), que não movem \
                 nada. Uma consulta sem contato devolve nil; uma recusada (cena 2D, mais de 256 \
                 consultas no passo) devolve nil e o motivo."
            }
            Self::Animation => {
                "Os clipes de animação de um objeto, por `obj.animation`, pelos nomes que eles \
                 têm no Estúdio. Os marcadores dos clipes chegam a `on_marker`."
            }
            Self::Camera => {
                "A câmera de jogo: qual está ativa, para onde olha, e os comandos dos nós de \
                 câmera. As funções recebem o objeto câmera; os modos e o olhar assistido pedem \
                 uma câmera com Controle de câmera."
            }
            Self::Interface => {
                "O elemento de interface de um objeto (texto, imagem, botão ou barra), por \
                 `obj.ui`: o texto e a cor dele mudam durante a partida. Esconder um elemento é \
                 `obj.visible = false`; um clique num botão chega a `on_click`."
            }
            Self::Audio => {
                "Sons da Biblioteca, pelo nome que eles têm lá: uma vez nos efeitos, em loop na \
                 música ou nos efeitos, e os volumes dos canais (geral, música e efeitos)."
            }
            Self::Game => {
                "O jogo como um todo: a pausa do jogo (o mundo para, a interface e a lógica \
                 seguem), o cursor do mouse e sair do jogo."
            }
            Self::Variables => {
                "As variáveis do jogo do projeto (Lógica > Variáveis do jogo), pelo nome, e o \
                 progresso salvo: as variáveis marcadas como salvas entre partidas. Elas guardam \
                 o valor quando a cena reinicia."
            }
            Self::Input => {
                "As ações de entrada do projeto (Lógica > Ações de entrada), pelo nome, e os \
                 eixos do passo. É a entrada do jogador que controla o objeto: na 0.7, o único \
                 jogador; o argumento `player` chega na 0.8."
            }
            Self::Time => {
                "O tempo de jogo, que para na pausa do jogo, e os sorteios da engine: cada objeto \
                 e script tem a própria sequência, e a mesma semente dá os mesmos números em \
                 toda máquina. `math.random` não existe nos scripts."
            }
            Self::Diagnostics => {
                "Linhas no Console com o nome do objeto e a linha do script. `error(texto)` e \
                 `assert` do Luau param a chamada com o texto, também na linha."
            }
            Self::Modules => {
                "Funções e dados que vários scripts usam, em outros arquivos `.luau` do \
                 projeto, em qualquer pasta: tabelas de cartas, de fases, de inimigos. Um módulo \
                 só guarda funções, constantes e dados; o que muda no jogo fica nos atributos e na \
                 memória."
            }
            Self::Reserved => {
                "Nomes que chegam na 0.8, com a rede e vários jogadores. Na 0.7, usar um deles é \
                 um erro que diz isso, e uma função do módulo com o nome de um evento da 0.8 gera \
                 um aviso: a engine ainda não a chama."
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

/// What a text names in the project where a script gives it: the lint of names (design §6.4)
/// checks the literal texts of these parameters, and of the attributes' keys (`obj.attr.Vida`),
/// against the project, and tools find in them where a script uses a sound or a scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameKind {
    /// A sound of the Biblioteca.
    Sound,
    /// A game variable.
    Variable,
    /// An attribute of objects, or one a script declares.
    Attribute,
    Scene,
    /// A model of the Biblioteca.
    Model,
    /// An input action.
    Action,
    /// An animation clip of an object.
    Clip,
    /// A marker of an animation clip.
    Marker,
    /// An object, by its name in the Hierarquia.
    Object,
    /// A physics surface.
    Surface,
}

impl NameKind {
    pub const ALL: [Self; 10] = [
        Self::Sound,
        Self::Variable,
        Self::Attribute,
        Self::Scene,
        Self::Model,
        Self::Action,
        Self::Clip,
        Self::Marker,
        Self::Object,
        Self::Surface,
    ];

    /// What it is, with its article, as messages name it: "o som", "a variável do jogo".
    pub fn what(self) -> &'static str {
        match self {
            Self::Sound => "o som",
            Self::Variable => "a variável do jogo",
            Self::Attribute => "o atributo",
            Self::Scene => "a cena",
            Self::Model => "o modelo",
            Self::Action => "a ação de entrada",
            Self::Clip => "o clipe",
            Self::Marker => "o marcador",
            Self::Object => "o objeto",
            Self::Surface => "a superfície",
        }
    }

    /// The help line the types and the reference give a row that takes one.
    pub fn checked(self) -> &'static str {
        match self {
            Self::Sound => "A conferência avisa de um som que o projeto não tem.",
            Self::Variable => "A conferência avisa de uma variável que o projeto não tem.",
            Self::Attribute => {
                "A conferência avisa de um atributo que nenhum objeto tem e nenhum script declara."
            }
            Self::Scene => "A conferência avisa de uma cena que o projeto não tem.",
            Self::Model => "A conferência avisa de um modelo que o projeto não tem.",
            Self::Action => "A conferência avisa de uma ação de entrada que o projeto não tem.",
            Self::Clip => "A conferência avisa de um clipe que nenhum objeto tem.",
            Self::Marker => "A conferência avisa de um marcador que nenhum clipe tem.",
            Self::Object => "A conferência avisa de um nome que nenhum objeto do projeto tem.",
            Self::Surface => "A conferência avisa de uma superfície que o projeto não tem.",
        }
    }
}

/// A parameter: its name, its Luau type (`?` when it may be left out), what it is, in
/// Portuguese, and what its text names in the project, if anything. `...` is the rest of the
/// arguments; a type ending in `...` is a generic pack.
#[derive(Clone, Copy, Debug)]
pub struct ApiParam {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
    pub names: Option<NameKind>,
}

const fn param(name: &'static str, ty: &'static str, doc: &'static str) -> ApiParam {
    ApiParam {
        name,
        ty,
        doc,
        names: None,
    }
}

/// A parameter whose text names something of the project, which the lint of names checks.
const fn named(
    name: &'static str,
    ty: &'static str,
    names: NameKind,
    doc: &'static str,
) -> ApiParam {
    ApiParam {
        name,
        ty,
        doc,
        names: Some(names),
    }
}

impl ApiParam {
    /// The texts its type allows, for a union of literal texts (`"music" | "effects"`), in its
    /// order; none for any other type.
    pub fn choices(&self) -> Option<Vec<&'static str>> {
        let ty = self.ty.strip_suffix('?').unwrap_or(self.ty);
        ty.split(" | ")
            .map(|choice| choice.strip_prefix('"')?.strip_suffix('"'))
            .collect()
    }
}

/// One row of the API.
#[derive(Clone, Copy, Debug)]
pub struct ApiFn {
    /// As scripts write it: `audio.play` and `print` for functions, `Object.position` for
    /// properties, `Object:after` for methods (`Character:jump` for a view's), `on_start` for
    /// callbacks, `attributes` for what a module declares (`Modulo.attributes`), `time.dt` for a
    /// value of a global table and `players` for a name kept for 0.8.
    pub name: &'static str,
    pub area: Area,
    pub kind: ApiKind,
    /// The type parameters, as `<X...>`; empty without.
    pub generics: &'static str,
    /// The parameters, after `self` for methods and callbacks.
    pub params: &'static [ApiParam],
    /// The Luau type it returns, `()` for nothing; a property's, a value's or a declaration's
    /// type.
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
    /// The table or type it belongs to (`audio`, `Object`, `Character`), or `Modulo` for
    /// callbacks, declarations and the callbacks kept for 0.8; empty for a function of the base
    /// and a global kept for 0.8.
    pub fn holder(&self) -> &'static str {
        match self.kind {
            ApiKind::Callback | ApiKind::Declaration => "Modulo",
            ApiKind::Reserved if self.name.starts_with("on_") => "Modulo",
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

    /// What it is a property or a method of; none for the other rows.
    pub fn view(&self) -> Option<View> {
        if !matches!(self.kind, ApiKind::Method | ApiKind::Property { .. }) {
            return None;
        }
        let holder = self.holder();
        View::ALL.into_iter().find(|view| view.name() == holder)
    }

    /// Whether objects themselves have it (a property or a method of `Object`).
    pub fn of_objects(&self) -> bool {
        self.view() == Some(View::Object)
    }

    /// What the texts it takes name in the project, in the order of its parameters: attributes
    /// for `obj.attr`, whose keys are their names, and surfaces for the properties a surface's
    /// name may be written to (`obj.collider.surface = "Gelo"`).
    pub fn names(&self) -> Vec<NameKind> {
        match self.imp {
            Imp::Property(handles::Property::Attr) => return vec![NameKind::Attribute],
            Imp::Property(handles::Property::Surface) => return vec![NameKind::Surface],
            _ => {}
        }
        let mut kinds: Vec<NameKind> = Vec::new();
        for kind in self.params.iter().filter_map(|p| p.names) {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        kinds
    }

    /// Whether it is `obj.attr`, the attributes by name.
    pub fn is_attributes(&self) -> bool {
        matches!(self.imp, Imp::Property(handles::Property::Attr))
    }

    /// Whether the types and the plan-B module declare it: every row but those luau-lsp types
    /// itself and the names kept for 0.8.
    pub fn in_types(&self) -> bool {
        !matches!(self.imp, Imp::Luau | Imp::Reserved) && !TYPED_BY_LUAU.contains(&self.name)
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
    /// The members of a view, its rows (`Character.` and `Character:`).
    View(View),
}

/// A field of a table type.
#[derive(Clone, Copy, Debug)]
pub struct ApiField {
    pub name: &'static str,
    pub ty: &'static str,
    pub doc: &'static str,
}

/// Rows luau-lsp types itself, which the types and the plan-B module leave out: a declaration
/// would hide what it knows (the type of the module `require` loads).
const TYPED_BY_LUAU: [&str; 1] = ["require"];

/// Names of members longer than this go in the last bucket of [`MEMBERS`].
const LONGEST: usize = 31;

/// The rows whose member names have one length (the last bucket, any longer), with the names.
type Bucket = Vec<(&'static [u8], &'static ApiFn)>;

/// The rows of each view, in the order of [`View::ALL`], by the length of their member names,
/// as the handles look them up on every access: properties before methods, and the attributes
/// first, the most read.
static MEMBERS: LazyLock<Vec<Vec<Bucket>>> = LazyLock::new(|| {
    View::ALL
        .into_iter()
        .map(|view| {
            let mut buckets = vec![Vec::new(); LONGEST + 1];
            let mut rows: Vec<&'static ApiFn> =
                API.iter().filter(|f| f.view() == Some(view)).collect();
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
        })
        .collect()
});

/// The property or method `view` has under `name`.
pub(crate) fn member(view: View, name: &[u8]) -> Option<&'static ApiFn> {
    MEMBERS[view as usize][name.len().min(LONGEST)]
        .iter()
        .find(|(member, _)| *member == name)
        .map(|(_, f)| *f)
}

/// The names `view` has, properties and methods.
pub(crate) fn members(view: View) -> impl Iterator<Item = &'static str> {
    API.iter()
        .filter(move |f| f.view() == Some(view))
        .map(|f| f.member())
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
    ("personagem", "character"),
    ("humanoid", "character"),
    ("colisor", "collider"),
    ("body", "collider"),
    ("plataforma", "platform"),
    ("controlador", "controller"),
    ("animacao", "animation"),
    ("animação", "animation"),
    ("animator", "animation"),
    ("interface", "ui"),
    ("gui", "ui"),
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

/// The row of a name kept for 0.8: a global (`players`), a member of a global table
/// (`game.online`) or a callback (`on_request`).
pub(crate) fn reserved(name: &str) -> Option<&'static ApiFn> {
    API.iter()
        .find(|f| f.kind == ApiKind::Reserved && f.name == name)
}

/// The callbacks the engine calls from 0.8.
pub(crate) fn later_callbacks() -> impl Iterator<Item = &'static str> {
    API.iter()
        .filter(|f| f.kind == ApiKind::Reserved && f.name.starts_with("on_"))
        .map(|f| f.name)
}

/// Why a name kept for 0.8 does nothing now.
pub(crate) fn later(name: &str) -> String {
    if name.starts_with("on_") {
        format!(
            "{name} chega na 0.8, com a rede e vários jogadores: na 0.7 a engine nunca chama \
             essa função"
        )
    } else {
        format!("{name} chega na 0.8, com a rede e vários jogadores; na 0.7 ainda não existe")
    }
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
    /// functions and values of the global tables and the methods of objects and of their views;
    /// and every row is there.
    #[test]
    fn nothing_scripts_reach_is_outside_the_table() {
        let host = ScriptHost::new(std::sync::Arc::default()).unwrap();
        let lua = host.lua();
        let base: Table = lua.named_registry_value("oxy.base").unwrap();
        let luau = stock();
        let standard = keys(&luau.globals());
        let installed = |f: &ApiFn| {
            matches!(f.kind, ApiKind::Function | ApiKind::Constant) && !matches!(f.imp, Imp::Luau)
        };
        let mut holders: BTreeSet<&str> = BTreeSet::new();
        for f in API.iter().filter(|f| installed(f)) {
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
                        .filter(|f| installed(f) && f.holder() == *holder)
                        .map(|f| f.member().to_owned())
                        .collect();
                    assert_eq!(keys(&table), rows, "{holder}");
                }
                LuaValue::Function(_) => assert!(find(holder).is_some(), "{holder}"),
                other => panic!("{holder}: {other:?}"),
            }
        }
        // The functions Luau keeps and the names of 0.8 are not installed: the first are Luau's
        // own, the second only refuse.
        for f in API.iter().filter(|f| matches!(f.imp, Imp::Luau)) {
            assert!(standard.contains(f.name), "{}", f.name);
        }
        for f in API.iter().filter(|f| f.kind == ApiKind::Reserved) {
            if f.holder().is_empty() {
                assert!(
                    base.raw_get::<LuaValue>(f.name).unwrap().is_nil(),
                    "{}",
                    f.name
                );
            }
        }
        // The methods, by the mark of the handles they belong to.
        let methods: Table = lua.named_registry_value("oxy.methods").unwrap();
        for view in View::ALL {
            let rows: BTreeSet<String> = API
                .iter()
                .filter(|f| f.kind == ApiKind::Method && f.view() == Some(view))
                .map(|f| f.member().to_owned())
                .collect();
            let table: Table = methods.raw_get(view.mark()).unwrap();
            assert_eq!(keys(&table), rows, "{view:?}");
        }
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
                    find(&name).is_some_and(|f| !matches!(f.imp, Imp::Luau))
                        || sandbox::SANDBOXED.contains(&name.as_str()),
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
        // What the rows say Luau keeps, the engine keeps.
        assert!(!replaced.contains("error") && !replaced.contains("assert"));
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
                // A function read every step shows `on_step`.
                (ApiKind::Function, Imp::Function(_)) => {
                    !f.example_at_top()
                        || f.holder() == "param"
                        || f.example.starts_with("function M.on_")
                }
                (ApiKind::Function, Imp::Luau) => !f.example_at_top() && f.holder().is_empty(),
                // A method used in a callback shows the callback.
                (ApiKind::Method, Imp::Method(_)) => {
                    !f.example_at_top() || f.example.starts_with("function M.on_")
                }
                (ApiKind::Method, Imp::FastMethod(_)) => f.of_objects() && !f.example_at_top(),
                (ApiKind::Property { write }, Imp::Property(property)) => {
                    write == property.writable() && !f.example_at_top()
                }
                (ApiKind::Callback, Imp::Callback(callback)) => {
                    callback.name() == f.name && f.example_at_top()
                }
                (ApiKind::Declaration, Imp::Declaration) => f.example_at_top(),
                (ApiKind::Constant, Imp::Constant(_)) => {
                    !f.example_at_top() && !f.holder().is_empty()
                }
                (ApiKind::Reserved, Imp::Reserved) => {
                    f.example_at_top() == f.name.starts_with("on_")
                }
                _ => false,
            };
            assert!(fits, "{}: {:?} com {:?}", f.name, f.kind, f.imp);
            // Members belong to a view, by the name before the dot or the colon.
            let member = matches!(f.kind, ApiKind::Method | ApiKind::Property { .. });
            assert_eq!(f.view().is_some(), member, "{}", f.name);
            if let Some(view) = f.view() {
                let separator = if f.kind == ApiKind::Method { ':' } else { '.' };
                assert!(
                    f.name.starts_with(&format!("{}{separator}", view.name())),
                    "{}",
                    f.name
                );
            }
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
        // Every view has members, and every view but objects is a property of objects or of a
        // global function's result.
        for view in View::ALL {
            assert!(members(view).next().is_some(), "{view:?}");
            assert_eq!(View::of_mark(view.mark()), Some(view));
        }
        assert!(
            member(View::Object, b"position").is_some()
                && member(View::Object, b"posicao").is_none()
        );
        assert!(member(View::Character, b"teleport").is_some());
        // Every alias names a member of objects.
        for (alias, member) in ALIASES {
            assert!(
                self::member(View::Object, member.as_bytes()).is_some(),
                "{alias}: {member}"
            );
            assert_eq!(alias.to_lowercase(), *alias);
        }
        // The names of 0.8 are the design's (§5.4), and none is a row of another kind.
        let reserved: Vec<&str> = API
            .iter()
            .filter(|f| f.kind == ApiKind::Reserved)
            .map(|f| f.name)
            .collect();
        assert_eq!(
            reserved,
            [
                "players",
                "Player",
                "session",
                "game.online",
                "game.is_host",
                "game.is_server",
                "on_request",
                "on_player_join",
                "on_player_leave"
            ]
        );
        assert_eq!(
            later_callbacks().collect::<Vec<_>>(),
            ["on_request", "on_player_join", "on_player_leave"]
        );
    }

    /// The parameters that take names of the project are marked, so the lint of names checks
    /// them and tools find them; they take texts, and the unions of texts parse as choices.
    #[test]
    fn the_names_of_the_project_the_rows_take_are_marked() {
        use NameKind::*;
        let named: Vec<(&str, &str, NameKind)> = API
            .iter()
            .flat_map(|f| {
                f.params
                    .iter()
                    .filter_map(move |p| Some((f.name, p.name, p.names?)))
            })
            .collect();
        assert_eq!(
            named,
            [
                ("on_input", "action", Action),
                ("on_marker", "marker", Marker),
                ("on_marker", "clip", Clip),
                ("Object:find_child", "name", Object),
                ("Object:get_child", "name", Object),
                ("scene.find", "name", Object),
                ("scene.get", "name", Object),
                ("scene.find_all", "name", Object),
                ("scene.with_attribute", "name", Attribute),
                ("scene.spawn_model", "model", Model),
                ("scene.change", "name", Scene),
                ("Object:has_attribute", "name", Attribute),
                ("param.surface", "name", Surface),
                ("Animation:play", "clip", Clip),
                ("audio.play", "name", Sound),
                ("audio.loop", "name", Sound),
                ("vars.get", "name", Variable),
                ("vars.set", "name", Variable),
                ("input.pressed", "action", Action),
                ("input.held", "action", Action),
                ("input.released", "action", Action),
            ]
        );
        // The properties: the keys of obj.attr, and the surfaces a surface's name may be
        // written to.
        let properties: Vec<(&str, Vec<NameKind>)> = API
            .iter()
            .filter(|f| matches!(f.kind, ApiKind::Property { .. }) && !f.names().is_empty())
            .map(|f| (f.name, f.names()))
            .collect();
        assert_eq!(
            properties,
            [
                ("Object.attr", vec![Attribute]),
                ("Character.body_surface", vec![Surface]),
                ("Collider.surface", vec![Surface]),
            ]
        );
        let mut kinds: BTreeSet<NameKind> = named.iter().map(|(_, _, kind)| *kind).collect();
        kinds.extend(properties.into_iter().flat_map(|(_, kinds)| kinds));
        assert_eq!(kinds.into_iter().collect::<Vec<_>>(), NameKind::ALL);
        assert_eq!(find("on_marker").unwrap().names(), [Marker, Clip]);
        for f in API {
            for p in f.params {
                if p.names.is_some() {
                    assert!(
                        ["string", "string?"].contains(&p.ty),
                        "{}: {}",
                        f.name,
                        p.name
                    );
                }
                if p.ty.starts_with('"') {
                    let choices = p.choices().unwrap_or_default();
                    assert!(choices.len() > 1, "{}: {}", f.name, p.name);
                }
            }
        }
        let mode = find("camera.set_mode").unwrap().params[1];
        assert_eq!(
            mode.choices().unwrap(),
            ["first_person", "third_person", "fixed", "fixed_object"]
        );
        assert_eq!(find("audio.play").unwrap().params[0].choices(), None);
    }

    /// Design §5.5: every operation of the node registry, frozen since 0.7.0, has its equivalent
    /// in scripts, a row of the API that says it does the same or plain Luau ([`SYNTAX`]).
    #[test]
    fn every_node_operation_has_its_equivalent_in_scripts() {
        let operations: Vec<&str> = crate::graph::registry().iter().map(|op| op.id).collect();
        assert_eq!(operations.len(), 86, "o registro de nós está congelado");
        let in_rows: BTreeSet<&str> = API.iter().flat_map(|f| f.nodes.iter().copied()).collect();
        let in_syntax: BTreeSet<&str> = SYNTAX.iter().map(|(op, _)| *op).collect();
        assert_eq!(in_syntax.len(), SYNTAX.len(), "operação repetida em SYNTAX");
        let unmapped: Vec<&str> = operations
            .iter()
            .copied()
            .filter(|op| !in_rows.contains(op) && !in_syntax.contains(op))
            .collect();
        assert!(
            unmapped.is_empty(),
            "operações de nós sem equivalente nos scripts: {unmapped:?}"
        );
        for op in in_syntax.iter().chain(&in_rows) {
            assert!(operations.contains(op), "{op} não é uma operação de nó");
        }
        // The checklist's 17 operations that plain Luau does: the constants, the arithmetic,
        // the comparison, Se / Senão, Sequência and the 8 of vectors.
        assert_eq!(SYNTAX.len(), 17);
        for (op, luau) in SYNTAX {
            assert!(luau.ends_with('.'), "{op}");
        }
        // The other 69 (66 functions and 3 references) are rows; the Vetor3 operations are
        // both, `vmath` doing what Luau's `vector` lacks.
        let only_rows = operations
            .iter()
            .filter(|op| in_rows.contains(*op) && !in_syntax.contains(*op))
            .count();
        assert_eq!(only_rows, 69);
    }

    /// The names a script gives where a node has a fixed choice are the node's choices, frozen
    /// with the graph, and the type of the row that takes them lists them, in the same order.
    #[test]
    fn the_names_scripts_choose_are_the_nodes_choices() {
        use crate::script::{camera, character, game};
        let union = |names: &[&str]| {
            names
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let ty = |row: &str, name: &str| {
            find(row)
                .unwrap()
                .params
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("{row}: {name}"))
                .ty
        };
        // "agile" is the generic name of "parkour", which the node keeps (design §5.1); the
        // types show only the generic one.
        let profiles: Vec<&str> = character::PROFILES
            .into_iter()
            .filter(|p| *p != "agile")
            .collect();
        let typed: Vec<&str> = character::PROFILES
            .into_iter()
            .filter(|p| *p != "parkour")
            .collect();
        // The names a script gives, the ones the types show, the node's operation and parameter,
        // and the rows and their parameters that take them.
        type Names<'a> = &'a [&'a str];
        type Choice<'a> = (
            Names<'a>,
            Names<'a>,
            &'a str,
            &'a str,
            &'a [(&'a str, &'a str)],
        );
        let checked: [Choice; 8] = [
            (
                &character::PARAMETERS,
                &character::PARAMETERS,
                "character.parameter",
                "parameter",
                &[
                    ("Character:set_param", "name"),
                    ("Character:get_param", "name"),
                ],
            ),
            (
                &character::OPTIONS,
                &character::OPTIONS,
                "character.option",
                "option",
                &[("Character:set_option", "name")],
            ),
            (
                &character::POSTURES,
                &character::POSTURES,
                "character.posture",
                "command",
                &[("Character:set_posture", "kind")],
            ),
            (
                &character::CHANNELS,
                &character::CHANNELS,
                "character.block",
                "channel",
                &[
                    ("Character:block", "channel"),
                    ("Character:block_reasons", "channel"),
                ],
            ),
            (
                &profiles,
                &typed,
                "character.profile",
                "profile",
                &[("Character:apply_profile", "profile")],
            ),
            (
                &camera::MODES,
                &camera::MODES,
                "camera.mode",
                "mode",
                &[("camera.set_mode", "mode")],
            ),
            (
                &camera::SETTINGS,
                &camera::SETTINGS,
                "camera.setting",
                "setting",
                &[("camera.set", "setting")],
            ),
            (
                &game::FORMATS,
                &game::FORMATS,
                "text.time",
                "format",
                &[("time.format", "format")],
            ),
        ];
        for (names, typed, operation, parameter, rows) in checked {
            let choices: BTreeSet<&str> = crate::graph::choices(operation, parameter)
                .unwrap()
                .iter()
                .map(|(id, _)| *id)
                .collect();
            assert_eq!(
                names.iter().copied().collect::<BTreeSet<_>>(),
                choices,
                "{operation}"
            );
            assert_eq!(names.len(), choices.len(), "{operation}");
            for (row, name) in rows {
                assert_eq!(ty(row, name), union(typed), "{row}");
            }
        }
        assert_eq!(rows::CHARACTER_PARAMETERS, union(&character::PARAMETERS));
    }
}
