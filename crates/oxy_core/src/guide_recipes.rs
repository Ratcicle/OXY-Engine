//! Human instructions and embedded, editable project documents for the offline guide.
use crate::{document::Project, persistence, script::ScriptSources};
use std::path::{Path, PathBuf};
pub struct Recipe {
    pub title: &'static str,
    pub setup: &'static str,
    pub flow: &'static str,
    pub expected: &'static str,
    pub bytes: &'static [u8],
}
pub fn recipes() -> &'static [Recipe] {
    static RECIPES: [Recipe; 14] = [
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
        Recipe {
            title: "Menu de pausa",
            setup: "O grupo Menu de pausa começa oculto e contém o grafo. Seus filhos: o Fundo da pausa, uma Imagem de interface sem imagem, que desenha um retângulo na Cor (com transparência); o título Pausa; e os botões Continuar, Reiniciar e Sair do jogo. A/D movem; Espaço pula.",
            flow: "No grupo: Ao pausar ou retomar, Quando = Ao pausar → ativar Visibilidade; Quando = Ao retomar → desativar Visibilidade (Objeto vazio = o próprio grupo, e os filhos o acompanham). Continuar: Ao clicar → Pausar / retomar jogo, Ação Retomar. Reiniciar: Ao clicar → Mudar de cena; Cena atual: Cena → Cena. Sair do jogo: Ao clicar → Sair do jogo.",
            expected: "Com o evento na cena, Esc pausa o jogo e abre o menu: o personagem para e o mouse fica livre, sem nó de cursor. Continuar, ou Esc de novo, fecha o menu e o jogo segue de onde parou. Reiniciar recarrega a cena como no começo, já sem pausa. Sair do jogo para o teste no editor e fecha o jogo exportado.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/13-menu-de-pausa/project.oxy.json"
            )),
        },
        Recipe {
            title: "Recorde salvo",
            setup: "A variável do jogo Melhor tempo (número, 0 = sem recorde) é Salva entre partidas. A cena se chama Fase 1. A área verde Chegada contém o grafo; o Placar guarda Início, Tempo e Recorde, que os textos da interface exibem. A/D movem; Espaço pula; a Parede segura o personagem.",
            flow: "Ler variável e Alterar variável recebem na Chave o Nome de Cena atual. Ao iniciar cena → Alterar Início do Placar com o Tempo de Ler tempo da simulação → se Melhor tempo > 0, Formatar tempo → Recorde. Ao entrar na área: o Tempo de Ler tempo da simulação menos Início dá o tempo da tentativa, que, formatado, vai para o Tempo do Placar. Se Melhor tempo == 0, ou se a tentativa for mais rápida → Alterar variável → Salvar progresso → Recorde, com Melhor tempo lido de novo. Senão, só uma mensagem no Console.",
            expected: "Chegue à área: o tempo vira o recorde e o progresso é salvo, como Melhor tempo · Fase 1. Pare e jogue de novo: o recorde aparece desde o começo, porque o teste do editor guarda o progresso até você abrir outro projeto; no jogo exportado, ele volta na próxima partida. Mais devagar, o recorde fica e nada é salvo; mais rápido, ele é trocado e salvo de novo.",
            bytes: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia/14-recorde-salvo/project.oxy.json"
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
/// A recipe in Luau: a project in folders (examples/guia-scripts) with the objects and IDs of
/// the node recipe of the same title, whose logic its scripts do.
pub struct ScriptRecipe {
    pub title: &'static str,
    pub setup: &'static str,
    /// What the scripts do, in place of the node recipe's connections.
    pub script: &'static str,
    pub expected: &'static str,
    pub bytes: &'static [u8],
    /// Each script by its path from the project folder, with its text.
    pub scripts: &'static [(&'static str, &'static str)],
}
macro_rules! script_recipe {
    ($folder:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/guia-scripts/",
            $folder,
            "/project.oxy.json"
        ))
    };
    ($folder:literal, $path:literal) => {
        (
            $path,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../examples/guia-scripts/",
                $folder,
                "/",
                $path
            )),
        )
    };
}
pub fn script_recipes() -> &'static [ScriptRecipe] {
    static RECIPES: [ScriptRecipe; 3] = [
        ScriptRecipe {
            title: "Porta com chave",
            setup: "Personagem: retângulo, controlador, colisor sólido e atributo booleano Chave. Porta: colisor sólido. Sua área filha detecta o visitante e anexa o script area_da_porta.luau. Chão sólido não invade a área. A/D movem; Espaço pula.",
            script: "on_area_enter recebe quem entrou (other). Se other tem o atributo Chave e ele é verdadeiro, o script desliga o colisor da porta, o pai da área (self.parent), e a esconde. Senão, print registra a recusa no Console.",
            expected: "Com Chave = verdadeiro, aproximar-se libera a passagem. Pare, altere Chave para falso e jogue novamente: a porta continua sólida e o Console registra a recusa. O atributo é lido do visitante, não do dono do script.",
            bytes: script_recipe!("02-porta-com-chave"),
            scripts: &[script_recipe!("02-porta-com-chave", "area_da_porta.luau")],
        },
        ScriptRecipe {
            title: "Carta e energia",
            setup: "Estado da partida tem Energia = 3. Carta Raio é um botão de interface que anexa carta.luau, com os atributos que o script declara: Custo = 2, Dano = 20 e os objetos Estado e Alvo. O Alvo tem Vida = 50. Textos exibem os atributos pelo mesmo vínculo do runtime.",
            script: "on_click compara a Energia do Estado com o Custo desta carta. Se a energia basta, tira o Custo da Energia e o Dano da Vida do Alvo, sem passar de 0; senão, print explica e nada muda. Os atributos são lidos na hora do clique.",
            expected: "Primeiro clique: energia 1, vida 30. Segundo clique: energia e vida continuam 1 e 30; o Console explica energia insuficiente. Parar restaura 3 e 50.",
            bytes: script_recipe!("03-carta-e-energia"),
            scripts: &[script_recipe!("03-carta-e-energia", "carta.luau")],
        },
        ScriptRecipe {
            title: "Ataque por marcador",
            setup: "Modelo animável tem o clip Ataque (0,6 s), uma trilha rígida do Braço e o marcador Impacto em 0,15 s, e anexa ataque.luau. Área de acerto é filha do modelo, começa desativada e anexa area_de_acerto.luau, com Dano = 15. O Alvo tem colisor sólido e Vida = 100. J aciona Atacar.",
            script: "No modelo: on_input toca o clip Ataque quando Atacar é pressionada; on_marker, no Impacto, liga o colisor da Área de acerto e chama self:after(ABERTA, Ataque.fechar), com ABERTA = 0.15 (0,15 s); fechar desliga o colisor. Na área: on_area_enter tira o Dano da Vida de quem entrou, se self:first_hit(other) disser que é o primeiro acerto nele nesta ativação.",
            expected: "O braço gira e cada golpe completo reduz 15 de vida, mesmo que o alvo permaneça na área. O marcador abre uma ativação nova; o temporizador guarda o nome da função e mantém o intervalo sem congelar o jogo. Pausar suspende o temporizador; Parar o cancela.",
            bytes: script_recipe!("04-ataque-por-marcador"),
            scripts: &[
                script_recipe!("04-ataque-por-marcador", "area_de_acerto.luau"),
                script_recipe!("04-ataque-por-marcador", "ataque.luau"),
            ],
        },
    ];
    &RECIPES
}
/// The script recipe `index`: its project and its scripts, as a run takes them.
pub fn load_script_recipe(index: usize) -> Result<(Project, ScriptSources), String> {
    let recipe = script_recipes()
        .get(index)
        .ok_or("Receita não encontrada.")?;
    let project = crate::migration::read(recipe.bytes)?;
    crate::document::validate_project(&project)?;
    let scripts = ScriptSources::from_texts(recipe.scripts.iter().copied());
    Ok((project, scripts))
}
/// Writes a copy of the script recipe `index` in the new folder `folder`, as the guide opens it:
/// the project file and its scripts. Answers the project file.
pub fn write_script_recipe(index: usize, folder: &Path) -> Result<PathBuf, String> {
    let (project, sources) = load_script_recipe(index)?;
    let file = folder.join(persistence::PROJECT_FILE);
    persistence::save_project(&file, &project)?;
    for (path, script) in &sources.files {
        persistence::write_script(folder, path, &script.text, false)?;
    }
    Ok(file)
}
pub fn movement_laboratory() -> Result<Project, String> {
    let project = crate::migration::read(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/laboratorio-3d/project.oxy.json"
    )))?;
    crate::document::validate_project(&project)?;
    Ok(project)
}
