//! Offline help keyed by the same stable operation identities as the executable registry.
use crate::{document::Project, graph::OperationDef};
pub const FIRST_BEHAVIOR: &str = "1. Crie um objeto; ele será o responsável pelo comportamento. Selecione-o e abra Lógica.\n2. Em Ações de entrada, crie Mostrar mensagem e escolha K.\n3. Adicione Ação de entrada. Escolha Mostrar mensagem e Pressionar.\n4. Adicione Mensagem de diagnóstico e escreva Minha primeira ação funciona!\n5. Conecte a saída de execução do evento à entrada de execução da mensagem.\n6. Use Jogar e pressione K. Abra Console manualmente para ler o resultado. Parar descarta o teste.\nA receita abaixo contém exatamente esses objetos e conexões, editáveis como qualquer projeto.";
pub const FOUNDATIONS: &str = "Execução (conexões brancas) decide quando uma ação acontece. Dados carregam números, textos, booleanos e referências; os tipos devem ser compatíveis. Ler um atributo consulta o valor atual, inclusive depois de outra ação o alterar.\nParâmetros são valores padrão. Uma porta de dados conectada fornece o valor usado no lugar do parâmetro correspondente.\nPróprio / nenhum como alvo usa o objeto responsável pelo grafo. A saída Objeto de um evento pode fornecer outro objeto: numa área, é quem entrou. Conecte essa referência explicitamente quando quiser agir sobre ele.\nAtributos são os mesmos do painel. Criar Vida não cria regras de morte. O controlador pronto oferece movimento e gravidade sem exigir montar todos os cálculos por nós.\nSequência dispara seus ramos em ordem, mas não aguarda terminar um ramo antes do seguinte. Esperar adia somente a continuação daquele ramo. Não há junção automática.\nO runtime limita o trabalho e cancela tarefas ao remover seu responsável, parar o teste ou mudar de cena. Ciclos de execução sem suporte são recusados. O Console mostra objeto/nó e a trilha recente; uma recusa comum não abre o painel automaticamente.\nLua, nós personalizados, junções de ramos e scripts avançados não estão disponíveis nesta versão.";
pub struct Topic {
    pub operation: &'static OperationDef,
    pub purpose: &'static str,
    pub example: &'static str,
    pub caution: &'static str,
}
impl Topic {
    pub fn requirements(&self) -> &'static str {
        match self.operation.id {
            "event.step" => {
                "Conecte Executar às ações que devem ocorrer antes do movimento, uma vez por passo fixo."
            }
            "event.character" => {
                "Escolha um personagem 3D e a transição desejada. Os dados do evento preservam aquele instante mesmo após Esperar."
            }
            id if id.starts_with("character.") => {
                "O alvo precisa do componente Personagem 3D com Corpo de movimento válido. Alvo vazio usa o responsável; uma referência conectada deve conter um objeto válido. Ações exigem entrada Executar."
            }
            id if id.starts_with("camera.") => {
                "Use uma câmera de jogo. Para seguir um personagem, configure seu rig e alvo. Ler câmera fornece a ativa e a orientação do controle; as ações usam a entrada Executar."
            }
            id if id.starts_with("query.") => {
                "Use numa cena 3D, com origem e direção em coordenadas mundiais. Conecte Executar e verifique Consulta válida antes de usar o contato. Limite de 256 consultas explícitas por passo."
            }
            "surface.get" | "surface.apply" => {
                "O alvo precisa de Colisor 3D. Crie a superfície física no inspetor e vincule seu ID; Aplicar exige entrada Executar."
            }
            "event.input" => {
                "Crie uma ação em Ações de entrada, escolha a tecla e vincule-a ao nó. O jogo deve estar rodando com a entrada capturada."
            }
            "event.area_enter" | "event.area_exit" => {
                "O responsável precisa de um colisor ativo marcado como área. O visitante também precisa de colisor; objetos sem caixa não entram na detecção."
            }
            "event.animation" => {
                "O responsável deve ter um clip com o marcador de mesmo nome, iniciado por Reproduzir animação."
            }
            "event.click" => {
                "Escolha um objeto visual clicável ou adicione um botão de interface ao responsável pelo grafo."
            }
            "attribute.get" | "attribute.set" | "action.damage" => {
                "Crie o atributo no painel do objeto correto e escreva o mesmo nome no nó. Dano exige um atributo numérico. Forneça o alvo pela porta ou por referência explícita; vazio usa o responsável."
            }
            "action.animation" => {
                "Crie o clip no Estúdio do modelo e escolha o nome no nó. O alvo deve possuir esse clip."
            }
            "action.sound" => {
                "Importe um WAV válido na Biblioteca e escolha esse recurso no nó. A reprodução requer um dispositivo de áudio disponível."
            }
            "action.spawn" | "action.remove" | "action.component" => {
                "Indique um objeto existente ou conecte uma referência de objeto. Confirme qual componente ou hierarquia será afetado."
            }
            "action.scene" => "Crie a cena de destino no projeto e escolha-a no parâmetro do nó.",
            "condition.branch" | "control.sequence" | "control.wait" | "debug.message" => {
                "Conecte uma saída de execução à entrada Executar. Forneça os dados pedidos por portas conectadas ou pelos parâmetros do painel."
            }
            "event.scene_start" => {
                "O objeto com este grafo precisa estar na cena iniciada. Ligue a saída Executar à primeira ação."
            }
            _ => {
                "Conecte a saída de dados a uma entrada do tipo correspondente. Esse nó fornece valores quando consultado; sozinho não inicia um comportamento."
            }
        }
    }
}
pub fn topics() -> &'static [Topic] {
    static TOPICS: std::sync::OnceLock<Vec<Topic>> = std::sync::OnceLock::new();
    TOPICS.get_or_init(||crate::graph::registry().iter().map(|operation| {
        let (purpose,example,caution)=match operation.id {
            "event.scene_start"=>("Inicia um fluxo quando esta cena começa.","Conecte a uma mensagem para verificar que o objeto está na cena.","O evento acontece novamente ao iniciar outra instância da cena; não a cada atualização."),
            "event.input"=>("Inicia um fluxo ao pressionar, manter ou soltar uma ação configurada na Lógica.","Escolha Mostrar mensagem, tecla K, modo Pressionar; conecte a Mensagem de diagnóstico.","Pressionar e Soltar são transições únicas. Manter executa uma vez por passo fixo; é adequado para ações contínuas. Pausa e campos de texto não recebem controles do jogo."),
            "event.click"=>("Inicia o comportamento ao clicar no objeto ou em seu botão de interface.","Um botão de carta inicia uma condição de energia antes de aplicar o efeito.","O grafo precisa pertencer ao objeto clicado; uma referência visual não cria uma ligação de clique."),
            "event.area_enter"=>("Detecta entrada em uma área e fornece o objeto que entrou.","Ligue Objeto à leitura do atributo Chave do visitante.","Adicione um colisor ativo marcado como área ao responsável. Não procure o visitante por um nome fixo."),
            "event.area_exit"=>("Executa quando o visitante sai de uma área e fornece esse objeto.","Use a referência do visitante para desativar um efeito temporário ao sair.","Um corpo de movimento 3D rápido pode entrar e sair no mesmo passo, nessa ordem. Desativar ou remover uma área cancela a ocupação; não simula uma travessia. Objetos legados usam a sobreposição entre passos."),
            "event.animation"=>("Inicia um fluxo quando a reprodução atravessa um marcador de animação.","Use Impacto para ativar a área do golpe.","O marcador precisa existir no clip reproduzido pelo responsável. Ficar parado perto do instante não repete o evento."),
            "value.number"=>("Fornece um número constante.","Use 2 como custo de uma carta.","Conecte a uma entrada numérica; textos não viram números automaticamente."),
            "value.text"=>("Fornece um texto constante.","Defina uma mensagem ou conteúdo textual para um atributo.","O texto não procura objetos pelo nome; use referências de objeto."),
            "value.bool"=>("Fornece verdadeiro ou falso.","Use verdadeiro ao registrar que uma chave foi coletada.","Uma condição espera um booleano, não o texto verdadeiro."),
            "value.object"=>("Fornece a referência de um objeto da cena.","Escolha o alvo de um efeito de carta.","Se o objeto for removido durante o teste, a referência deixa de resolver; verifique o diagnóstico."),
            "attribute.get"=>("Lê o atributo do objeto informado.","Ligue o visitante de uma área ao alvo e escolha Chave.","Sem alvo conectado ou explícito, lê o responsável. O nome do atributo deve corresponder ao valor que você criou."),
            "attribute.set"=>("Altera um atributo usando o valor fornecido.","Depois de verificar energia suficiente, grave Energia menos Custo.","Nada associa automaticamente Vida à morte. Monte as condições e ações desejadas."),
            "math.binary"=>("Soma, subtrai, multiplica ou divide A e B.","Energia menos Custo fornece a nova energia.","Dividir por zero gera diagnóstico. Confira a ordem A e B ao subtrair ou dividir."),
            "condition.compare"=>("Compara A e B e fornece um booleano.","Energia maior ou igual a Custo permite jogar a carta.","Comparar fornece dados; conecte o resultado à Condição para escolher um fluxo de execução."),
            "condition.branch"=>("Escolhe somente a saída Verdadeiro ou Falso.","Verdadeiro desconta energia e aplica dano; Falso mostra uma mensagem.","A porta de execução inicia a decisão; conectar somente o dado não executa ações."),
            "action.damage"=>("Subtrai dano de um atributo numérico configurável do alvo.","A área de um golpe aplica 25 ao atributo Vida do objeto que entrou.","Uma vez por ativação e alvo evita repetição do golpe, inclusive após uma espera. Ativações distintas podem causar novo dano."),
            "action.animation"=>("Reproduz um clip de animação do objeto escolhido.","A ação de entrada inicia Ataque; seu marcador Impacto inicia o acerto.","O clip deve existir nesse objeto. Não confunda a peça de uma trilha com o modelo que contém o clip."),
            "action.sound"=>("Solicita reprodução de um WAV importado, com volume configurável.","O marcador Passo pode acionar um som do projeto.","Importe o WAV na Biblioteca. Arquivo ou dispositivo inválido é diagnosticado; não há áudio espacial ou mixer avançado."),
            "action.spawn"=>("Cria uma cópia da hierarquia indicada durante o jogo.","Crie uma cópia de um objeto de efeito e use a saída Objeto para configurar sua continuação.","É uma instância do runtime. Parar descarta as criações; referências internas da hierarquia são remapeadas."),
            "action.remove"=>("Remove um objeto e seus descendentes do jogo.","Após registrar Chave no visitante, remova o coletável.","As tarefas do objeto removido são canceladas. A alteração não apaga o documento de edição."),
            "action.component"=>("Ativa ou desativa o componente escolhido.","Ative a colisão da área de ataque, espere sua duração e desative-a.","Visibilidade, comportamento, controlador, animação e colisão são estados distintos. Ocultar a peça não desativa seu colisor."),
            "action.scene"=>("Encerra a cena atual e abre o destino configurado.","Uma passagem usa a área para iniciar a segunda cena.","Escolha uma cena existente. Tarefas e ativações da cena anterior são canceladas."),
            "control.sequence"=>("Dispara as saídas 1, 2 e 3 nessa ordem.","Inicie som e animação a partir do mesmo evento.","Não aguarda que um ramo termine. Para esperar antes da próxima ação, conecte-as em um mesmo ramo através de Esperar."),
            "control.wait"=>("Suspende somente a continuação por um tempo, sem bloquear a simulação.","Após ativar uma área, espere 0,15 segundo e desative-a.","O tempo é da simulação: pausar o jogo suspende a espera. Remover o responsável, Parar ou mudar de cena cancela a tarefa."),
            "debug.message"=>("Registra uma mensagem no Console durante o teste.","Ligue à ação K para verificar seu primeiro fluxo.","Abra Console manualmente. O nó não abre painéis nem produz uma notificação por execução."),
            _=>crate::graph::movement_help(operation.id).unwrap_or(("Operação ainda sem documentação pedagógica.","Consulte as portas e os parâmetros disponíveis.","Relate esta lacuna antes de usar a operação em um projeto importante.")),
        };Topic {operation,purpose,example,caution}
    }).collect())
}
pub fn first_recipe() -> Result<Project, String> {
    crate::guide_recipes::load(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_registered_operation_has_help_and_first_recipe_runs_as_data() {
        assert_eq!(topics().len(), crate::graph::registry().len());
        assert!(topics().iter().all(|t| !t.purpose.contains("ainda sem")));
        let project = first_recipe().unwrap();
        let before = project.clone();
        let action = project.input_bindings.keys().next().unwrap().clone();
        let mut runtime = crate::runtime::Runtime::new(&project, &project.start_scene).unwrap();
        runtime.advance(
            crate::runtime::FIXED_DT,
            &crate::runtime::InputFrame {
                pressed: [action].into(),
                ..Default::default()
            },
        );
        assert_eq!(
            runtime
                .logs
                .iter()
                .filter(|l| l.contains("Minha primeira ação funciona!"))
                .count(),
            1
        );
        assert_eq!(project, before);
    }
}
