# OXY Engine

Editor e runtime desktop próprios em Rust, com Windows como primeiro alvo. A versão 0.1 reúne cenas 2D/3D, montagem por peças, pintura de pixels, keyframes, atributos, lógica visual e um player independente. O projeto de validação é um documento normal e pode ser editado.

## Requisitos e execução

Para usar um pacote pronto, abra `Abrir OXY Engine.cmd` ou `oxy_player.exe` na pasta distribuída. Mantenha `data` ao lado dos executáveis. **Não é necessário instalar Rust, compilador ou editor externo para executar o pacote.**

Windows 10/11 x64, driver gráfico atualizado e GPU compatível com um backend nativo do wgpu. Somente para desenvolver: Rust **1.92.0**, `rustfmt`, `clippy` e um compilador/linker Windows. A configuração usual usa MSVC com ferramentas C++ e Windows SDK; veja os [pré-requisitos oficiais do Rust](https://rust-lang.github.io/rustup/installation/windows-msvc.html). Também é possível usar Rust GNU com uma distribuição **completa** de MinGW-w64, incluindo bibliotecas e arquivos de inicialização.

As dependências diretas estão fixadas em `Cargo.toml`; `Cargo.lock` fixa também as transitivas. A interface usa [eframe/egui 0.33.3 com backend wgpu](https://github.com/emilk/egui/tree/0.33.3/crates/eframe), compartilhando [wgpu 27.0.1](https://docs.rs/wgpu/27.0.1/wgpu/) com o renderizador da OXY. Nenhuma engine pronta é usada no núcleo.

Na máquina preparada durante o desenvolvimento, execute em PowerShell, na raiz do repositório:

```powershell
$a = @('run','--locked','-p','oxy_editor','--','examples/validacao/project.oxy.json')
& .\scripts\cargo.ps1 @a
```

O script encontra o Cargo e a instalação GNU preparada. Em outra máquina com Rust/MSVC configurado, use diretamente:

```powershell
cargo run --locked -p oxy_editor -- examples/validacao/project.oxy.json
cargo run --locked -p oxy_player -- examples/validacao/project.oxy.json
cargo build --workspace --release --locked
```

O editor aceita o caminho do projeto como primeiro argumento. Sem argumento, abre a validação se ela estiver disponível no diretório atual; caso contrário, inicia um projeto vazio. O player aceita arquivo ou pasta. Sem argumento, procura `data/project.oxy.json` **ao lado do próprio executável**.

## Fluxo de uso

1. Use **Projeto → Novo projeto** e **Salvar** para escolher uma pasta. **+ Cena** cria cenas 2D ou 3D; **Definir inicial** escolhe a cena do player.
2. Em **Cena**, use **+ Objeto** ou importe PNG. A biblioteca permite criar/aplicar sprites e colocar modelos. Hierarquia e viewport selecionam o mesmo objeto; propriedades editam aparência, pivô, transformações, parentesco e componentes.
3. Abra **Estúdio** pelo botão direito. Em **Modelagem**, novas peças recebem a seleção como pai. **Salvar hierarquia como modelo** preserva as peças editáveis na biblioteca.
4. Em **Pintura**, crie uma textura 256×256/512×512 ou importe PNG. Pinte a imagem ou a superfície selecionada; há pincel, preenchimento, conta-gotas, paleta e exportação PNG. Texturas compartilhadas exigem escolher editar todas as referências ou criar uma cópia.
5. Em **Animação**, escolha o objeto dono, crie um clip, insira keyframes das peças e edite suas poses. A linha do tempo permite mover/copiar/excluir quadros, definir duração, repetir e inserir marcadores. **Pose-base** encerra a prévia sem gravá-la no modelo.
6. Adicione atributos nas propriedades e abra **Lógica**. Busque operações, conecte portas de execução/dados e configure os parâmetros. Os mesmos atributos alimentam texto, botões e barras da interface de jogo. Um nome como `Vida` não cria regras automaticamente.
7. **Jogar** cria uma instância isolada na aba **Jogo**. **Pausar** suspende a simulação; **Parar** descarta o teste. Saia da aba ou pressione Escape para liberar a entrada; retome explicitamente.
8. Salve, feche e reabra o projeto. O indicador de alterações inclui documentos e pixels; salvar usa arquivos temporários e recuperação da versão anterior se houver erro.

## Controles

| Contexto | Controle |
|---|---|
| Cena / Estúdio | Esquerdo seleciona; central arrasta a câmera; roda amplia/reduz; direito orbita no 3D ou abre contexto |
| Transformações | Arraste as extremidades dos eixos; escolha Mover/Girar/Escalar; Alt suspende o encaixe durante movimento |
| Edição | Ctrl+S salva; Ctrl+Z desfaz; Ctrl+Y/Ctrl+Shift+Z refaz; Ctrl+D duplica; Delete exclui a hierarquia selecionada |
| Lógica | Arraste cabeçalhos dos nós; conecte portas; pan com botão central e zoom com roda; parâmetros no painel do nó |
| Jogo padrão | A/D movem; Espaço pula; J ataca; E interage; W/S movem em profundidade no 3D |
| Player | Escape pausa; Retomar continua; F3 abre diagnóstico |

As teclas de jogo podem ser remapeadas nas propriedades do projeto, ao limpar a seleção. Os atalhos de edição ficam separados da captura de gameplay. Unidades em metros; +Y para cima, sistema 3D destro e câmera mirando -Z local. Rotações aparecem em graus na interface e são armazenadas em radianos.

## Validação e distribuição

`examples/validacao/project.oxy.json` contém três cenas: sala 2D com coleta/porta/ataque, boneco 3D articulado com textura pintável e interação de carta com custo/energia/dano. Todos os comportamentos estão em componentes, atributos, clips e grafos do documento. Os sprites/texturas/áudio são placeholders locais; nenhum asset externo do Metroidvania foi usado.

```powershell
.\scripts\cargo.ps1 fmt --all --check
.\scripts\cargo.ps1 build --workspace --locked
.\scripts\cargo.ps1 test --workspace --locked
$a = @('clippy','--workspace','--all-targets','--locked','--','-D','warnings')
& .\scripts\cargo.ps1 @a
# Opcional: gerar e testar uma nova cópia dos dados de validação.
$a = @('run','--locked','-p','oxy_core','--example','create_demos','--','artifacts/validacao-regenerada')
& .\scripts\cargo.ps1 @a
```

Os testes cobrem serialização/referências, ciclos e duplicação de hierarquias, isolamento de execução, desfazer/refazer, keyframes/marcadores, grafos/esperas/cancelamento, colisões e pixels/vínculos após salvar/reabrir. Incluem falha real de substituição por arquivo bloqueado no Windows, falha parcial de substituição e preservação dos backups durante recuperação.

Verificação desta entrega: **58 testes automatizados passaram**, além de **3 testes nativos explícitos** do editor, player e GPU. Formatação, build release e Clippy com `-D warnings` passaram. As janelas reais usam wgpu; os testes de interface injetam eventos somente no próprio egui, sem disputar o teclado/mouse do desktop. Foram exercitados gizmo, nós, entrada, pausa/retomada, isolamento, pintura/desfazer/refazer, keyframes, salvamento/reabertura e os quatro cliques da carta. Capturas e relatórios estão em `qa/`; o renderizador foi verificado na Radeon RX 6600/Vulkan, incluindo leitura de profundidade e textura da GPU.

Para repetir os testes nativos em Windows com GPU disponível:

```powershell
$a = @('test','--workspace','--locked','--','--ignored','--nocapture')
& .\scripts\cargo.ps1 @a
```

A reprodução audível em alto-falantes e a execução em uma segunda máquina Windows permanecem verificações manuais; os testes nativos não certificam todo hardware nem cada combinação possível de edição.

Para distribuir um jogo desktop:

```powershell
.\scripts\package.ps1 -Project 'examples/validacao' -Destination 'dist/OXY-Player'
.\dist\OXY-Player\oxy_player.exe
# Para incluir também o editor nativo:
.\scripts\package.ps1 -Project 'examples/validacao' -Destination 'dist/OXY-Engine' -IncludeEditor
```

O pacote padrão contém player e pasta `data`, com apenas os assets referenciados; não precisa do editor nem do diretório-fonte. `-IncludeEditor` acrescenta `oxy_editor.exe` e o atalho `Abrir OXY Engine.cmd` para abrir os dados incluídos. O script monta uma pasta temporária antes de substituir o destino e preserva um pacote anterior. `-DebugBuild` gera pacote de desenvolvimento; `-SkipBuild` usa os executáveis já compilados do perfil escolhido.

## Estrutura e limites

`oxy_core` contém documentos, persistência, histórico, pintura, animação, colisões, ações/grafos e simulação; `oxy_render` contém geometria, UVs, câmera, renderização e interface do jogo; `oxy_editor` e `oxy_player` são hosts nativos independentes. PNG e WAV importados são copiados, e modelos compostos ficam estruturados no JSON versionado.

- Colisões usam AABBs e áreas; girar a malha não gira o colisor. Não há física de malhas, roll, parry ou lock-on.
- Transformações são posição/rotação/escala com pivô. Operações globais ou de parentesco que exigiriam cisalhamento são recusadas com diagnóstico; ajuste a escala do pai ou edite em coordenadas locais.
- Animação usa peças rígidas e interpolação linear/manutenção de pose, com slerp de rotações; não há pesos, IK ou mistura avançada.
- Modelos são estruturas reutilizáveis copiadas para a cena. Salvar uma edição cria outro asset; não há herança de templates nem propagação automática para instâncias.
- Pintura trabalha em um único canal RGBA, sem camadas ou mapas de relevo. Geometria vem das primitivas; não há importação/edição de malhas arbitrárias, escultura ou extrusão.
- Lua é a linguagem planejada; nesta versão as operações são nativas, registradas por identificadores estáveis. Não há plugins públicos nem scripts personalizados.
- Excluir um objeto referenciado por nós preserva a referência quebrada para diagnóstico. Corrija ou remova o nó indicado antes de salvar/jogar; uma ação nunca é redirecionada silenciosamente para outro objeto.
- Áudio simples WAV mono/estéreo no Windows, sem mixer ou áudio espacial. Limites de trabalho por atualização protegem a simulação de grafos excessivos; dados de versões incompatíveis são rejeitados.
