//! Human instructions and embedded, editable project documents for the offline guide.
use crate::document::Project;
pub struct Recipe {
    pub title: &'static str,
    pub setup: &'static str,
    pub flow: &'static str,
    pub expected: &'static str,
    pub bytes: &'static [u8],
}
pub fn recipes() -> &'static [Recipe] {
    static RECIPES: [Recipe; 5] = [
        Recipe {
            title: "Primeira mensagem",
            setup: "Um retângulo contém o grafo. Na Lógica, a ação Mostrar mensagem está ligada a K, no modo Pressionar.",
            flow: "Ação de entrada: Executar → Mensagem de diagnóstico: Executar. Clique Jogar, capture a entrada e pressione K. Abra Console manualmente.",
            expected: "Cada pressionamento registra Minha primeira ação funciona!. Manter K no modo Pressionar não repete a mensagem.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/01-primeiro-comportamento/project.oxy.json"
            )),
        },
        Recipe {
            title: "Porta com chave",
            setup: "Personagem: retângulo, controlador, colisor sólido e atributo booleano Chave. Porta: colisor sólido. Sua área filha detecta o visitante e contém o grafo. Chão sólido não invade a área. A/D movem; Espaço pula.",
            flow: "Ao entrar na área: Objeto do evento → Ler atributo: Objeto; atributo Chave. Valor → Comparar A; B = verdadeiro, comparação ==. Resultado → Se / Senão: Condição. Evento: Executar → Se / Senão: Executar. Verdadeiro → desativar Colisão da Porta → desativar Visibilidade da Porta. Falso → Mensagem de diagnóstico.",
            expected: "Com Chave = verdadeiro, aproximar-se libera a passagem. Pare, altere Chave para falso e jogue novamente: a porta continua sólida e o Console registra a recusa. O atributo é lido do visitante, não do dono do grafo.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/02-porta-com-chave/project.oxy.json"
            )),
        },
        Recipe {
            title: "Carta e energia",
            setup: "Estado da partida tem Energia = 3. Carta Raio é um botão de interface com Custo = 2 e contém o grafo. O Alvo tem Vida = 50. Textos exibem os atributos pelo mesmo vínculo do runtime.",
            flow: "Ao clicar → Se / Senão. Ler Energia do Estado e Custo desta carta → Comparar >= → Condição. No ramo Verdadeiro: Energia menos Custo → Alterar Energia do Estado → Aplicar dano 20 ao Alvo. Falso apenas registra mensagem. As portas de dados são lidas quando a ação executa.",
            expected: "Primeiro clique: energia 1, vida 30. Segundo clique: energia e vida continuam 1 e 30; a mensagem explica energia insuficiente. Parar restaura 3 e 50.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/03-carta-e-energia/project.oxy.json"
            )),
        },
        Recipe {
            title: "Ataque por marcador",
            setup: "Modelo animável tem o clip Ataque (0,6 s), uma trilha rígida do Braço e o marcador Impacto em 0,15 s. Área de acerto é filha do modelo, começa desativada e contém seu próprio grafo. O Alvo tem colisor sólido e Vida = 100. J aciona Atacar.",
            flow: "No modelo: Ação de entrada → Reproduzir animação Ataque. Marcador Impacto → ativar Colisão da Área de acerto → Esperar 0,15 s → desativar Colisão. Na área: Ao entrar: Executar → Aplicar dano; Objeto do evento → Objeto; quantidade 15, Uma vez por ativação/alvo ligada.",
            expected: "O braço gira e cada golpe completo reduz 15 de vida, mesmo que o alvo permaneça na área. O marcador abre uma ativação nova; a espera mantém o intervalo sem congelar o jogo. Pausar suspende a espera; Parar a cancela.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/04-ataque-por-marcador/project.oxy.json"
            )),
        },
        Recipe {
            title: "Passagem entre cenas",
            setup: "Cena Entrada tem personagem com controlador, chão e uma área azul chamada Passagem. Cena Destino é a segunda cena do mesmo projeto. A área não toca o chão; A/D movem o personagem.",
            flow: "Na Passagem, Ao entrar na área: Executar → Mudar de cena: Executar. O parâmetro Cena de destino aponta para Destino pelo ID estável. O nome mostrado no seletor é só um rótulo.",
            expected: "Caminhar até a área abre Destino. As tarefas da cena anterior são canceladas. Parar volta ao contexto de edição sem persistir o estado do teste.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/05-passagem-entre-cenas/project.oxy.json"
            )),
        },
    ];
    &RECIPES
}
pub fn load(index: usize) -> Result<Project, String> {
    let recipe = recipes().get(index).ok_or("Receita não encontrada.")?;
    let project = crate::migration::read(recipe.bytes)?;
    crate::document::validate_project(&project)?;
    Ok(project)
}
