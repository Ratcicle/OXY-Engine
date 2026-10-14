//! Typed native movement operations. Parameters are editable defaults, never demo rules.
use super::*;
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
            "transform.write",
            "Mover / girar / escalar objeto",
            "Transformação 3D",
            true,
            vec![
                target(),
                text("channel", "O que muda", "position"),
                text("mode", "Como", "add"),
                text("space", "Espaço", "local"),
                v3("value", "Valor", [0.; 3]),
                flag("jump", "Salto: soltar passageiros", false),
            ],
            vec![],
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
/// What each node does (its own purpose, which is also its catalog description), an example
/// and its cautions. Units are the engine's: metres, metres per second, degrees and seconds.
pub fn movement_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    const VECTORS: &str = "Um vetor não tem unidade própria: vale a do que representa (metros numa posição, m/s numa velocidade, graus num olhar). Valores não finitos são recusados.";
    const QUERIES: &str = "Não move objetos. Até 256 consultas por passo; confira Consulta válida antes de usar o contato. A consulta encontra também o que a forma já toca na Origem: uma que começa dentro do personagem o atinge na distância 0, então escolha-o em Ignorar objeto/hierarquia, que ignora também os filhos dele; uma esfera ou cápsula centrada nos pés já toca o chão. Áreas só contam com Incluir áreas.";
    Some(match id {
        "event.step" => (
            "Executa uma vez por passo fixo da simulação (60 por segundo), antes do movimento, e fornece a Duração do passo (1/60 s) e o Tempo da simulação, o tempo de jogo desde o começo da partida, ambos em segundos.",
            "Renove a intenção de um personagem controlado por nós.",
            "Não acompanha a taxa de desenho. Esperas criam continuações independentes e respeitam o orçamento. Na pausa do jogo (Pausar / retomar jogo), não executa: o mundo fica parado.",
        ),
        "event.character" => (
            "Executa na transição do personagem 3D escolhida em Quando (pular, aterrissar, perder o apoio, mudar de superfície ou de postura, ou começar um contato lateral) e fornece o estado daquele instante, como Ler personagem 3D (posições em metros, velocidades em m/s), a Rapidez do impacto ao aterrissar, em m/s, o objeto do contato lateral e a superfície anterior.",
            "Escolha Aterrissar e use a rapidez anterior à correção para uma condição de impacto.",
            "Contatos laterais persistentes não repetem início a cada passo. Um teleporte cancela continuações da trajetória anterior.",
        ),
        "input.read" => (
            "Lê uma ação de entrada no passo atual: Pressionada e Solta são verdadeiras só no passo em que a tecla desce ou sobe; Mantida, enquanto ela está segura.",
            "Em A cada passo, ligue Pressionada à Condição de Se / Senão para pedir um pulo só quando a tecla desce.",
            "Lê sem consumir a ação: o controlador e outros nós veem o mesmo estado no mesmo passo. Escolha uma ação criada em Ações de entrada.",
        ),
        "input.axes" => (
            "Lê os eixos do passo atual: Movimento é o Vetor2 (direita, frente), de comprimento até 1, das teclas de movimento do personagem 3D escolhido; Olhar é o deslocamento do mouse neste passo, em unidades do dispositivo e não em graus (X para a direita, Y para baixo); Roda é o giro da roda do mouse neste passo, em linhas (positivo para cima).",
            "Ligue Movimento à entrada Direita/frente de Definir intenção para este passo, num personagem com as ações automáticas desligadas.",
            "Olhar é deslocamento relativo, não velocidade: não o multiplique por dt nem pela escala da interface; a câmera o converte em graus pela sensibilidade. Olhar vale (0, 0) quando a câmera ativa não usa o mouse: sem câmera ativa, na Fixa ou sem Controle de câmera, com olhar assistido ou com o olhar do personagem bloqueado, na pausa do jogo e com o cursor à mostra (Mostrar / esconder cursor). Só Movimento usa o objeto, que precisa ser um personagem 3D (vazio: o responsável).",
        ),
        "time.read" => (
            "Fornece a Duração do passo fixo (1/60 s) e o Tempo de jogo desde o começo da partida, ambos em segundos.",
            "Ao começar um desafio, guarde o Tempo num atributo; ao concluí-lo, subtraia o valor guardado do Tempo atual para saber quantos segundos levou.",
            "O tempo é o do jogo: para na pausa do jogo e na da engine, segue de onde parou ao retomar e não volta a zero ao mudar de cena.",
        ),
        "character.state" => (
            "Lê o estado atual do personagem 3D: a Posição (a origem, nos pés) e o Ponto de apoio, em metros; as velocidades em m/s (total, com a do apoio; própria, só o embalo; horizontal e a rapidez dela); se está no chão, subindo ou caindo; a Postura em texto (Em pé, Agachado ou Deslizando); o apoio, a superfície e a normal dele; e os bloqueios com os motivos.",
            "Mostre Rapidez horizontal num atributo ligado ao texto do HUD.",
            "Velocidade total inclui apoio; própria representa embalo relativo. Apoio é chão utilizável, não qualquer parede.",
        ),
        "character.intent" => (
            "Define para onde o personagem 3D quer andar no próximo passo: um Vetor2 (direita, frente) de comprimento até 1, relativo à Direção do personagem (eixos da cena, corpo ou câmera), que o motor converte em velocidade pelas acelerações dele.",
            "Escolha fonte Nós no controlador e renove com A cada passo.",
            "A intenção expira; não soma duas intenções completas. Bloquear entrada mantém gravidade e inércia.",
        ),
        "character.jump" => (
            "Pede um pulo ao personagem 3D, como a tecla de pular: ele pula se estiver no chão ou dentro da tolerância após a borda; senão, o pedido espera pela antecipação do pulo. A altura vem da Velocidade de pulo do personagem, em m/s.",
            "Numa ação de entrada, com as ações automáticas do personagem desligadas, ligue Executar a Pedir pulo.",
            "Vários pedidos no mesmo passo viram um pulo só. Para lançar o personagem com uma velocidade escolhida, use Adicionar / definir velocidade.",
        ),
        "character.posture" => (
            "Liga ou desliga correr, agachar ou deslizar no personagem 3D, como as teclas do controlador. Correr usa a Velocidade correndo e agachar, a Velocidade agachado do personagem, em m/s.",
            "Peça Agachar com Ativar ligado ao pressionar uma tecla e desligado ao soltá-la.",
            "Ficar em pé exige espaço acima. Deslizar exige o deslize permitido e embalo; desligar Deslizar faz o personagem levantar, se couber.",
        ),
        "character.velocity" => (
            "Soma um Vetor3 mundial em m/s (metros por segundo) ao embalo do personagem 3D ou substitui a velocidade total dele por esse vetor.",
            "Adicione (0,8,0) para lançar o personagem para cima.",
            "Não é força em newtons. Adicionar preserva embalo; Definir substitui explicitamente a velocidade total. Uma velocidade para cima tira o personagem do chão.",
        ),
        "character.block" => (
            "Acumula bloqueios independentes de movimento ou olhar, por motivo.",
            "Bloqueie movimento por diálogo e remova somente esse motivo ao fechar.",
            "Não desliga colisão, gravidade ou câmera. Liberar um motivo não remove outros bloqueios.",
        ),
        "character.profile" => (
            "Aplica ao personagem 3D, durante o jogo, um perfil de movimento pronto: Direto, Ágil com embalo ou Saltos encadeados, com os mesmos valores do botão Aplicar perfil de movimento do editor.",
            "Compare Ágil com embalo e Saltos encadeados sem reiniciar o personagem.",
            "Perfil não teleporta nem zera velocidade. Reduzir o limite horizontal abaixo do embalo atual exige Permitir reduzir embalo. Vale só durante a partida: o projeto não muda.",
        ),
        "character.parameter" => (
            "Muda um valor do motor do personagem 3D durante o jogo, na unidade indicada na opção: velocidades em m/s, acelerações em m/s², atritos e resistência por segundo, tolerâncias em milissegundos, duração do deslize em segundos e giro em graus por segundo.",
            "Ao entrar numa área, mude Velocidade andando para 3 m/s; ao sair, volte a 6.",
            "Um valor que o personagem não aceita gera diagnóstico e não muda nada. Reduzir o limite horizontal abaixo do embalo atual exige Permitir reduzir embalo. Vale só durante a partida: o projeto não muda.",
        ),
        "character.option" => (
            "Liga ou desliga uma opção do personagem 3D durante o jogo: usar as ações automáticas (desligado, ele obedece só aos nós), permitir deslize, alternar agachamento, pulo automático ao segurar e virar o corpo para o olhar.",
            "Desligue Usar ações automáticas para comandar o personagem por Definir intenção e Pedir pulo.",
            "Com as ações automáticas desligadas, o personagem não anda sem Definir intenção a cada passo. Vale só durante a partida: o projeto não muda.",
        ),
        "transform.read" => (
            "Fornece a Posição em metros, a Rotação XYZ em graus e o Giro horizontal em graus (0° olha para −Z; positivo gira para a esquerda) do objeto, no Mundo ou no Local (relativo ao pai).",
            "Guarde a Posição e o Giro do personagem em atributos, como ponto de retorno, e use-os em Teleportar com segurança.",
            "No Mundo, a posição é a do pivô, como nas Propriedades, e num personagem 3D, a origem, onde Teleportar com segurança o põe. Para mover, use Mover / girar / escalar objeto ou, num personagem, Teleportar com segurança.",
        ),
        "character.yaw" => (
            "Gira o corpo do personagem 3D para o Giro mundial dado, em graus (0° olha para −Z; positivo gira para a esquerda), sem mudar a posição.",
            "Defina 90° para o personagem olhar para −X; 180°, para +Z.",
            "Se o corpo girado ocupar um obstáculo, nada muda e o nó gera diagnóstico. Em primeira pessoa, a câmera que segue o personagem gira junto. A velocidade não muda.",
        ),
        "transform.write" => (
            "Define ou soma a posição (metros), a rotação (graus XYZ) ou a escala de um objeto, no mundo ou no espaço do pai.",
            "Em A cada passo, multiplique (2, 0, 0) pela Duração do passo e some o resultado à Posição no Mundo de uma plataforma móvel que segue a animação: ela anda 2 m/s e leva quem está em cima.",
            "No Mundo, a posição é a do pivô, como nas Propriedades e em Ler posição e orientação; no Local, os eixos são os do pai. Somar à rotação no Local soma aos ângulos XYZ; no Mundo, gira nos eixos do mundo. Somar à escala multiplica; a escala só muda no Local e não pode chegar a zero. Recusa a raiz de um personagem 3D (use Teleportar com segurança ou Definir giro horizontal do personagem) e objetos que uma animação em reprodução move (pause-a com Ativar / desativar componente). Uma plataforma movida mais que o Salto máximo num passo, ou com Salto: soltar passageiros ligado, solta quem está em cima sem lançá-lo. Uma animação terminada ou pausada não desfaz a mudança.",
        ),
        "character.teleport" => (
            "Põe os pés do personagem 3D na Posição mundial, em metros, se houver espaço, com giro do corpo e olhar opcionais, em graus, e fornece Sucesso, o Motivo de recusa e o Destino utilizado, em metros.",
            "Guarde a posição (Vetor3) e o olhar (Vetor2) de um ponto de retorno e use-os ao reiniciar.",
            "Destino ocupado não altera a posição. Busca local é limitada a 2 m; limpa apoio, pedidos e trajetória anterior.",
        ),
        "camera.read" => (
            "Fornece a câmera de jogo ativa, o Olhar de controle em graus (Vetor2: giro, 0° para −Z e positivo para a esquerda; inclinação, positiva para cima), a Direção do olhar (Vetor3 mundial de comprimento 1) e o Modo em texto: Primeira pessoa, Terceira pessoa ou Fixa.",
            "Ligue o Olhar à entrada Olhar de Teleportar com segurança para o jogador reaparecer olhando para o mesmo lado.",
            "É o olhar da simulação, sem a suavização da imagem. Numa câmera Fixa, vem da rotação do próprio objeto câmera.",
        ),
        "camera.activate" => (
            "Torna a câmera escolhida a câmera de jogo ativa, com transição suave pela duração indicada, em segundos (0 = corte seco).",
            "Ao entrar numa área, ative a câmera de vista geral com Transição de 1 segundo; ao sair, volte à câmera do jogador.",
            "O objeto precisa ter Câmera. A câmera mantém o modo e o objeto que acompanha.",
        ),
        "camera.mode" => (
            "Troca o modo de uma câmera com Controle de câmera, com transição em segundos: Primeira pessoa (nos olhos), Terceira pessoa (atrás, à distância desejada) ou Fixa, parada sem seguir o personagem nem girar com o mouse. Fixa na vista atual para a câmera onde a vista está; Fixa no objeto câmera leva a vista à posição e à rotação do próprio objeto câmera.",
            "Ao terminar uma partida, troque para Fixa na vista atual e, em seguida, mova o objeto câmera com Mover / girar / escalar objeto: a vista sai de onde estava e vai até o novo ponto pela transição. Para soltar o mouse sem mexer na câmera, use Mostrar / esconder cursor.",
            "Na Fixa na vista atual, o objeto câmera passa para onde a vista está, e movê-lo depois move a vista; mover o objeto antes de trocar o modo não muda nada, pois a troca o põe de volta na vista. Para levar a vista a um ponto escolhido antes, mova o objeto e use Fixa no objeto câmera. Numa câmera que não é a ativa, ou que já está fixa, as duas Fixas usam o objeto câmera como ele está. Fixa na vista atual a partir da Primeira pessoa, a câmera continua escondendo as peças escondidas em primeira pessoa até o objeto câmera ser movido, para o corpo não tampar a vista; em qualquer modo, ela atravessa o corpo do personagem acompanhado. O mouse fica livre, como com o cursor à mostra. A tecla de alternar primeira e terceira pessoa do Controle de câmera também sai da Fixa.",
        ),
        "camera.target" => (
            "Troca o objeto que uma câmera com Controle de câmera acompanha, normalmente um personagem 3D: a câmera passa a segui-lo, no modo em que está.",
            "Ao trocar de personagem controlado, aponte a câmera do jogador para o novo.",
            "O objeto acompanhado não pode ser a própria câmera. Vale só durante a partida: o projeto não muda.",
        ),
        "camera.look_at" => (
            "Vira uma câmera com Controle de câmera para um Ponto mundial, em metros, ou para um objeto e a mantém assim, sem obedecer ao mouse, até Liberar olhar ao jogador.",
            "Ao abrir uma porta, olhe para ela, espere 2 segundos com Esperar sem bloquear e libere o olhar.",
            "Olhar assistido substitui o mouse até ser liberado; ao liberar, o controle segue da vista atual. A câmera precisa estar ativa.",
        ),
        "camera.setting" => (
            "Ajusta o enquadramento de uma câmera, com suavização em segundos: Campo de visão vertical em graus (10 a 150), Distância desejada em metros (terceira pessoa) ou Deslocamento do ombro em metros.",
            "Ao correr, aumente o campo de visão para 75° em 0,3 segundo; ao parar, volte a 60°.",
            "Colisão vence suavização: diante de uma parede, a câmera chega mais perto que a distância pedida. Distância e ombro exigem Controle de câmera; o campo de visão vale para qualquer câmera.",
        ),
        "surface.get" => (
            "Fornece a superfície física aplicada ao Colisor 3D do objeto ou ao corpo do personagem 3D; vazia quando não há uma.",
            "Guarde a superfície de uma plataforma antes de trocá-la e reaplique-a depois.",
            "Fornece a referência, não os valores de atrito e tração. Para saber onde o personagem pisa, use Superfície de Ler personagem 3D.",
        ),
        "surface.apply" => (
            "Aplica uma superfície física (atrito, tração, multiplicador de velocidade e esteira) ao Colisor 3D do objeto ou ao corpo do personagem 3D durante o jogo; vazia tira a superfície.",
            "Compare Gelo, Piso comum e Lama numa plataforma.",
            "Atrito freia embalo; tração controla aceleração. Aplicar um material não muda textura nem cria regras de dano. Vale só durante a partida: o projeto não muda.",
        ),
        "value.surface" => (
            "Fornece uma das superfícies físicas do projeto para Aplicar superfície ao colisor ou para Comparar com a superfície de um apoio.",
            "Ligue Gelo à entrada Superfície de Aplicar superfície ao colisor.",
            "Crie as superfícies no inspetor do Colisor 3D. Sem superfície escolhida, a referência é vazia.",
        ),
        "value.vector2" => (
            "Fornece um Vetor2 constante (X, Y), como um eixo direita/frente ou um olhar em graus (giro, inclinação).",
            "Use (0, 1) como intenção de andar para a frente.",
            VECTORS,
        ),
        "value.vector3" => (
            "Fornece um Vetor3 constante (X, Y, Z), como uma posição em metros, uma velocidade em m/s ou uma direção.",
            "Use (0, 8, 0) como impulso para cima em Adicionar / definir velocidade.",
            VECTORS,
        ),
        "vector2.make" => (
            "Monta um Vetor2 com os números X e Y, ligados às entradas ou escritos no painel.",
            "Monte (X do Movimento, 0) para andar só de lado.",
            VECTORS,
        ),
        "vector3.make" => (
            "Monta um Vetor3 com os números X, Y e Z, ligados às entradas ou escritos no painel.",
            "Construa uma velocidade Vetor3 a partir de números calculados.",
            VECTORS,
        ),
        "vector2.parts" => (
            "Separa um Vetor2 nos números X e Y.",
            "Separe o Olhar de Ler câmera de jogo para usar só o giro (X).",
            VECTORS,
        ),
        "vector3.parts" => (
            "Separa um Vetor3 nos números X, Y e Z.",
            "Separe a Posição do personagem e compare Y com uma altura para saber se ele caiu.",
            VECTORS,
        ),
        "vector2.math" => (
            "Calcula com os Vetor2 A e B: A + B, A − B, A vezes o escalar, A normalizado (comprimento 1), o Comprimento de A e o Produto escalar A · B.",
            "Normalize o Movimento antes de multiplicá-lo pela rapidez desejada.",
            "Vetor zero normaliza para zero. Comprimento e Normalizado usam só A. Valores não finitos são recusados.",
        ),
        "vector3.math" => (
            "Calcula com os Vetor3 A e B: A + B, A − B, A vezes o escalar, A normalizado (comprimento 1), o Comprimento de A e o Produto escalar A · B.",
            "Ligue A − B de duas posições à entrada A de outro nó: o Comprimento dele é a distância, em metros.",
            "Vetor zero normaliza para zero. Comprimento e Normalizado usam só A. Valores não finitos são recusados.",
        ),
        "vector.direction" => (
            "Converte uma direção entre o espaço do objeto (Local) e o do mundo, só pela rotação dele: de Local para Mundo, (0, 0, −1) dá para onde a frente do objeto aponta no mundo.",
            "Converta (0, 0, −1) de Local para Mundo no personagem e use o resultado como Direção de Consultar raio, à frente dele.",
            "Usa só a rotação, sem posição nem escala: serve para direções e velocidades, não para pontos. Valores não finitos são recusados.",
        ),
        "query.ray" => (
            "Lança um raio da Origem na Direção, mundiais, até a Distância em metros e guarda o primeiro contato: Atingiu, Objeto, Ponto e Normal mundiais, Distância até o ponto em metros, Fração do percurso (0 a 1) e Superfície.",
            "Da posição do personagem mais 1 m para cima, lance um raio de 2 m para baixo, com ele em Ignorar objeto/hierarquia, para saber se há chão sob ele.",
            QUERIES,
        ),
        "query.sphere" => (
            "Varre uma esfera do Raio dado, em metros, com centro na Origem, na Direção até a Distância e guarda o primeiro contato: Atingiu, Objeto, Ponto e Normal mundiais, Distância percorrida em metros, Fração do percurso e Superfície.",
            "Da posição do personagem mais 1 m para cima, com ele em Ignorar objeto/hierarquia, varra uma esfera para a frente dele para detectar uma parede.",
            QUERIES,
        ),
        "query.capsule" => (
            "Varre uma cápsula em pé (Altura total e Raio em metros, centro na Origem) na Direção até a Distância e guarda o primeiro contato: Atingiu, Objeto, Ponto e Normal mundiais, Distância percorrida em metros, Fração do percurso e Superfície.",
            "Com centro 1 m acima dos pés do personagem e ele em Ignorar objeto/hierarquia, varra uma cápsula de 1,8 m para a frente dele para saber se ele passa por um vão.",
            QUERIES,
        ),
        "query.space" => (
            "Testa, sem mover nada, se uma cápsula em pé (Altura total e Raio em metros, centro na Origem) cabe ali: Espaço livre é verdadeiro quando ela não toca nada; senão, Atingiu, o Objeto e a Distância negativa, a penetração em metros. Direção e Distância não contam.",
            "Antes de Teleportar com segurança, teste se o destino tem espaço para a cápsula do personagem, com ele em Ignorar objeto/hierarquia.",
            QUERIES,
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
            ("parkour", "Ágil com embalo"),
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
        ("transform.read" | "transform.write", "space") => {
            &[("world", "Mundo"), ("local", "Local")]
        }
        ("transform.write", "channel") => &[
            ("position", "Posição (m)"),
            ("rotation", "Rotação XYZ (°)"),
            ("scale", "Escala"),
        ],
        ("transform.write", "mode") => {
            &[("set", "Definir"), ("add", "Somar (escala: multiplicar)")]
        }
        ("vector.direction", "space") => &[
            ("local_to_world", "Local → Mundo"),
            ("world_to_local", "Mundo → Local"),
        ],
        ("camera.mode", "mode") => &[
            ("first_person", "Primeira pessoa"),
            ("third_person", "Terceira pessoa"),
            ("fixed", "Fixa na vista atual"),
            ("fixed_object", "Fixa no objeto câmera"),
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
