//! The shared types of the API, which the definitions, the plan-B module and the reference
//! declare before the globals.
use super::{ApiField, ApiType, TypeDef, View};

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
    ApiType {
        name: "Character",
        generics: "",
        doc: "O Personagem 3D de um objeto, por `obj.character`: o estado e os comandos dele.",
        def: TypeDef::View(View::Character),
    },
    ApiType {
        name: "Collider",
        generics: "",
        doc: "O colisor (2D, 3D ou área) de um objeto, por `obj.collider`.",
        def: TypeDef::View(View::Collider),
    },
    ApiType {
        name: "Platform",
        generics: "",
        doc: "A plataforma móvel de um objeto, por `obj.platform`.",
        def: TypeDef::View(View::Platform),
    },
    ApiType {
        name: "Controller",
        generics: "",
        doc: "O Movimento 2D de um objeto, por `obj.controller`.",
        def: TypeDef::View(View::Controller),
    },
    ApiType {
        name: "Animation",
        generics: "",
        doc: "As animações (clipes) de um objeto, por `obj.animation`.",
        def: TypeDef::View(View::Animation),
    },
    ApiType {
        name: "UiElement",
        generics: "",
        doc: "O elemento de interface de um objeto, por `obj.ui`.",
        def: TypeDef::View(View::Ui),
    },
    ApiType {
        name: "RandomStream",
        generics: "",
        doc: "Uma sequência de sorteios com nome, de `random.stream`: independente das outras.",
        def: TypeDef::View(View::Stream),
    },
    ApiType {
        name: "TeleportOptions",
        generics: "",
        doc: "Opções de `obj.character:teleport`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "search",
                ty: "number?",
                doc: "Até quantos metros procurar um lugar livre perto do destino, de 0 a 2; 0 \
                      (o padrão) recusa um destino ocupado.",
            },
            ApiField {
                name: "keep_velocity",
                ty: "boolean?",
                doc: "true mantém a velocidade total; sem ela, o personagem chega parado.",
            },
            ApiField {
                name: "yaw",
                ty: "number?",
                doc: "O giro do corpo no mundo, em graus (0 olha para -Z); sem ele, fica o de \
                      agora.",
            },
            ApiField {
                name: "look",
                ty: "vector?",
                doc: "O olhar, como `character.look` dá: x o giro e y a inclinação, em graus; \
                      sem ele, fica o de agora.",
            },
        ]),
    },
    ApiType {
        name: "Hit",
        generics: "",
        doc: "O primeiro contato de uma consulta de `physics`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "object",
                ty: "Object",
                doc: "O objeto atingido.",
            },
            ApiField {
                name: "point",
                ty: "vector",
                doc: "O ponto do contato, no mundo, em metros.",
            },
            ApiField {
                name: "normal",
                ty: "vector",
                doc: "A normal da superfície atingida, no mundo.",
            },
            ApiField {
                name: "distance",
                ty: "number",
                doc: "A distância percorrida até o contato, em metros (em `capsule_fits`, a \
                      penetração, negativa).",
            },
            ApiField {
                name: "fraction",
                ty: "number",
                doc: "A fração do percurso até o contato, de 0 a 1.",
            },
            ApiField {
                name: "surface",
                ty: "Surface?",
                doc: "A superfície física do que foi atingido, ou nil para a padrão.",
            },
        ]),
    },
    ApiType {
        name: "QueryOptions",
        generics: "",
        doc: "Opções das consultas de `physics`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "areas",
                ty: "boolean?",
                doc: "true inclui as áreas (sensores); sem ela, só o que colide.",
            },
            ApiField {
                name: "exclude",
                ty: "AnyObject?",
                doc: "Um objeto que a consulta ignora, com tudo o que está nele: o próprio \
                      personagem, para não se atingir.",
            },
            ApiField {
                name: "category",
                ty: "(number | string | { string })?",
                doc: "Os grupos de colisão a que a consulta pertence: os nomes dos grupos (como \
                      no editor) ou a máscara em número (padrão 1, Cenário padrão).",
            },
            ApiField {
                name: "mask",
                ty: "(number | string | { string })?",
                doc: "Os grupos de colisão que a consulta encontra: os nomes ou a máscara em \
                      número (padrão: todos).",
            },
        ]),
    },
    ApiType {
        name: "LoopOptions",
        generics: "",
        doc: "Opções de `audio.loop`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "channel",
                ty: "(\"music\" | \"effects\")?",
                doc: "O canal: \"music\" (o padrão) ou \"effects\".",
            },
            ApiField {
                name: "volume",
                ty: "number?",
                doc: "Volume de 0 a 1 (padrão 1).",
            },
            ApiField {
                name: "fade",
                ty: "number?",
                doc: "A entrada suave, em segundos, de 0 a 60 (padrão 0).",
            },
        ]),
    },
    ApiType {
        name: "StopOptions",
        generics: "",
        doc: "Opções de `audio.stop`.",
        def: TypeDef::Table(&[
            ApiField {
                name: "sound",
                ty: "string?",
                doc: "O som que para, pelo nome na Biblioteca; sem ele, todos os do canal.",
            },
            ApiField {
                name: "channel",
                ty: "(\"music\" | \"effects\" | \"all\")?",
                doc: "O canal: \"music\", \"effects\" ou \"all\" (o padrão), os dois.",
            },
            ApiField {
                name: "fade",
                ty: "number?",
                doc: "A saída suave, em segundos, de 0 a 60 (padrão 0).",
            },
        ]),
    },
];
