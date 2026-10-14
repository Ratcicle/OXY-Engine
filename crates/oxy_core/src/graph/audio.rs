//! Sound operations: one-shots, loops on the music and effects channels, and the run's volumes.
use super::*;
/// Where Tocar em loop plays.
pub const SOUND_CHANNELS: [(&str, &str); 2] = [("music", "Música"), ("effects", "Efeitos")];
/// What Parar som stops.
pub const STOP_CHANNELS: [(&str, &str); 3] = [
    ("music", "Música"),
    ("effects", "Efeitos"),
    ("all", "Todos"),
];
/// The volume Volume do canal sets and Ler volumes gives.
pub const VOLUME_CHANNELS: [(&str, &str); 3] = [
    ("master", "Geral"),
    ("music", "Música"),
    ("effects", "Efeitos"),
];
pub(super) fn definitions() -> Vec<OperationDef> {
    use PortType::*;
    let exec = || port("exec", "Executar", Exec);
    let next = || vec![port("exec", "Continuar", Exec)];
    let volume = || number("volume", "Volume", 1.);
    let fade = || number("fade", "Transição (s)", 0.);
    vec![
        // Moved from Ações unchanged, its output's label included.
        OperationDef {
            id: "action.sound",
            label: "Reproduzir WAV",
            category: "Som",
            inputs: vec![exec()],
            outputs: vec![exec()],
            params: vec![
                text("asset", "Áudio", ""),
                number("volume", "Volume (0 a 1)", 1.),
            ],
        },
        // A connected Volume replaces the parameter, as a menu bar or a variable gives it.
        OperationDef {
            id: "action.sound_loop",
            label: "Tocar em loop",
            category: "Som",
            inputs: vec![exec(), port("volume", "Volume", Number)],
            outputs: next(),
            params: vec![
                text("asset", "Áudio", ""),
                text("channel", "Canal", "music"),
                volume(),
                fade(),
            ],
        },
        OperationDef {
            id: "action.sound_stop",
            label: "Parar som",
            category: "Som",
            inputs: vec![exec()],
            outputs: next(),
            params: vec![
                text("asset", "Áudio", ""),
                text("channel", "Canal", "all"),
                fade(),
            ],
        },
        OperationDef {
            id: "action.volume",
            label: "Volume do canal",
            category: "Som",
            inputs: vec![exec(), port("volume", "Volume", Number)],
            outputs: next(),
            params: vec![text("channel", "Canal", "master"), volume()],
        },
        op(
            "audio.read",
            "Ler volumes",
            "Som",
            false,
            vec![],
            vec![
                port("master", "Geral", Number),
                port("music", "Música", Number),
                port("effects", "Efeitos", Number),
            ],
        ),
    ]
}
pub fn audio_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "action.sound_loop" => (
            "Toca um WAV em loop, no Volume de 0 a 1, entrando pela Transição, em segundos. Na Música, o mesmo áudio continua tocando ao trocar ou reiniciar a cena; outro áudio entra com transição. Nos Efeitos, o loop acaba com a cena.",
            "Ao iniciar a cena, toque a trilha dela na Música com Transição de 1 segundo: ao reiniciar a cena, a trilha segue de onde estava; numa cena com outra trilha, a nova entra enquanto a anterior sai.",
            "A Música toca um áudio por vez; os Efeitos, até 32 loops, um por áudio: pedir um loop que já toca só muda o volume dele, e o 33º interrompe o comportamento com erro. Transição é a entrada suave em segundos (0 = de uma vez), de 0 a 60; na Música, é também a saída do áudio anterior. A pausa do jogo congela os efeitos e deixa a música tocando; a pausa da engine congela tudo. Parar o teste silencia tudo.",
        ),
        "action.sound_stop" => (
            "Para os loops de um áudio, ou todos os do canal, saindo pela Transição, em segundos (0 = de uma vez).",
            "Ao vencer, pare a Música com Transição de 2 segundos antes de mostrar o placar.",
            "Áudio vazio (Todos os sons do canal) para todos os loops do canal escolhido; o canal Todos vale para a Música e os Efeitos. Parar um som que não toca não faz nada. Sons tocados uma vez, por Reproduzir WAV, não param: eles acabam sozinhos.",
        ),
        "action.volume" => (
            "Ajusta o volume geral, da música ou dos efeitos durante a partida, de 0 a 1.",
            "Num menu de opções, ao clicar no botão + da música, some 0,1 ao valor de Ler volumes · Música, limite entre 0 e 1 e ligue ao Volume; guarde o mesmo valor numa variável salva entre partidas.",
            "Cada som é ouvido no volume geral vezes o do canal vezes o do próprio som; valores fora de 0 a 1 ficam no limite mais próximo. Os volumes valem para toda a partida, também depois de mudar ou reiniciar a cena, e cada partida começa com todos em 1. Para lembrar a escolha, guarde-a numa variável salva entre partidas e aplique-a com este nó ao iniciar o jogo.",
        ),
        "audio.read" => (
            "Fornece os volumes atuais (geral, da música e dos efeitos), de 0 a 1, para barras de um menu de opções.",
            "Ao abrir o menu de opções, grave Música num atributo ligado a uma barra da interface: a barra mostra o volume escolhido.",
            "Os valores vão de 0 a 1 e começam em 1 a cada partida; mudam só por Volume do canal.",
        ),
        _ => return None,
    })
}
/// Fixed values of the parameters of these operations.
pub fn audio_choices(
    operation: &str,
    parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match (operation, parameter) {
        ("action.sound_loop", "channel") => &SOUND_CHANNELS,
        ("action.sound_stop", "channel") => &STOP_CHANNELS,
        ("action.volume", "channel") => &VOLUME_CHANNELS,
        _ => return None,
    })
}
