# OXY Engine 0.1.3

Editor e runtime desktop próprios em Rust, com cenas 2D/3D, montagem por peças, pintura PNG, animação, atributos, lógica visual e execução independente. Interface e renderização compartilham egui/eframe 0.33.3 com wgpu 27.0.1; nenhuma engine pronta é usada como núcleo. Dependências fixadas e `Cargo.lock` mantido.

## Abrir e compilar

Windows 10/11 x64, driver atualizado e GPU compatível com DirectX 12 ou Vulkan. Extraia o ZIP inteiro em uma pasta gravável e abra **`OXY Engine.exe`** diretamente. O pacote inclui ícone, metadados de versão e **`oxy_player.exe`** separado. Mantenha `data` e as DLLs junto dos executáveis; Rust/Cargo são necessários apenas para desenvolver/compilar.

Para desenvolver, instale Rust **1.92.0**, rustfmt, Clippy e o linker MSVC com ferramentas C++/Windows SDK, ou MinGW-w64 completo. Na raiz do repositório, em PowerShell:

```powershell
cargo run --locked -p oxy_editor -- examples/validacao/project.oxy.json
cargo run --locked -p oxy_player -- examples/validacao/project.oxy.json
cargo build --workspace --release --locked
```

Sem argumentos, o editor mostra **Novo projeto**, **Abrir projeto**, **Projetos recentes** e **Projeto de exemplo**. O exemplo abre uma cópia temporária das três cenas; Salvar pede uma nova pasta e copia seus assets, preservando o original. **Projeto → Tela inicial** retorna ao início com confirmação de alterações pendentes. Recentes ficam em `%LOCALAPPDATA%/OXY Engine/recent-projects.json`; remover da lista não exclui arquivos. O editor também aceita o arquivo do projeto como argumento. O player aceita arquivo/pasta e, sem argumento, procura `data/project.oxy.json` ao lado do próprio executável. Nesta máquina, o auxiliar `scripts/cargo.ps1` configura o compilador GNU preparado:

```powershell
$oxyArgs = @('run','--locked','-p','oxy_editor','--','examples/validacao/project.oxy.json')
& .\scripts\cargo.ps1 @oxyArgs
```

## Criar → editar → testar → salvar

1. Em **Projeto**, crie e salve um projeto local. **+ Cena** adiciona uma cena 2D/3D; **Definir inicial** escolhe a cena do player. **+ Objeto** cria formas, grupos, câmeras e elementos de interface; a biblioteca importa cópias de PNG/WAV e guarda modelos compostos.
2. Selecione na hierarquia ou no viewport. **Ctrl+clique** adiciona/remove objetos; **Shift+clique** seleciona um intervalo na hierarquia. **F2** renomeia, Enter confirma e Escape cancela. Arraste sobre outro objeto para mudar o pai, ou sobre **Raiz da cena** para removê-lo, mantendo a transformação global. **Agrupar**, duplicar e excluir trabalham com a seleção e suas hierarquias.
3. **W/E/R** escolhem mover/girar/escalar; arraste os eixos ou use valores numéricos. A seleção múltipla gira/escala em torno do centro do conjunto. Duplo clique no mesmo objeto enquadra a seleção. Botão central move a câmera, roda amplia/reduz e arraste direito orbita no 3D; Alt suspende o encaixe durante o movimento.
4. Botão direito → **Abrir no Estúdio** preserva o objeto escolhido. Em Modelagem, novas peças usam a seleção como pai. **Salvar hierarquia como modelo** mantém peças, grupos e pivôs editáveis.
5. Em Pintura, crie 256×256/512×512 ou importe PNG. Pincel, preenchimento, conta-gotas e paleta alteram pixels na imagem ou na superfície. **Exportar PNG** salva a imagem; **Remover textura** retira apenas o vínculo. **Substituir**, **Editar no Estúdio** e **Localizar na biblioteca** ficam junto à miniatura. Antes de pintar uma textura compartilhada, escolha editar o original ou criar uma cópia independente.
6. Em Animação, o modelo vem da raiz/grupo selecionado ou do ancestral animado; **Modelo** permite escolha manual. Use **+ Nova animação**, F2 e o menu da lista para renomear/duplicar/excluir. Exclusões referenciadas pela lógica são bloqueadas e listadas. Ajuste a **pose provisória** de uma peça/grupo e use **+ Quadro-chave** no cursor: a pose-base permanece intacta. Grave ou descarte poses provisórias antes de trocar animação/modelo ou reproduzir. A linha do tempo oferece posição/rotação/escala, quadros movíveis, copiar/colar/excluir, duração, repetição e eventos; as propriedades editam a seleção.
7. Edite atributos nas propriedades e configure **Lógica** pelo catálogo pesquisável. Conecte execução/dados e escolha parâmetros por nomes. Texto, imagem, botão e barra compõem a interface do jogo; use `{valor}` para exibir um atributo vinculado. Atributos como `Vida` não impõem regras por conta própria.
8. **Jogar** abre uma cópia isolada na aba Jogo. Sair da aba, pausar ou pressionar Esc libera a entrada; **Retomar** é explícito e **Parar** descarta o teste. **Ctrl+S** salva; **Ctrl+Z** desfaz; **Ctrl+Y/Ctrl+Shift+Z** refaz. Arrastos e pinceladas são um gesto de histórico. **Ctrl+D/Delete** duplicam/excluem objetos ou nós conforme a ferramenta ativa.

**Colisores e articulações (0.1.2):** selecionar uma peça ou grupo com colisor mostra sua caixa física, mesmo sem aparência. **Colisores** mostra as demais caixas; sólidos são verdes, áreas amarelas e desativados tracejados. **C / Editar colisor** abre alças de bordas/cantos no 2D e faces no 3D; o centro desloca somente a caixa. **Ajustar ao objeto / grupo/filhos** oferece uma prévia com margem e escolha das peças; confirmar calcula uma única caixa na pose-base. Imagens usam o retângulo do sprite. Alt suspende o encaixe; Esc cancela o gesto inteiro e, sem gesto, sai da ferramenta. No Jogo, o overlay usa a instância real e é somente leitura; mudanças no documento exigem Parar e Jogar novamente.

**P / Editar pivô** move o ponto de giro sem desmontar a peça, filhos ou colisores. Campos numéricos usam a mesma compensação; **Centralizar na peça / conjunto** e **Valores da ferramenta** dão precisão. No 3D, o centro usa um plano voltado à câmera, e as alças X/Y/Z usam eixos do mundo. W/E/R retornam às ferramentas de objetos. Em janelas estreitas, **Opções** reúne grade, encaixe, desativados e valores numéricos. Setas na Hierarquia recolhem/expandem filhos.

Na Animação, **Gravar poses alteradas** grava todas as peças provisórias no tempo em que foram editadas, em um único comando de desfazer. O cursor permanece nesse instante até gravar/descartar. Cada quadro-chave contém posição, rotação e escala vinculadas. Editar pivô exige pausa e ausência de rascunhos; a ferramenta entra explicitamente na pose-base e restaura o contexto ao sair. Pivôs diretamente animados e parentescos que envolveriam movimentos existentes são bloqueados com os clips afetados: não há retargeting aproximado. As montagens exercitadas pela QA ficam como dados editáveis em `qa/v0.1.2/spatial-project.oxy.json`.

Os controles de jogo são remapeáveis nas propriedades do projeto, com a seleção limpa. Padrão: A/D movem, Espaço pula, J ataca, E interage e W/S movem em profundidade no 3D. No player, Esc pausa e F3 mostra diagnóstico. Unidades em metros, +Y para cima, sistema 3D destro; a interface mostra graus e os documentos guardam radianos.

## Verificar e distribuir

```powershell
cargo fmt --all --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
# Testes com janela nativa: Windows e GPU disponíveis.
cargo test -p oxy_editor native_spatial_workflow --locked -- --ignored --nocapture
cargo test -p oxy_editor native_editor_workflow --locked -- --ignored --nocapture
cargo test -p oxy_player native_player_portable_workflow --locked -- --ignored --nocapture
cargo test -p oxy_render native_gpu_depth_texture_and_resize --locked -- --ignored --nocapture
cargo test -p oxy_editor native_portable_home_workflow --locked -- --ignored --nocapture
# ZIP da engine, com player e exemplos; -SkipBuild reutiliza release já compilado.
.\scripts\package-engine.ps1
& '.\dist\OXY-Engine-0.1.3-windows-x64\OXY Engine.exe'
.\scripts\test-portable.ps1 -Zip '.\dist\OXY-Engine-0.1.3-windows-x64.zip'
# O fluxo de jogos permanece separado (apenas runtime + dados do seu projeto).
.\scripts\package.ps1 -Project 'examples/validacao' -Destination 'dist/Meu-Jogo'
.\dist\Meu-Jogo\oxy_player.exe
```

Para usar o auxiliar com argumentos após `--`, passe um vetor, como no exemplo de execução. O empacotador inclui executáveis e dados locais, prepara uma pasta temporária e preserva o pacote anterior. `-DebugBuild` escolhe desenvolvimento; `-SkipBuild` usa binários já compilados do perfil escolhido. O pacote independe do diretório-fonte.

**Distribuição automática:** `.github/workflows/portable-windows.yml` gera ZIP Windows x64 e SHA-256, disponibiliza artifact por 14 dias em pushes para `main` e anexa ambos a GitHub Releases em tags `v*`. A tag deve corresponder à versão do workspace (por exemplo, `v0.1.3`); atualize `Cargo.toml`/`Cargo.lock` antes de uma nova versão. O build usa MSVC com runtime C estático. `ci.yml` continua verificando fmt/build/test/Clippy. Binários, ZIPs e DLLs permanecem em `target/` e `dist/`, ignorados pelo Git. Nenhum instalador, updater ou assinatura foi acrescentado.

`test-portable.ps1` extrai em um caminho temporário com espaços, confere arquitetura x64, ícone/metadados, DLLs importadas e assets, e abre o editor sem argumentos a partir de outro diretório, com Rust/Cargo removidos do PATH. `-ContentOnly` omite a janela para runners sem GPU. Para a aceitação em Windows realmente limpo, execute esse mesmo script e ZIP em outra máquina/Windows Sandbox sem Rust, confirme os quatro comandos da tela inicial, abra o exemplo, teste Jogar e salve/reabra uma cópia. **Windows Sandbox não está disponível neste ambiente; o teste em outra máquina e a execução do novo workflow no GitHub ainda não foram verificados.**

`examples/validacao/project.oxy.json` contém sala 2D, boneco 3D articulado e interação de carta. São cenas editáveis normais com comportamentos em componentes/grafos e placeholders locais. Para gerar outra cópia: `cargo run --locked -p oxy_core --example create_demos -- artifacts/validacao-regenerada`.

Os testes cobrem persistência segura e falhas de escrita, referências/hierarquias, seleção e transformações, isolamento de execução, histórico de documentos/pixels, animação/eventos, grafos/esperas/cancelamento, colisões, caches e interação egui. Testes nativos exercitam editor, player e GPU; evidências desta revisão ficam em `qa/v0.1.2/`. A configuração em `.github/workflows/ci.yml` executa formatação, build, testes sem janela e Clippy no Windows; **a execução no GitHub ainda não foi verificada**.

Verificado na atualização portátil em Windows/GNU: **95 testes automatizados e 3 testes nativos passaram** (tela inicial, regressão do editor e player), além de fmt, builds de desenvolvimento/release e Clippy com `-D warnings`. O executável do ZIP abriu fora do repositório e sem Rust/Cargo no PATH; o pacote passou na inspeção de arquitetura, recursos e DLLs. Evidências novas ficam em `qa/portable/`. A QA usa janelas wgpu reais e entrada isolada no egui. Na tela inicial, houve 1 atualização espontânea em 500 ms após estabilização; não é uma medição de consumo total de CPU/GPU.

## Estrutura e limites

`oxy_core` separa documentos, simulação, ações, persistência e histórico; `oxy_render` compartilha renderização, entrada e interface de jogo; `oxy_editor` e `oxy_player` são hosts independentes. A v0.1.3 mantém **schema_version 1**, compatível com os documentos da v0.1, v0.1.1 e v0.1.2.

O histórico armazena deltas de documentos e pixels. Imagens CPU são carregadas sob demanda, com orçamento de **128 MiB**; pixels alterados/em uso são preservados e podem ultrapassá-lo até serem liberados. Caches de malhas CPU/GPU usam LRU de **64 entradas/64 MiB** cada. O editor ocioso redesenha sob demanda; jogo, animação, interação e diagnóstico têm atualizações próprias. Esses mecanismos não representam medições de desempenho nem um limite total de memória. Em janelas estreitas, a Animação usa o botão **Propriedades** para abrir a edição da pose/quadro/evento, deixando espaço para o viewport e a timeline.

- O overlay 3D permite ver a caixa através da geometria, mas ainda não diferencia trechos ocultos por oclusão. Ajuste usa limites geométricos retangulares, sem silhueta transparente ou corpo composto.
- Escala múltipla é uniforme; operações que exigiriam cisalhamento são recusadas. Colisões são AABBs/áreas: girar a malha não gira o colisor.
- Modelos preservam peças rígidas e grupos; não há edição livre de malha, pesos, IK, mistura avançada, PBR ou sombras sofisticadas. Pintura usa RGBA sem camadas.
- Modelos instanciados são cópias independentes, sem herança de variantes. Excluir um recurso retira seu registro da biblioteca, preservando o arquivo no disco para desfazer; referências em uso impedem essa exclusão.
- Referências quebradas de objetos são diagnosticadas e precisam ser corrigidas antes de salvar/jogar. Escrita segura preserva o arquivo anterior se falhar; versões incompatíveis são rejeitadas.
- Lua permanece planejada: ações atuais são nativas com IDs estáveis. Não há scripts/plugins públicos, gêneros completos, instalador ou exportação web/mobile.
- Áudio WAV simples no Windows, sem mixer/áudio espacial. Reprodução audível e uso em uma segunda máquina Windows **não foram validados manualmente**.
