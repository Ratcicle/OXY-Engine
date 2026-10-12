//! Operations about the game as a whole rather than one object: its variables and the scene
//! running.
use super::*;
pub(super) fn definitions() -> Vec<OperationDef> {
    use PortType::*;
    // A connected Chave replaces the parameter; the empty key is the variable's own value.
    let key = || port("key", "Chave", Text);
    vec![
        OperationDef {
            id: "variable.get",
            label: "Ler variável do jogo",
            category: "Jogo",
            inputs: vec![key()],
            outputs: vec![port("value", "Valor", Any)],
            params: vec![text("variable", "Variável", ""), text("key", "Chave", "")],
        },
        OperationDef {
            id: "variable.set",
            label: "Alterar variável do jogo",
            category: "Jogo",
            inputs: vec![
                port("exec", "Executar", Exec),
                key(),
                port("value", "Novo valor", Any),
            ],
            outputs: vec![port("exec", "Continuar", Exec)],
            params: vec![
                text("variable", "Variável", ""),
                text("key", "Chave", ""),
                number("value", "Novo valor", 0.),
            ],
        },
        op(
            "scene.current",
            "Cena atual",
            "Jogo",
            false,
            vec![],
            vec![port("scene", "Cena", Text), port("name", "Nome", Text)],
        ),
    ]
}
pub fn game_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "variable.get" => (
            "Fornece o valor atual de uma variável do jogo, que vale para o projeto todo e continua o mesmo ao mudar ou reiniciar a cena.",
            "Ao iniciar a cena, leia Moedas e grave o valor num atributo ligado a um texto da interface: a contagem segue de uma cena para outra.",
            "Sem chave, o nó lê o valor único da variável. Com uma chave, lê o valor guardado para ela; uma chave em que nada foi gravado dá o valor inicial. Ligue o Nome de Cena atual à Chave para ler um valor por cena. A cada partida, as variáveis começam no valor inicial: ao Jogar de novo no editor e ao abrir o jogo.",
        ),
        "variable.set" => (
            "Muda o valor de uma variável do jogo. O novo valor vale para toda a lógica do projeto e continua depois de mudar ou reiniciar a cena.",
            "Ao coletar uma moeda, some 1 ao valor de Moedas e grave o resultado; a cena seguinte lê o total.",
            "O Novo valor precisa ser do tipo da variável, o do valor inicial dela: outro tipo interrompe o comportamento do objeto com erro. Com uma chave, o nó grava um valor só dela, e o valor único e as outras chaves ficam como estão; ligue o Nome de Cena atual à Chave para guardar um valor por cena. Cada variável guarda até 1024 chaves, e textos e chaves têm até 4096 bytes.",
        ),
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
