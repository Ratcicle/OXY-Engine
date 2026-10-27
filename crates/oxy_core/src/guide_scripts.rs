//! The Guide's section on scripts (design §6.7, piece 5): the pages that teach a person, or an
//! agent, to write a script; the reference of the API, made from its registry as
//! docs/api-scripts.md is; and the search of the whole Guide (scripts first, the nodes after).
//! Every code block of a page is a whole script, kept in `guide_scripts/`, that a test loads and
//! runs.
use crate::script::generate::{self, GuideEntry, GuidePage};
use crate::script::registry::{self, Area};
use crate::{graph, guide, guide_recipes};
use std::sync::LazyLock;

/// A piece of a page. In its texts, a name in backticks reads as code and `**text**` stands out.
#[derive(Clone, Copy, Debug)]
pub enum Block {
    Text(&'static str),
    /// A numbered step: the steps of a page count from 1, in order.
    Step(&'static str),
    /// A whole script, by its file name.
    Code {
        file: &'static str,
        code: &'static str,
    },
    /// A title that opens a part of the page.
    Heading(&'static str),
}

pub struct Page {
    /// The page as the Guide's list names it.
    pub title: &'static str,
    /// The title on the page.
    pub heading: &'static str,
    pub blocks: &'static [Block],
}

macro_rules! code {
    ($file:literal) => {
        Block::Code {
            file: $file,
            code: include_str!(concat!("guide_scripts/", $file)),
        }
    };
}

static PAGES: [Page; 4] = [
    Page {
        title: "Primeiro script",
        heading: "Seu primeiro script",
        blocks: &[
            Block::Text(
                "Um script é um arquivo `.luau` do projeto que dá comportamento a objetos. Quem \
                 escreve pode ser você, no VS Code, ou um agente de IA ligado à OXY; a OXY anexa, \
                 roda, mostra os erros e recarrega quando o arquivo muda.",
            ),
            Block::Step(
                "Crie `girar.luau` em qualquer pasta do projeto, ou peça ao agente: “um script que \
                 gira o objeto, com a velocidade num atributo”.",
            ),
            code!("girar.luau"),
            Block::Step(
                "Selecione um objeto e, em Propriedades, use **Scripts › + Anexar script**. O \
                 atributo Velocidade aparece em Atributos com a nota “pedido por girar.luau”.",
            ),
            Block::Step(
                "Use Jogar: o objeto gira 90 graus por segundo. Com o teste rodando, mude o código \
                 e salve o arquivo: o script recarrega entre dois passos, sem parar o teste. A \
                 Velocidade se muda em Atributos, com o teste parado.",
            ),
            Block::Step(
                "Um erro vai para o Console com o arquivo e a linha (`girar.luau:6`); clique para \
                 ver o código naquela linha. Só aquela chamada para: o resto do jogo continua.",
            ),
            Block::Heading("Como um script funciona"),
            Block::Text(
                "O arquivo devolve uma tabela, o módulo. As funções com nome de evento \
                 (`on_start`, `on_step`, `on_area_enter`…) a engine chama sozinha, e `self` é o \
                 objeto que anexa o script. Um script serve a vários objetos: os 6 checkpoints do \
                 parkour usam um arquivo só, cada um com a sua Ordem.",
            ),
            Block::Text(
                "Os nomes da API são em inglês; os de objetos, atributos, sons e ações são os do \
                 projeto, em português. A lista completa está em Referência da API, aqui ao lado.",
            ),
        ],
    },
    Page {
        title: "Onde guardar valores",
        heading: "Onde guardar valores",
        blocks: &[
            Block::Text(
                "O que precisa durar de uma chamada para outra fica na engine, nunca em variáveis \
                 do próprio script: assim o valor atravessa a recarga do arquivo e, na 0.8, a rede. \
                 Há três lugares, e quem escolhe é quem lê o valor.",
            ),
            Block::Text(
                "**Atributo** (`self.attr.Vida`): o que outro objeto, a interface ou um grafo de \
                 nós leem. Aparece nas Propriedades; o script declara os que espera em \
                 `Modulo.attributes`, e anexá-lo cria os que faltam.",
            ),
            Block::Text(
                "**Variável do jogo** (`vars.get`, `vars.set`): o que vale para o jogo inteiro e \
                 continua quando a cena muda ou recomeça, como pontos ou um recorde. Ela se cria \
                 em Lógica › Variáveis do jogo; as marcadas como Salva entre partidas vão para o \
                 progresso com `save.write()`.",
            ),
            Block::Text(
                "**Memória do script** (`self.state`): o que só este script lê, neste objeto: \
                 contagens, listas, o momento de agir. Os campos e os valores iniciais se declaram \
                 em `Modulo.state`.",
            ),
            code!("contador.luau"),
            Block::Text(
                "Um `local` do topo do arquivo que muda dentro de uma função é recusado ao \
                 carregar, com a linha: o valor se perderia na recarga. Constantes no topo podem \
                 (`local ALTURA = 1.5`).",
            ),
            Block::Text(
                "Guarde momentos, não contagens: `self.state.pronto_em = time.now() + 0.5` muda \
                 uma vez, enquanto somar `dt` a cada passo muda sempre. O primeiro nível da \
                 memória não se percorre (`pairs(self.state)` é um erro): guarde listas num \
                 campo, como `nomes` acima.",
            ),
        ],
    },
    Page {
        title: "Esperar, repetir e mensagens",
        heading: "Esperar, repetir e mensagens",
        blocks: &[
            Block::Text(
                "Um script nunca fica parado esperando: não há `wait()`. Para agir depois, peça um \
                 temporizador: `self:after(2, M.fechar)` chama `M.fechar(self)` dois segundos de \
                 jogo depois. A engine guarda o nome da função e os dados, conta o tempo de jogo, \
                 que para na pausa, e o temporizador atravessa a recarga.",
            ),
            Block::Text(
                "A função tem de ser uma do módulo, como `M.fechar`, nunca uma criada na hora. \
                 `self:every` repete até `self:cancel(id)`; `self:sequence` chama a função com cada \
                 item de uma lista, um por intervalo; `self:schedule` dá a cada passo o próprio \
                 atraso. Os temporizadores são do próprio objeto e caem quando ele sai, quando a \
                 cena troca e quando o script para.",
            ),
            Block::Text(
                "Para avisar outro objeto, mande uma mensagem: `alvo:send(\"abrir\", 3)`. Os \
                 scripts dele a recebem em `on_message(self, name, value, from)`, na fase \
                 seguinte da lógica, com uma cópia do valor.",
            ),
            code!("alavanca.luau"),
            code!("porta.luau"),
            Block::Text(
                "Com a Porta escolhida no atributo Porta da Alavanca, um clique na Alavanca libera \
                 a passagem por 3 segundos. A Porta precisa de um colisor; a Alavanca, de ser \
                 clicável (um objeto visual ou um botão de interface).",
            ),
        ],
    },
    Page {
        title: "Erros e recarga",
        heading: "Erros e recarga",
        blocks: &[
            Block::Text(
                "Quando um script erra, só aquela chamada para: o que ela fez até ali fica, e o \
                 resto do jogo segue, inclusive os outros objetos com o mesmo script. É igual no \
                 teste do editor e no jogo exportado.",
            ),
            Block::Text(
                "O erro aparece uma vez no Console, com o objeto, o arquivo e a linha \
                 (`porta.luau:8`), e um contador (×37) quando se repete no mesmo lugar. Clique na \
                 linha para ver o código, com o erro embaixo dela. O cartão Scripts do objeto e a \
                 marca { } da Hierarquia também avisam.",
            ),
            Block::Text(
                "Um laço sem fim, ou um limite passado (trabalho demais numa chamada, \
                 temporizadores ou mensagens demais, a memória do script acima de 64 KB), desliga \
                 o script naquele objeto. Ele volta quando o arquivo é salvo de novo ou quando a \
                 cena recomeça.",
            ),
            Block::Text(
                "Para o que nunca deveria acontecer, `error(\"texto\")` e `assert(valor, \
                 \"texto\")` param a chamada com a mensagem na linha. Uma falha esperada, como um \
                 teleporte sem espaço, se confere pelo que a função devolve.",
            ),
            Block::Heading("Recarga"),
            Block::Text(
                "Com o teste rodando, salvar o arquivo recarrega o script entre dois passos, mesmo \
                 com o teste pausado, e o Console diz “Script recarregado: girar.luau (1 \
                 objeto)”. Atributos, variáveis, a memória dos scripts e os temporizadores \
                 continuam como estavam; um campo novo em `Modulo.state` nasce com o valor \
                 declarado.",
            ),
            Block::Text(
                "Se o arquivo novo tem erro, o teste segue com a versão anterior, e o Console \
                 mostra a linha. Um atributo que o script passou a declarar só é criado ao \
                 anexá-lo de novo, depois do teste.",
            ),
            Block::Text(
                "Um agente faz o mesmo pelo MCP: `oxy_scripts` grava e confere o arquivo, o \
                 `attach_script` do `oxy_apply` anexa, e o `oxy_console` e o `oxy_simulate` trazem \
                 os erros com o arquivo e a linha.",
            ),
        ],
    },
];

/// The pages of Scripts, in the Guide's order: the first is the one the Guide opens at.
pub fn pages() -> &'static [Page] {
    &PAGES
}

/// The reference of the API as the Guide shows it: one page per area with rows, in the areas'
/// order, with every row of the registry in its order, made once (see `generate::guide`).
pub fn reference() -> &'static [GuidePage] {
    static REFERENCE: LazyLock<Vec<GuidePage>> = LazyLock::new(generate::guide);
    &REFERENCE
}

/// How many rows the reference has.
pub fn functions() -> usize {
    reference().iter().map(|page| page.entries.len()).sum()
}

/// The reference's page of `area`; none for an area without rows.
pub fn reference_page(area: Area) -> Option<&'static GuidePage> {
    reference().iter().find(|page| page.area == area)
}

/// The row named `name` and its page.
pub fn entry(name: &str) -> Option<(&'static GuidePage, &'static GuideEntry)> {
    reference().iter().find_map(|page| {
        page.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| (page, entry))
    })
}

/// A row as a script writes it where it is used: its signature up to the type parameters, the
/// parameters or the type (`obj:after`, `obj.character:teleport`, `audio.play`, `obj.position`,
/// `on_start`, `Modulo.attributes`).
pub fn call_name(entry: &GuideEntry) -> &str {
    let signature = entry.signature.as_str();
    let end = [
        signature.find('('),
        signature.find('<'),
        signature.find(": "),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(signature.len());
    &signature[..end]
}

/// The nodes that do what the row does, by their labels.
pub fn node_labels(entry: &GuideEntry) -> Vec<&'static str> {
    let ids = registry::find(entry.name).map_or(&[][..], |f| f.nodes);
    ids.iter()
        .map(|id| {
            graph::registry()
                .iter()
                .find(|op| op.id == *id)
                .map_or(*id, |op| op.label)
        })
        .collect()
}

/// What the search finds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    /// A page of Scripts, by its index in [`pages`].
    Page(usize),
    /// A recipe in script, by its index in `guide_recipes::script_recipes`.
    ScriptRecipe(usize),
    /// A row of the reference, by its area and name.
    Function { area: Area, name: &'static str },
    /// A recipe in nodes, by its index in `guide_recipes::recipes`.
    Recipe(usize),
    /// A node, by its operation.
    Node(&'static str),
}

/// `text` as the search compares it: in lowercase, without accents.
fn fold(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// What the search looks in, made once: each subject with its name and the rest of its text,
/// folded, in the Guide's order.
struct Subject {
    found: Found,
    name: String,
    text: String,
}

static SUBJECTS: LazyLock<Vec<Subject>> = LazyLock::new(|| {
    let mut subjects = Vec::new();
    let mut add = |found: Found, name: &str, text: &[&str]| {
        subjects.push(Subject {
            found,
            name: fold(name),
            text: fold(&text.join(" ")),
        });
    };
    for (index, page) in PAGES.iter().enumerate() {
        let texts: Vec<&str> = page
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Text(text) | Block::Step(text) | Block::Heading(text) => Some(*text),
                Block::Code { .. } => None,
            })
            .chain([page.heading])
            .collect();
        add(Found::Page(index), page.title, &texts);
    }
    for (index, recipe) in guide_recipes::script_recipes().iter().enumerate() {
        add(
            Found::ScriptRecipe(index),
            recipe.title,
            &[recipe.setup, recipe.script],
        );
    }
    for page in reference() {
        for entry in &page.entries {
            // A member of objects also by the name the pages and the examples write: `self:after`.
            let call = call_name(entry);
            let name = match call
                .strip_prefix("obj")
                .filter(|m| m.starts_with(['.', ':']))
            {
                Some(member) => format!("{} {call} self{member}", entry.name),
                None => format!("{} {call}", entry.name),
            };
            let params: Vec<&str> = entry.params.iter().map(|p| p.doc).collect();
            let mut text = vec![entry.summary, entry.doc];
            text.extend(params);
            add(
                Found::Function {
                    area: page.area,
                    name: entry.name,
                },
                &name,
                &text,
            );
        }
    }
    for (index, recipe) in guide_recipes::recipes().iter().enumerate() {
        add(Found::Recipe(index), recipe.title, &[recipe.setup]);
    }
    for topic in guide::topics() {
        let op = topic.operation;
        add(Found::Node(op.id), op.label, &[op.category, topic.purpose]);
    }
    subjects
});

/// What the Guide has on `query`, without regard to case or accents: the pages of Scripts, the
/// recipes in script, the rows of the API (by name, `after`, `self:after` or `teleport`, and by
/// the help), the recipes in nodes and the nodes, in that order; the rows whose names have the
/// query come before those whose help has it. Nothing for an empty query.
pub fn search(query: &str) -> Vec<Found> {
    let query = fold(query.trim());
    if query.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u8, Found)> = SUBJECTS
        .iter()
        .filter_map(|subject| {
            let named = subject.name.contains(&query);
            if !named && !subject.text.contains(&query) {
                return None;
            }
            let rank = match subject.found {
                Found::Page(_) => 0,
                Found::ScriptRecipe(_) => 1,
                Found::Function { .. } if named => 2,
                Found::Function { .. } => 3,
                Found::Recipe(_) => 4,
                Found::Node(_) => 5,
            };
            Some((rank, subject.found))
        })
        .collect();
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, found)| found).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::registry::{API, ApiKind, View};
    use std::collections::BTreeSet;

    #[test]
    fn the_pages_of_scripts_come_in_the_approved_order_and_every_script_is_whole() {
        let titles: Vec<&str> = pages().iter().map(|page| page.title).collect();
        assert_eq!(
            titles,
            [
                "Primeiro script",
                "Onde guardar valores",
                "Esperar, repetir e mensagens",
                "Erros e recarga"
            ]
        );
        // The first page shows girar.luau after its first step, as the prototype has it.
        assert!(matches!(
            &pages()[0].blocks[..3],
            [
                Block::Text(_),
                Block::Step(_),
                Block::Code {
                    file: "girar.luau",
                    ..
                }
            ]
        ));
        let steps = pages()[0]
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Step(_)))
            .count();
        assert_eq!(steps, 4);
        for block in pages().iter().flat_map(|page| page.blocks) {
            if let Block::Code { file, code } = block {
                assert_eq!(code.lines().next(), Some("--!strict"), "{file}");
                crate::script::check(file, code).unwrap();
            }
        }
    }

    /// The members of objects a text may name: the rows of `Object`, its views' properties
    /// (`obj.character`) and the module's own functions are rows too.
    fn object_member(name: &str) -> bool {
        API.iter()
            .any(|f| f.view() == Some(View::Object) && f.member() == name)
    }

    /// Every name of the API the pages write in backticks is a row of the registry: a member
    /// of `self`, `obj` or `other` (`self:after`, `self.state`), a member of a global table
    /// (`vars.get`, `time.now`), a field of the module (`Modulo.state`) or a callback
    /// (`on_message`). Names of the module itself (`M.fechar`), files (`girar.luau`) and Luau's
    /// own functions are left alone.
    #[test]
    fn the_names_of_the_api_the_pages_write_are_rows() {
        let holders: BTreeSet<&str> = API
            .iter()
            .filter(|f| matches!(f.kind, ApiKind::Function | ApiKind::Constant))
            .map(|f| f.holder())
            .filter(|holder| !holder.is_empty())
            .collect();
        let mut checked = 0;
        for block in pages().iter().flat_map(|page| page.blocks) {
            let text = match block {
                Block::Text(text) | Block::Step(text) | Block::Heading(text) => *text,
                Block::Code { .. } => continue,
            };
            for code in text.split('`').skip(1).step_by(2) {
                let words: Vec<&str> = code
                    .split(|c: char| !(c.is_alphanumeric() || "_.:".contains(c)))
                    .filter(|word| !word.is_empty())
                    .collect();
                for word in words {
                    let parts: Vec<&str> = word.split(['.', ':']).collect();
                    match parts.as_slice() {
                        ["self" | "obj" | "other", member, ..] => {
                            assert!(object_member(member), "{word}");
                        }
                        [holder, member, ..] if holders.contains(holder) => {
                            assert!(
                                registry::find(&format!("{holder}.{member}")).is_some(),
                                "{word}"
                            );
                        }
                        ["Modulo", member] => {
                            let row = registry::find(member);
                            assert!(
                                row.is_some_and(|f| f.kind == ApiKind::Declaration),
                                "{word}"
                            );
                        }
                        [_, member] if word.contains(':') && member.parse::<u32>().is_err() => {
                            assert!(object_member(member), "{word}");
                        }
                        [callback] if callback.starts_with("on_") => {
                            let row = registry::find(callback);
                            assert!(row.is_some_and(|f| f.kind == ApiKind::Callback), "{word}");
                        }
                        _ => continue,
                    }
                    checked += 1;
                }
            }
        }
        // self:after, vars.get, Modulo.state, on_message…: the pages name the API often.
        assert!(checked > 20, "{checked}");
    }

    #[test]
    fn the_reference_has_every_row_of_the_registry_once_with_the_counts_of_its_areas() {
        let mut names: Vec<&str> = reference()
            .iter()
            .flat_map(|page| page.entries.iter().map(|entry| entry.name))
            .collect();
        assert_eq!(names.len(), API.len());
        assert_eq!(functions(), API.len());
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), API.len());
        let areas: Vec<Area> = reference().iter().map(|page| page.area).collect();
        let expected: Vec<Area> = Area::ALL
            .into_iter()
            .filter(|area| API.iter().any(|f| f.area == *area))
            .collect();
        assert_eq!(areas, expected);
        for page in reference() {
            let rows: Vec<&str> = API
                .iter()
                .filter(|f| f.area == page.area)
                .map(|f| f.name)
                .collect();
            let shown: Vec<&str> = page.entries.iter().map(|entry| entry.name).collect();
            assert_eq!(shown, rows, "{}", page.title);
            assert_eq!(page.title, page.area.title());
        }
        let timers = reference_page(Area::Timers).unwrap();
        assert_eq!(timers.entries.len(), 5);
        let (page, after) = entry("Object:after").unwrap();
        assert_eq!(page.area, Area::Timers);
        assert_eq!(call_name(after), "obj:after");
        assert_eq!(node_labels(after), ["Esperar sem bloquear"]);
        let call = |name| call_name(entry(name).unwrap().1);
        assert_eq!(call("Character:teleport"), "obj.character:teleport");
        assert_eq!(call("Object.position"), "obj.position");
        assert_eq!(call("on_start"), "on_start");
        assert_eq!(call("attributes"), "Modulo.attributes");
        assert_eq!(call("time.dt"), "time.dt");
        assert_eq!(call("audio.play"), "audio.play");
        assert!(node_labels(entry("Object:cancel").unwrap().1).is_empty());
    }

    #[test]
    fn the_search_finds_pages_recipes_functions_by_name_and_help_and_nodes() {
        assert!(search("  ").is_empty());
        let functions = |query: &str| -> Vec<&'static str> {
            search(query)
                .into_iter()
                .filter_map(|found| match found {
                    Found::Function { name, .. } => Some(name),
                    _ => None,
                })
                .collect()
        };
        // By name: the row named so comes first, before those whose help names it.
        assert_eq!(functions("after")[0], "Object:after");
        // And as the pages write it, on `self`.
        assert_eq!(functions("self:after")[0], "Object:after");
        assert_eq!(functions("self.state")[0], "Object.state");
        assert!(search("after").contains(&Found::Function {
            area: Area::Timers,
            name: "Object:after"
        }));
        let teleport = search("teleport");
        assert!(teleport.contains(&Found::Function {
            area: Area::Character,
            name: "Character:teleport"
        }));
        let node = graph::registry()
            .iter()
            .find(|op| op.label == "Teleportar com segurança")
            .unwrap();
        assert!(teleport.contains(&Found::Node(node.id)));
        // Rows come before nodes, as the Guide lists them.
        let at = |found: &Found| teleport.iter().position(|f| f == found).unwrap();
        assert!(
            at(&Found::Function {
                area: Area::Character,
                name: "Character:teleport"
            }) < at(&Found::Node(node.id))
        );
        // By the help in Portuguese, without regard to case or accents.
        assert!(functions("ATRAVESSA A RECARGA").contains(&"Object:after"));
        assert!(functions("memoria deste script").contains(&"Object.state"));
        // Pages and recipes, both kinds.
        assert_eq!(search("guardar valores")[0], Found::Page(1));
        assert!(search("girar.luau").contains(&Found::Page(0)));
        let door = search("porta com chave");
        assert_eq!(door[0], Found::ScriptRecipe(0));
        assert!(door.contains(&Found::Recipe(1)));
        assert!(search("Esperar sem bloquear").contains(&Found::Node("control.wait")));
        assert!(search("zzzz nada").is_empty());
    }

    #[test]
    fn every_recipe_in_script_shows_each_script_its_project_runs() {
        for (index, recipe) in guide_recipes::script_recipes().iter().enumerate() {
            let (project, sources) = guide_recipes::load_script_recipe(index).unwrap();
            let shown: BTreeSet<&str> = recipe.scripts.iter().map(|(path, _)| *path).collect();
            let attached: BTreeSet<String> = crate::script::attached_paths(&project);
            assert!(!shown.is_empty(), "{}", recipe.title);
            assert_eq!(
                shown,
                attached.iter().map(String::as_str).collect(),
                "{}",
                recipe.title
            );
            for (path, text) in recipe.scripts {
                assert_eq!(sources.get(path).unwrap().text, *text, "{path}");
                assert!(recipe.setup.contains(path), "{}: {path}", recipe.title);
            }
        }
    }
}
