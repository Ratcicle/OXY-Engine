//! Operations about the game as a whole rather than one object: its variables, the progress
//! saved between runs, the scene running, the game's own pause, the cursor and quitting.
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
            "save.write",
            "Salvar progresso",
            "Jogo",
            true,
            vec![],
            vec![],
        ),
        op(
            "save.clear",
            "Apagar progresso",
            "Jogo",
            true,
            vec![],
            vec![],
        ),
        op(
            "save.state",
            "Ler progresso salvo",
            "Jogo",
            false,
            vec![],
            vec![port("exists", "Existe", Bool)],
        ),
        op(
            "scene.current",
            "Cena atual",
            "Jogo",
            false,
            vec![],
            vec![port("scene", "Cena", Text), port("name", "Nome", Text)],
        ),
        op(
            "game.pause",
            "Pausar / retomar jogo",
            "Jogo",
            true,
            vec![text("mode", "Ação", "toggle")],
            vec![],
        ),
        op(
            "game.read",
            "Ler estado do jogo",
            "Jogo",
            false,
            vec![],
            vec![port("paused", "Pausado", Bool)],
        ),
        op(
            "game.cursor",
            "Mostrar / esconder cursor",
            "Jogo",
            true,
            vec![text("mode", "Ação", "show")],
            vec![],
        ),
        // Like Mudar de cena, nothing follows it.
        OperationDef {
            id: "game.quit",
            label: "Sair do jogo",
            category: "Jogo",
            inputs: vec![port("exec", "Executar", Exec)],
            outputs: vec![],
            params: vec![],
        },
    ]
}
pub fn game_help(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match id {
        "variable.get" => (
            "Fornece o valor atual de uma variável do jogo, que vale para o projeto todo e continua o mesmo ao mudar ou reiniciar a cena.",
            "Ao iniciar a cena, leia Moedas e grave o valor num atributo ligado a um texto da interface: a contagem segue de uma cena para outra.",
            "Sem chave, o nó lê o valor único da variável. Com uma chave, lê o valor guardado para ela; uma chave em que nada foi gravado dá o valor inicial. Ligue o Nome de Cena atual à Chave para ler um valor por cena. A cada partida, as variáveis começam no valor inicial: ao Jogar de novo no editor e ao abrir o jogo; as marcadas como Salva entre partidas começam no progresso salvo, quando há (veja Salvar progresso).",
        ),
        "variable.set" => (
            "Muda o valor de uma variável do jogo. O novo valor vale para toda a lógica do projeto e continua depois de mudar ou reiniciar a cena.",
            "Ao coletar uma moeda, some 1 ao valor de Moedas e grave o resultado; a cena seguinte lê o total.",
            "O Novo valor precisa ser do tipo da variável, o do valor inicial dela: outro tipo interrompe o comportamento do objeto com erro. Com uma chave, o nó grava um valor só dela, e o valor único e as outras chaves ficam como estão; ligue o Nome de Cena atual à Chave para guardar um valor por cena. Cada variável guarda até 1024 chaves, e textos e chaves têm até 4096 bytes.",
        ),
        "save.write" => (
            "Salva as variáveis do jogo marcadas como Salva entre partidas, com todas as chaves: na próxima vez que o jogo abrir, elas começam nesses valores.",
            "Depois de Alterar variável do jogo gravar um novo recorde, ligue Continuar a Salvar progresso: o recorde volta na próxima partida.",
            "Nada é salvo sem este nó: mudar uma variável não basta. As variáveis que não são salvas entre partidas ficam de fora. No teste do editor, o progresso vai para o progresso do teste, que dura até abrir outro projeto, nunca para o do jogo exportado. Salve em momentos marcados, como ao concluir uma etapa, e não a cada passo.",
        ),
        "save.clear" => (
            "Apaga o progresso salvo e volta as variáveis salvas entre partidas ao valor inicial, com todas as chaves.",
            "Num botão Recomeçar do menu, ligue Ao clicar a Apagar progresso: o jogo volta a ser como na primeira vez.",
            "No jogo exportado, não há como desfazer. As variáveis que não são salvas entre partidas continuam como estão. No teste do editor, apaga só o progresso do teste.",
        ),
        "save.state" => (
            "Fornece Existe: verdadeiro quando há progresso salvo, de uma partida anterior ou desta.",
            "Ao iniciar a cena do menu, ligue Existe à Condição de Se / Senão para mostrar o botão Continuar só quando há progresso.",
            "Existe passa a verdadeiro depois de Salvar progresso e a falso depois de Apagar progresso. Para ler os valores, use Ler variável do jogo: as variáveis salvas já começam neles.",
        ),
        "scene.current" => (
            "Fornece a cena em que o objeto está: o identificador, para Mudar de cena, e o nome, para chaves e textos.",
            "Ligue a saída Cena à entrada Cena de Mudar de cena para reiniciar a cena. A mesma lógica, copiada para outra cena ou colocada a partir de um modelo, reinicia a cena em que estiver.",
            "Cena é o identificador estável, que não muda ao renomear a cena: é o que Mudar de cena espera. Nome é o texto mostrado, que pode mudar ou se repetir: use-o em textos e chaves, não como destino. Para abrir outra cena, escolhida pelo nome, use Referência de cena.",
        ),
        "game.pause" => (
            "Pausa, retoma ou alterna a pausa do jogo. Na pausa, o mundo para: movimento, animações, áreas, A cada passo e Esperar, e os efeitos sonoros congelam; a música continua. As ações de entrada, os cliques na interface e a lógica continuam, e o mouse fica livre.",
            "Num botão Continuar de um menu de pausa, ligue Ao clicar a Pausar / retomar jogo com a Ação Retomar. Numa ação de entrada, Alternar pausa e retoma com a mesma tecla.",
            "Cada mudança dispara Ao pausar ou retomar na cena, que pode mostrar e esconder o menu; pedir o estado em que o jogo já está não faz nada. Uma espera iniciada durante a pausa só termina depois de retomar. As ações de entrada continuam disparando na pausa: use Ler estado do jogo para ignorar as que movem o jogo. Mudar de cena termina a pausa. Esta é a pausa do jogo; a da engine (Pausar no editor, perda de foco) para também a lógica, e as duas podem acontecer juntas.",
        ),
        "game.read" => (
            "Fornece Pausado: verdadeiro durante a pausa do jogo.",
            "Numa ação de entrada que dispara um efeito, ligue Pausado à Condição de Se / Senão e siga pelo Falso: com o jogo pausado, o efeito não acontece.",
            "Lê só a pausa do jogo, a de Pausar / retomar jogo: na pausa da engine a lógica não executa. Para agir quando a pausa muda, use o evento Ao pausar ou retomar.",
        ),
        "game.cursor" => (
            "Mostra o cursor e solta o mouse, ou o esconde e devolve o mouse à câmera, sem pausar o jogo: o mundo, a lógica e as ações de entrada continuam.",
            "Ao mostrar uma tela de resultado ou um menu que não pausa o jogo, use este nó com a Ação Mostrar para os botões receberem cliques; no Ao clicar do botão que volta a jogar, com a Ação Esconder.",
            "Com o cursor à mostra, o mouse não gira a câmera, que fica como estava, e o Olhar de Ler eixos de entrada vale (0, 0); os cliques chegam aos botões da interface e aos objetos, e um clique num botão não aciona também a ação de entrada do botão do mouse. O personagem continua andando com as teclas: para pará-lo, use Bloquear / liberar entrada por motivo ou a pausa do jogo. Esconder só prende o mouse quando a câmera ativa gira com ele (Primeira ou Terceira pessoa); na pausa do jogo, o cursor fica livre de qualquer forma. Mudar de cena volta ao padrão: a câmera que gira com o mouse volta a prendê-lo. Esc continua pausando: o menu de pausa do jogo, quando a cena tem um, ou o teste.",
        ),
        "game.quit" => (
            "Encerra o jogo: o jogo exportado fecha a janela.",
            "Num botão Sair de um menu, ligue Ao clicar a Sair do jogo.",
            "No teste do editor, para o teste, como Parar, e escreve uma linha no Console; numa simulação dos agentes, encerra a simulação. O que ainda falta da lógica no mesmo passo executa. Para o progresso continuar na próxima partida, ligue antes Salvar progresso.",
        ),
        _ => return None,
    })
}
/// Fixed values of the parameters of these operations.
pub fn game_choices(
    operation: &str,
    parameter: &str,
) -> Option<&'static [(&'static str, &'static str)]> {
    Some(match (operation, parameter) {
        ("game.pause", "mode") => &[
            ("pause", "Pausar"),
            ("resume", "Retomar"),
            ("toggle", "Alternar"),
        ],
        ("game.cursor", "mode") => &[("show", "Mostrar"), ("hide", "Esconder")],
        _ => return None,
    })
}
