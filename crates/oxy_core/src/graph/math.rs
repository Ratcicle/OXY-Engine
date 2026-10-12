//! Number and text data operations: arithmetic, functions, limits and time formatting.
use super::*;
pub const ARITHMETIC: [(&str, &str); 7] = [
    ("+", "Somar · A + B"),
    ("-", "Subtrair · A − B"),
    ("*", "Multiplicar · A × B"),
    ("/", "Dividir · A ÷ B"),
    ("%", "Resto · A mod B"),
    ("min", "Mínimo · menor entre A e B"),
    ("max", "Máximo · maior entre A e B"),
];
pub(super) fn definitions() -> Vec<OperationDef> {
    use PortType::*;
    let result = || vec![port("value", "Resultado", Number)];
    vec![
        op(
            "math.binary",
            "Operação aritmética",
            "Dados",
            false,
            vec![
                number("a", "A", 0.),
                number("b", "B", 1.),
                text("operator", "Operação", "+"),
            ],
            result(),
        ),
        op(
            "math.unary",
            "Função matemática",
            "Dados",
            false,
            vec![
                number("value", "Valor", 0.),
                text("operator", "Função", "floor"),
            ],
            result(),
        ),
        op(
            "math.clamp",
            "Limitar entre mínimo e máximo",
            "Dados",
            false,
            vec![
                number("value", "Valor", 0.),
                number("min", "Mínimo", 0.),
                number("max", "Máximo", 1.),
            ],
            result(),
        ),
        op(
            "text.time",
            "Formatar tempo",
            "Dados",
            false,
            vec![
                number("seconds", "Segundos", 0.),
                text("format", "Formato", "mm:ss.cc"),
            ],
            vec![port("text", "Texto", Text)],
        ),
    ]
}
pub fn math_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "math.binary" => (
            "Calcula A e B pela operação escolhida: soma, subtração, multiplicação, divisão, resto, mínimo ou máximo.",
            "Energia menos Custo fornece a nova energia; o resto de Contador por 2 alterna entre 0 e 1.",
            "Dividir ou tirar o resto por zero gera diagnóstico. O resto nunca é negativo: −1 mod 3 dá 2. Confira a ordem A e B ao subtrair, dividir ou tirar o resto.",
        ),
        "math.unary" => (
            "Aplica uma função a um número: piso, teto, arredondar, truncar, valor absoluto, sinal ou raiz quadrada.",
            "O piso de 2,7 é 2 e o teto é 3; o valor absoluto de uma diferença dá a distância entre dois números.",
            "Arredondar leva as metades para longe do zero (2,5 vira 3; −2,5 vira −3). Truncar descarta a parte fracionária (−2,7 vira −2), e Piso desce (−2,7 vira −3). Sinal fornece −1, 0 ou 1. Raiz quadrada de número negativo gera diagnóstico.",
        ),
        "math.clamp" => (
            "Mantém um número entre um mínimo e um máximo.",
            "Limite Vida mais Cura entre 0 e 100 antes de alterar o atributo.",
            "Um valor dentro do intervalo passa sem mudar. Mínimo maior que o máximo gera diagnóstico.",
        ),
        "text.time" => (
            "Escreve uma quantidade de segundos como texto de tempo, como 01:23.45.",
            "A cada passo, formate o atributo Tempo e grave o texto num atributo ligado a um texto da interface.",
            "Trunca em vez de arredondar: 59,999 segundos mostram 00:59.99. A parte decimal usa ponto. Nos formatos com minutos, eles passam de 59 (62:05.00); em h:mm:ss, contam as horas. ss.cc mostra os segundos totais com dois dígitos ao menos (00.50). Tempo negativo ganha um - à frente; valor não finito gera diagnóstico.",
        ),
        _ => return None,
    })
}
pub fn math_choices(
    operation: &str,
    parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match (operation, parameter) {
        ("math.binary", "operator") => &ARITHMETIC,
        ("math.unary", "operator") => &[
            ("floor", "Piso"),
            ("ceil", "Teto"),
            ("round", "Arredondar"),
            ("trunc", "Truncar"),
            ("abs", "Valor absoluto"),
            ("sign", "Sinal"),
            ("sqrt", "Raiz quadrada"),
        ],
        ("text.time", "format") => &[
            ("mm:ss.cc", "mm:ss.cc · 01:23.45"),
            ("m:ss.cc", "m:ss.cc · 1:23.45"),
            ("mm:ss", "mm:ss · 01:23"),
            ("ss.cc", "ss.cc · 83.45 (segundos totais)"),
            ("h:mm:ss", "h:mm:ss · 0:01:23"),
            ("mm:ss.mmm", "mm:ss.mmm · 01:23.456"),
        ],
        _ => return None,
    })
}
