//! Typed native movement operations. Parameters are editable defaults, never demo rules.
use super::*;
fn op(
    id: &'static str,
    label: &'static str,
    category: &'static str,
    action: bool,
    params: Vec<ParamDef>,
    outputs: Vec<PortDef>,
) -> OperationDef {
    let mut inputs: Vec<_> = params
        .iter()
        .filter(|p| !matches!(p.default, Value::Text(_)))
        .map(|p| port(p.id, p.label, value_type(&p.default)))
        .collect();
    let mut outputs = outputs;
    if action {
        inputs.insert(0, port("exec", "Executar", PortType::Exec));
        outputs.insert(0, port("exec", "Continuar", PortType::Exec));
    }
    OperationDef {
        id,
        label,
        category,
        inputs,
        outputs,
        params,
    }
}
fn target() -> ParamDef {
    param("target", "Objeto", Value::Object(None))
}
fn v3(id: &'static str, label: &'static str, value: [f32; 3]) -> ParamDef {
    param(id, label, Value::Vector3(value))
}
fn flag(id: &'static str, label: &'static str, value: bool) -> ParamDef {
    param(id, label, Value::Bool(value))
}
fn state_ports() -> Vec<PortDef> {
    use PortType::*;
    vec![
        port("position", "Posição mundial (m)", Vector3),
        port("velocity", "Velocidade total (m/s)", Vector3),
        port("relative", "Velocidade própria (m/s)", Vector3),
        port("horizontal", "Velocidade horizontal (m/s)", Vector3),
        port("speed", "Rapidez horizontal (m/s)", Number),
        port("grounded", "No chão", Bool),
        port("rising", "Subindo", Bool),
        port("falling", "Caindo", Bool),
        port("posture", "Postura", Text),
        port("support", "Apoio", Object),
        port("surface", "Superfície", Surface),
        port("point", "Ponto de apoio (m)", Vector3),
        port("normal", "Normal de apoio", Vector3),
        port("support_velocity", "Velocidade do apoio (m/s)", Vector3),
        port("movement_blocked", "Movimento bloqueado", Bool),
        port("look_blocked", "Olhar bloqueado", Bool),
        port("movement_reasons", "Motivos de movimento", Text),
        port("look_reasons", "Motivos de olhar", Text),
    ]
}
pub(super) fn definitions() -> Vec<OperationDef> {
    use PortType::*;
    let mut out = vec![
        OperationDef {
            id: "event.step",
            label: "A cada passo da simulação",
            category: "Entrada e tempo",
            inputs: vec![],
            outputs: vec![
                port("exec", "Executar", Exec),
                port("self", "Responsável", Object),
                port("dt", "Duração do passo (s)", Number),
                port("time", "Tempo da simulação (s)", Number),
            ],
            params: vec![],
        },
        OperationDef {
            id: "event.character",
            label: "Evento do personagem 3D",
            category: "Personagem 3D",
            inputs: vec![],
            outputs: {
                let mut p = vec![
                    port("exec", "Executar", Exec),
                    port("context", "Personagem do evento", Object),
                    port("self", "Responsável", Object),
                    port("impact", "Rapidez do impacto (m/s)", Number),
                    port("contact", "Objeto do contato lateral", Object),
                    port("previous_surface", "Superfície anterior", Surface),
                ];
                p.extend(state_ports());
                p
            },
            params: vec![target(), text("kind", "Quando", "land")],
        },
        op(
            "input.read",
            "Ler ação de entrada",
            "Entrada e tempo",
            false,
            vec![text("action", "Ação", "pular")],
            vec![
                port("pressed", "Pressionada neste passo", Bool),
                port("held", "Mantida", Bool),
                port("released", "Solta neste passo", Bool),
            ],
        ),
        op(
            "input.axes",
            "Ler eixos de entrada",
            "Entrada e tempo",
            false,
            vec![target()],
            vec![
                port("movement", "Movimento (direita/frente)", Vector2),
                port("look", "Olhar relativo (X/Y)", Vector2),
                port("wheel", "Roda", Number),
            ],
        ),
        op(
            "time.read",
            "Ler tempo da simulação",
            "Entrada e tempo",
            false,
            vec![],
            vec![
                port("dt", "Duração do passo (s)", Number),
                port("time", "Tempo (s)", Number),
            ],
        ),
        op(
            "character.state",
            "Ler personagem 3D",
            "Personagem 3D",
            false,
            vec![target()],
            state_ports(),
        ),
        op(
            "character.intent",
            "Definir intenção para este passo",
            "Personagem 3D",
            true,
            vec![
                target(),
                param("axis", "Direita/frente", Value::Vector2([0.; 2])),
            ],
            vec![],
        ),
        op(
            "character.jump",
            "Pedir pulo",
            "Personagem 3D",
            true,
            vec![target()],
            vec![],
        ),
        op(
            "character.posture",
            "Correr / agachar / deslizar",
            "Personagem 3D",
            true,
            vec![
                target(),
                text("command", "Comando", "crouch"),
                flag("enabled", "Ativar", true),
            ],
            vec![],
        ),
        op(
            "character.velocity",
            "Adicionar / definir velocidade (m/s)",
            "Personagem 3D",
            true,
            vec![
                target(),
                v3("velocity", "Velocidade mundial (m/s)", [0., 8., 0.]),
                text("mode", "Operação", "add"),
            ],
            vec![],
        ),
        op(
            "character.block",
            "Bloquear / liberar entrada por motivo",
            "Personagem 3D",
            true,
            vec![
                target(),
                text("channel", "Entrada", "movement"),
                text("reason", "Motivo", "interação"),
                flag("blocked", "Bloquear", true),
            ],
            vec![],
        ),
        op(
            "character.profile",
            "Aplicar perfil de movimento",
            "Personagem 3D",
            true,
            vec![
                target(),
                text("profile", "Perfil", "direct"),
                flag(
                    "allow_limit",
                    "Permitir reduzir embalo pelo novo limite",
                    false,
                ),
            ],
            vec![],
        ),
        op(
            "character.parameter",
            "Alterar parâmetro do personagem",
            "Personagem 3D",
            true,
            vec![
                target(),
                text("parameter", "Parâmetro", "speed"),
                number("value", "Valor", 6.),
                flag(
                    "allow_limit",
                    "Permitir reduzir embalo pelo novo limite",
                    false,
                ),
            ],
            vec![],
        ),
        op(
            "character.option",
            "Alterar opção do personagem",
            "Personagem 3D",
            true,
            vec![
                target(),
                text("option", "Opção", "automatic_input"),
                flag("enabled", "Ativar", true),
            ],
            vec![],
        ),
        op(
            "transform.read",
            "Ler posição e orientação",
            "Transformação 3D",
            false,
            vec![target(), text("space", "Espaço", "world")],
            vec![
                port("position", "Posição (m)", Vector3),
                port("rotation", "Rotação XYZ (°)", Vector3),
                port("yaw", "Giro horizontal (°)", Number),
            ],
        ),
        op(
            "character.yaw",
            "Definir giro horizontal do personagem",
            "Transformação 3D",
            true,
            vec![target(), number("yaw", "Giro mundial (°)", 0.)],
            vec![],
        ),
        op(
            "character.teleport",
            "Teleportar com segurança",
            "Transformação 3D",
            true,
            vec![
                target(),
                v3("position", "Posição mundial dos pés (m)", [0.; 3]),
                flag("keep_velocity", "Manter velocidade total", false),
                flag("restore_yaw", "Restaurar giro do corpo", true),
                number("yaw", "Giro mundial (°)", 0.),
                flag("restore_look", "Restaurar olhar", true),
                param(
                    "look",
                    "Olhar mundial: giro/inclinação (°)",
                    Value::Vector2([0.; 2]),
                ),
                number("search", "Busca local máxima (m; 0 recusa ocupado)", 0.),
            ],
            vec![
                port("success", "Sucesso", Bool),
                port("error", "Motivo de recusa", Text),
                port("position", "Destino utilizado (m)", Vector3),
            ],
        ),
        op(
            "camera.read",
            "Ler câmera de jogo",
            "Câmera de jogo",
            false,
            vec![],
            vec![
                port("camera", "Câmera ativa", Object),
                port("look", "Olhar de controle: giro/inclinação (°)", Vector2),
                port("forward", "Direção do olhar", Vector3),
                port("mode", "Modo", Text),
            ],
        ),
        op(
            "camera.activate",
            "Ativar câmera de jogo",
            "Câmera de jogo",
            true,
            vec![target(), number("seconds", "Transição (s)", 0.25)],
            vec![],
        ),
        op(
            "camera.mode",
            "Trocar modo da câmera",
            "Câmera de jogo",
            true,
            vec![
                target(),
                text("mode", "Modo", "third_person"),
                number("seconds", "Transição (s)", 0.25),
            ],
            vec![],
        ),
        op(
            "camera.target",
            "Definir personagem acompanhado",
            "Câmera de jogo",
            true,
            vec![
                target(),
                param("object", "Objeto acompanhado", Value::Object(None)),
            ],
            vec![],
        ),
        op(
            "camera.look_at",
            "Olhar assistido / liberar olhar",
            "Câmera de jogo",
            true,
            vec![
                target(),
                text("mode", "Alvo do olhar", "point"),
                v3("point", "Ponto mundial (m)", [0.; 3]),
                param("object", "Objeto observado", Value::Object(None)),
            ],
            vec![],
        ),
        op(
            "camera.setting",
            "Ajustar enquadramento da câmera",
            "Câmera de jogo",
            true,
            vec![
                target(),
                text("setting", "Ajuste", "fov"),
                number("value", "Valor", 60.),
                number("seconds", "Suavização (s)", 0.15),
            ],
            vec![],
        ),
        op(
            "surface.get",
            "Ler superfície do colisor",
            "Superfícies físicas",
            false,
            vec![target()],
            vec![port("surface", "Superfície", Surface)],
        ),
        op(
            "surface.apply",
            "Aplicar superfície ao colisor",
            "Superfícies físicas",
            true,
            vec![
                target(),
                param("surface", "Superfície", Value::Surface(None)),
            ],
            vec![],
        ),
        op(
            "value.surface",
            "Referência de superfície",
            "Superfícies físicas",
            false,
            vec![param("value", "Superfície", Value::Surface(None))],
            vec![port("value", "Superfície", Surface)],
        ),
    ];
    for (constant, construct, parts, math, kind, zero, axes) in [
        (
            "value.vector2",
            "vector2.make",
            "vector2.parts",
            "vector2.math",
            Vector2,
            Value::Vector2([0.; 2]),
            &["x", "y"][..],
        ),
        (
            "value.vector3",
            "vector3.make",
            "vector3.parts",
            "vector3.math",
            Vector3,
            Value::Vector3([0.; 3]),
            &["x", "y", "z"][..],
        ),
    ] {
        out.push(op(
            constant,
            if kind == Vector2 {
                "Vetor2 constante"
            } else {
                "Vetor3 constante"
            },
            "Vetores",
            false,
            vec![param("value", "Vetor", zero.clone())],
            vec![port("value", "Vetor", kind)],
        ));
        out.push(op(
            construct,
            if kind == Vector2 {
                "Construir Vetor2"
            } else {
                "Construir Vetor3"
            },
            "Vetores",
            false,
            axes.iter().map(|axis| number(axis, axis, 0.)).collect(),
            vec![port("value", "Vetor", kind)],
        ));
        out.push(op(
            parts,
            if kind == Vector2 {
                "Decompor Vetor2"
            } else {
                "Decompor Vetor3"
            },
            "Vetores",
            false,
            vec![param("value", "Vetor", zero.clone())],
            axes.iter().map(|axis| port(axis, axis, Number)).collect(),
        ));
        out.push(op(
            math,
            if kind == Vector2 {
                "Operações com Vetor2"
            } else {
                "Operações com Vetor3"
            },
            "Vetores",
            false,
            vec![
                param("a", "Vetor A", zero.clone()),
                param("b", "Vetor B", zero),
                number("scalar", "Escalar", 1.),
            ],
            vec![
                port("sum", "A + B", kind),
                port("difference", "A − B", kind),
                port("scaled", "A × escalar", kind),
                port("normalized", "A normalizado (zero seguro)", kind),
                port("length", "Comprimento de A", Number),
                port("dot", "Produto escalar A · B", Number),
            ],
        ));
    }
    out.push(op(
        "vector.direction",
        "Transformar direção entre espaços",
        "Vetores",
        false,
        vec![
            target(),
            v3("value", "Direção", [0., 0., -1.]),
            text("space", "Conversão", "local_to_world"),
        ],
        vec![port("value", "Direção convertida", Vector3)],
    ));
    for (id, label) in [
        ("query.ray", "Consultar raio"),
        ("query.sphere", "Varrer esfera"),
        ("query.capsule", "Varrer cápsula"),
        ("query.space", "Testar espaço livre para cápsula"),
    ] {
        out.push(op(
            id,
            label,
            "Consultas físicas 3D",
            true,
            vec![
                v3("origin", "Origem/centro mundial (m)", [0., 1., 0.]),
                v3("direction", "Direção mundial", [0., 0., -1.]),
                number("distance", "Distância (m)", 10.),
                number("radius", "Raio (m)", 0.3),
                number("height", "Altura total da cápsula (m)", 1.8),
                flag("sensors", "Incluir áreas", false),
                param("exclude", "Ignorar objeto/hierarquia", Value::Object(None)),
                number("category", "Pertence aos grupos…", 1.),
                number("mask", "Consultar grupos…", f64::from(u32::MAX)),
            ],
            vec![
                port("hit", "Atingiu / espaço ocupado", Bool),
                port("free", "Espaço livre", Bool),
                port("object", "Objeto atingido", Object),
                port("point", "Ponto mundial (m)", Vector3),
                port("normal", "Normal mundial", Vector3),
                port("distance", "Distância / penetração negativa (m)", Number),
                port("fraction", "Fração do percurso", Number),
                port("surface", "Superfície", Surface),
                port("success", "Consulta válida", Bool),
                port("error", "Motivo de recusa", Text),
            ],
        ));
    }
    out
}
pub fn movement_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "event.step" => (
            "Executa antes do movimento, uma vez por passo fixo de simulação.",
            "Renove a intenção de um personagem controlado por nós.",
            "Não acompanha a taxa de desenho. Esperas criam continuações independentes e respeitam o orçamento.",
        ),
        "event.character" => (
            "Entrega uma transição com dados imutáveis do personagem naquele instante.",
            "Escolha Aterrissar e use a rapidez anterior à correção para uma condição de impacto.",
            "Contatos laterais persistentes não repetem início a cada passo. Um teleporte cancela continuações da trajetória anterior.",
        ),
        "input.read" | "input.axes" | "time.read" => (
            "Lê entrada ou tempo do passo atual sem consumir novamente as ações.",
            "Ligue o eixo de movimento a Definir intenção; consulte o tempo para um cronômetro.",
            "Olhar é deslocamento relativo, não velocidade. Não multiplique o mouse por dt nem por escala da interface.",
        ),
        "character.state" => (
            "Lê velocidades, apoio, postura e bloqueios atuais do motor.",
            "Mostre Rapidez horizontal num atributo ligado ao texto do HUD.",
            "Velocidade total inclui apoio; própria representa embalo relativo. Apoio é chão utilizável, não qualquer parede.",
        ),
        "character.intent" => (
            "Substitui a intenção direcional somente para o próximo passo aplicável.",
            "Escolha fonte Nós no controlador e renove com A cada passo.",
            "A intenção expira; não soma duas intenções completas. Bloquear entrada mantém gravidade e inércia.",
        ),
        "character.jump" | "character.posture" => (
            "Envia pedidos ao mesmo motor usado pelos controles automáticos.",
            "Peça um pulo ou agachamento por uma ação de entrada.",
            "Pulo coalesce pedidos e respeita chão/tolerâncias. Ficar em pé exige espaço; deslize exige perfil habilitado e embalo.",
        ),
        "character.velocity" => (
            "Adiciona ou substitui velocidade mundial em metros por segundo.",
            "Adicione (0,8,0) para lançar o personagem para cima.",
            "Não é força em newtons. Adicionar preserva embalo; Definir substitui explicitamente a velocidade total.",
        ),
        "character.block" => (
            "Acumula bloqueios independentes de movimento ou olhar, por motivo.",
            "Bloqueie movimento por diálogo e remova somente esse motivo ao fechar.",
            "Não desliga colisão, gravidade ou câmera. Liberar um motivo não remove outros bloqueios.",
        ),
        "character.profile" | "character.parameter" | "character.option" => (
            "Altera apenas configurações permitidas do motor durante o teste.",
            "Compare Parkour e Saltos encadeados sem reiniciar o personagem.",
            "Perfil não teleporta nem zera velocidade. Reduzir limite abaixo do embalo exige política explícita.",
        ),
        "character.teleport" => (
            "Reloca a cápsula após validar espaço; informa sucesso ou recusa.",
            "Guarde posição Vetor3 e olhar Vetor2 como checkpoint e use-os ao reiniciar.",
            "Destino ocupado não altera a posição. Busca local é limitada a 2 m; limpa apoio, pedidos e trajetória anterior.",
        ),
        "transform.read" | "character.yaw" => (
            "Consulta transformações ou define giro horizontal compatível com a raiz física.",
            "Guarde giro mundial em graus junto ao checkpoint.",
            "Posição usa metros. Rotação usa graus XYZ e espaço declarado. Mova o personagem por Teleportar com segurança.",
        ),
        "camera.read" | "camera.activate" | "camera.mode" | "camera.target" | "camera.look_at"
        | "camera.setting" => (
            "Controla a câmera de jogo pelo mesmo serviço de primeira/terceira pessoa.",
            "Troque para terceira pessoa sem zerar a velocidade e ajuste distância/ombro.",
            "Olhar assistido substitui o mouse até ser liberado. Colisão vence suavização. FOV é vertical em graus; distâncias são metros.",
        ),
        "surface.get" | "surface.apply" | "value.surface" => (
            "Lê ou aplica uma referência de material físico, independente da pintura.",
            "Compare Gelo, Piso comum e Lama numa plataforma.",
            "Atrito freia embalo; tração controla aceleração. Aplicar um material não muda textura nem cria regras de dano.",
        ),
        "value.vector2" | "value.vector3" | "vector2.make" | "vector3.make" | "vector2.parts"
        | "vector3.parts" | "vector2.math" | "vector3.math" | "vector.direction" => (
            "Calcula dados vetoriais tipados com operações explícitas.",
            "Construa uma velocidade Vetor3 ou normalize uma direção antes de uma consulta.",
            "Vetor zero normaliza para zero. Transformar direção usa rotação, sem translação/escala. Valores não finitos são recusados.",
        ),
        "query.ray" | "query.sphere" | "query.capsule" | "query.space" => (
            "Consulta formas físicas do estado atual e guarda o resultado nesta execução.",
            "Varra uma esfera a partir da posição do personagem para detectar uma parede.",
            "Não move objetos. Origem é o centro da forma; sensores são opcionais. Há limite de consultas e saídas de sucesso/erro.",
        ),
        _ => return None,
    })
}
pub fn movement_choices(
    operation: &str,
    parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match (operation, parameter) {
        ("event.character", "kind") => &[
            ("jump", "Ao pular"),
            ("land", "Ao aterrissar"),
            ("leave", "Ao perder apoio"),
            ("surface", "Ao mudar superfície"),
            ("posture", "Ao mudar postura"),
            ("side", "Ao iniciar contato lateral"),
        ],
        ("character.posture", "command") => &[
            ("sprint", "Correr"),
            ("crouch", "Agachar"),
            ("slide", "Deslizar"),
        ],
        ("character.velocity", "mode") => &[
            ("add", "Adicionar velocidade"),
            ("set", "Substituir velocidade"),
        ],
        ("character.block", "channel") => &[("movement", "Movimento"), ("look", "Olhar")],
        ("character.profile", "profile") => &[
            ("direct", "Direto"),
            ("parkour", "Parkour"),
            ("chained", "Saltos encadeados"),
        ],
        ("character.option", "option") => &[
            ("automatic_input", "Usar ações automáticas (desligado: Nós)"),
            ("slide_enabled", "Permitir deslize"),
            ("crouch_toggle", "Alternar agachamento"),
            ("automatic_jump", "Pulo automático ao segurar"),
            ("face_look", "Virar corpo para o olhar"),
        ],
        ("character.parameter", "parameter") => &[
            ("speed", "Velocidade andando (m/s)"),
            ("sprint_speed", "Velocidade correndo (m/s)"),
            ("crouch_speed", "Velocidade agachado (m/s)"),
            ("gravity", "Gravidade (m/s²)"),
            ("jump_speed", "Velocidade de pulo (m/s)"),
            ("ground_acceleration", "Aceleração no chão (m/s²)"),
            ("ground_braking", "Frenagem no chão (m/s²)"),
            ("ground_friction", "Atrito do personagem (por s)"),
            ("air_acceleration", "Aceleração aérea (m/s²)"),
            ("air_projected_limit", "Limite direcional aéreo (m/s)"),
            ("air_resistance", "Resistência horizontal do ar (por s)"),
            ("horizontal_limit", "Limite horizontal (m/s; 0 desliga)"),
            ("jump_retention", "Conservação ao saltar (0 a 1)"),
            ("landing_retention", "Conservação ao pousar (0 a 1)"),
            ("coyote_ms", "Tolerância após borda (ms)"),
            ("jump_buffer_ms", "Antecipação do pulo (ms)"),
            ("slide_duration", "Duração do deslize (s)"),
            ("slide_friction", "Atrito do deslize (por s)"),
            ("slide_control", "Controle do deslize (m/s²)"),
            ("angular_speed", "Velocidade de giro (°/s)"),
        ],
        ("transform.read", "space") => &[("world", "Mundo"), ("local", "Local")],
        ("vector.direction", "space") => &[
            ("local_to_world", "Local → Mundo"),
            ("world_to_local", "Mundo → Local"),
        ],
        ("camera.mode", "mode") => &[
            ("first_person", "Primeira pessoa"),
            ("third_person", "Terceira pessoa"),
            ("fixed", "Fixa"),
        ],
        ("camera.look_at", "mode") => &[
            ("point", "Olhar para ponto"),
            ("object", "Olhar para objeto"),
            ("clear", "Liberar olhar ao jogador"),
        ],
        ("camera.setting", "setting") => &[
            ("fov", "Campo de visão vertical (°)"),
            ("distance", "Distância desejada (m)"),
            ("shoulder", "Deslocamento do ombro (m)"),
        ],
        _ => return None,
    })
}
