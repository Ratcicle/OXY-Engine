//! Operations about the game as a whole rather than one object: the scene running.
use super::*;
pub(super) fn definitions() -> Vec<OperationDef> {
    use PortType::*;
    vec![op(
        "scene.current",
        "Cena atual",
        "Jogo",
        false,
        vec![],
        vec![port("scene", "Cena", Text), port("name", "Nome", Text)],
    )]
}
pub fn game_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "scene.current" => (
            "Fornece a cena em que o objeto está: o identificador, para Mudar de cena, e o nome, para chaves e textos.",
            "Ligue a saída Cena à entrada Cena de Mudar de cena para reiniciar a cena. A mesma lógica, copiada para outra cena ou colocada a partir de um modelo, reinicia a cena em que estiver.",
            "Cena é o identificador estável, que não muda ao renomear a cena: é o que Mudar de cena espera. Nome é o texto mostrado, que pode mudar ou se repetir: use-o em textos e chaves, não como destino. Para abrir outra cena, escolhida pelo nome, use Referência de cena.",
        ),
        _ => return None,
    })
}
/// Fixed values of the parameters of these operations; none has one yet.
pub fn game_choices(
    _operation: &str,
    _parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    None
}
