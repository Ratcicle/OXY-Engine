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
    static RECIPES: [Recipe; 12] = [
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
        Recipe {
            title: "Primeira pessoa por nós",
            setup: "Crie o grupo Jogador e use + Adicionar componente → Personagem 3D. Coloque as peças visuais como filhos. + Objeto → Câmera cria outra entidade: nela, adicione Controle da câmera e escolha Alvo = Jogador. Mantenha a câmera na raiz. No Básico do personagem, desative Ler ações de movimento automaticamente.",
            flow: "A cada passo → Definir intenção; Ler eixos: Movimento → Intenção. Ação Pular → Solicitar pulo. Ler ação Correr/Agachar: Mantida → Comando de postura. O mouse pertence ao Controle da câmera.",
            expected: "WASD move e Espaço pula. Remova a conexão de intenção: ela expira no próximo passo, sem velocidade desejada presa.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/06-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Trocar câmera por nós",
            setup: "Jogador e Câmera principal são objetos separados na Hierarquia. A câmera tem Controle da câmera, com Alvo = Jogador. Crie ações 1 e 2; os nós de câmera apontam para Câmera principal, não para o Jogador.",
            flow: "Ação 1 → Trocar modo: Primeira pessoa. Ação 2 → Trocar modo: Terceira pessoa. Transição 0,25 s; o controle de colisão continua no personagem.",
            expected: "Troque enquanto pula: velocidade e apoio continuam. Q muda o ombro; o volume da câmera evita paredes.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/07-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Gelo e piso comum",
            setup: "O piso possui colisor 3D e vínculo de superfície. As duas superfícies são recursos editáveis do projeto.",
            flow: "Ação 1 → Aplicar superfície: Comum. Ação 2 → Aplicar superfície: Gelo. O alvo é o piso, e a referência de superfície usa porta própria.",
            expected: "Ande e solte a direção. Gelo freia menos. A cor do material visual não muda nem controla o atrito.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/08-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Pulo e impulso",
            setup: "O grupo tem Personagem 3D. Crie Solicitar salto em K e Impulso em I.",
            flow: "K → Solicitar pulo. I → Alterar velocidade: Somar, vetor mundial (0, 7, -8) m/s. A operação soma à velocidade atual, sem multiplicar por dt.",
            expected: "K respeita chão e antecipação. I também atua no ar e preserva o embalo lateral; o Console informa parâmetros inválidos.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/09-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Checkpoint e reinício",
            setup: "A área azul tem sensor e o personagem possui Destino (Vetor3), Giro salvo (Número), Olhar salvo (Vetor2) e Checkpoint (Texto).",
            flow: "Ao entrar: Objeto do evento → Ler posição/rotação e alvos de Alterar atributo. Guarde posição/giro e Ler câmera: Olhar. R → Teleportar, recebendo os atributos; mantenha Restabelecer giro/olhar e consulte Sucesso.",
            expected: "Atravesse a área, olhe para outro lado e pressione R. Posição, orientação e olhar voltam; velocidade/buffers anteriores são descartados. Um destino ocupado é recusado ou usa busca local até 0,5 m.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/10-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Bloquear entrada durante queda",
            setup: "O personagem começa acima do chão. Crie ações B e N. Bloquear entrada atua por motivo, não desliga o componente.",
            flow: "B → Bloquear movimento, motivo teste de queda. N → Liberar movimento com o mesmo motivo. Gravidade, apoio e impulsos continuam simulando.",
            expected: "Pressione B enquanto cai: o corpo aterrissa normalmente, mas WASD não cria intenção. N libera apenas o bloqueio desse motivo.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/11-movimento-3d/project.oxy.json"
            )),
        },
        Recipe {
            title: "Saltos manuais e automáticos",
            setup: "Aplique o perfil Saltos encadeados; os valores continuam editáveis. Crie ações 1 e 2.",
            flow: "1 → Alterar opção: Pulo automático desligado. 2 → a mesma opção ligada. Movimento e olhar continuam nas ações remapeáveis.",
            expected: "No modo manual, manter Espaço não salta de novo. No automático, saltos encadeiam nos pousos. Mouse sozinho não gera velocidade.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/12-movimento-3d/project.oxy.json"
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
pub fn movement_laboratory() -> Result<Project, String> {
    let project = crate::migration::read(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/laboratorio-3d/project.oxy.json"
    )))?;
    crate::document::validate_project(&project)?;
    Ok(project)
}
