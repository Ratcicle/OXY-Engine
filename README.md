# OXY Engine 0.3.1

Editor e runtime desktop próprios em Rust: cenas 2D/3D, modelagem por peças e polígonos, pintura PNG, animação rígida, atributos e lógica visual. Editor e player compartilham egui/eframe 0.33.3 e wgpu 27.0.1, sem outra engine como núcleo. Dependências fixadas e `Cargo.lock` mantido.

Esta atualização limpa o Inspetor: só componentes presentes, adição contextual e ações estruturais na Hierarquia. Os presets e o laboratório usam Jogador e Câmera principal separados, ligados pelo campo Alvo. Também corrige a velocidade ao aterrissar em plataformas móveis. O [relatório da 0.3.1](docs/v0.3.1.md) registra testes e limites; os sistemas da [0.3.0](docs/v0.3.0.md), o histórico e o fluxo portátil permanecem.

## Abrir e desenvolver

Windows 10/11 x64, driver gráfico atualizado e GPU compatível com DirectX 12 ou Vulkan. Extraia o ZIP inteiro em uma pasta gravável e abra **`OXY Engine.exe`** diretamente. Mantenha `data` e as DLLs junto dos executáveis. Rust/Cargo são necessários somente para compilar; **`oxy_player.exe`** é o runtime separado dos jogos.

Sem argumentos, o editor mostra **Novo projeto**, **Abrir projeto**, **Projetos recentes** e **Projeto de exemplo**. O exemplo abre uma cópia segura das três cenas de validação; Salvar escolhe outra pasta e copia os assets. Recentes e preferências ficam em `%LOCALAPPDATA%/OXY Engine`, fora do projeto. **Projeto → Tela inicial** confirma antes de descartar alterações.

**Laboratório 3D · movimento** abre uma pista editável como cópia independente. WASD move, mouse olha, Espaço pula, Shift corre e C agacha/desliza; V alterna FP/TP, Q troca ombro e R retorna ao checkpoint. 1/2/3 aplicam perfis editáveis; T/Y alternam pulo automático/manual e I aplica impulso. Em cena 3D, **+ Objeto** oferece personagens FP/TP e plataforma; nada é criado obrigatoriamente. **Ajuda → Guia de lógica visual** inclui sete receitas novas (doze no total), com os mesmos componentes/nós da pista.

Para desenvolver, instale Rust **1.92.0**, rustfmt, Clippy e ferramentas C++/Windows SDK para MSVC, ou MinGW-w64 completo. Na raiz do repositório, em PowerShell:

```powershell
cargo run --locked -p oxy_editor
cargo run --locked -p oxy_editor -- examples/validacao/project.oxy.json
cargo run --locked -p oxy_player -- examples/validacao/project.oxy.json
cargo build --workspace --release --locked
```

Nesta máquina, `scripts/cargo.ps1` configura o compilador GNU preparado. Passe argumentos após `--` como vetor:

```powershell
./scripts/cargo.ps1 @('run','--locked','-p','oxy_editor','--','examples/validacao/project.oxy.json')
```

## Criar → modelar → pintar → animar → testar

1. **Novo projeto** pede nome e primeira cena 2D/3D, sem personagens ou mecânicas automáticas. Na faixa **Cena**, o seletor alterna cenas; **+** cria; **⋯** renomeia, duplica, exclui e escolhe a inicial. Exclusões em uso são bloqueadas com as referências.
2. **+ Objeto** cria grupos, formas, câmeras e interface. PNG/WAV são importados como cópias. Ctrl+clique seleciona vários objetos; Shift+clique seleciona intervalo na Hierarquia. Arraste para definir parentesco, ou para **Raiz da cena**, preservando a transformação global. F2 renomeia sem mudar o ID. Duplo clique enquadra; setas recolhem filhos.
3. No **Estúdio → Modelagem**, adicione cubo, esfera, cilindro, plano, pirâmide, cone ou tubo. O painel de criação ajusta dimensões/segmentos com prévia; Enter ou o primeiro clique externo confirma, Esc cancela. O clique externo é consumido. Parâmetros continuam editáveis até **Converter em malha** ou a primeira operação de geometria.
4. Escolha Objeto/Face/Aresta/Vértice. Clique seleciona; Ctrl+clique alterna; caixa comum substitui os visíveis e **Ctrl+caixa soma os componentes tocados, inclusive ocultos**. Armar uma operação não converte a primitiva. Arraste uma alça e solte para aplicar um comando; números aplicam ao soltar, Enter ou sair do campo. **Última operação**, em Propriedades, ajusta esse mesmo comando a partir da origem. Esc ou Ctrl+Z durante o gesto restaura geometria, UVs, seleção e cópias temporárias de textura.
5. Em **Pintura**, crie 256×256/512×512 ou importe PNG. Pincel, preenchimento, conta-gotas e paleta alteram pixels na imagem ou na superfície. **Selecionar faces** relaciona a malha às ilhas UV reais. Cortes preservam a pintura por interpolação; novas superfícies recebem espaço independente no atlas. Sem espaço, escolha a cópia ampliada ou cancele. Textura compartilhada exige escolher original/cópia. **Remover textura** remove só o vínculo; **Exportar PNG** salva pixels. Sombreamento **Plano/Suave** altera a iluminação, não arredonda a silhueta.
6. Na **Hierarquia**, botão direito → **Salvar hierarquia como modelo** preserva peças, malhas, pivôs e vínculos. Instanciar produz uma cópia independente. Para incluir personagem e câmera no mesmo modelo, agrupe ambos explicitamente antes de salvar; salvar só Jogador inclui seus filhos, não a câmera independente. Em **Animação**, escolha o modelo/clip e a peça; ajuste a pose provisória e grave **+ Quadro-chave** ou **Gravar poses alteradas**. Cada quadro contém posição/rotação/escala vinculadas. Gravar/descartar precede mudança de cursor/modelo e reprodução; a pose-base permanece separada. Duração, repetição, eventos e copiar/mover/excluir quadros ficam na timeline.
7. Na **Lógica**, **Ações de entrada** centraliza nomes/teclas usados por controladores e nós, com Pressionar/Manter pressionado/Soltar. Projetos novos recebem vínculos de movimento quando um controlador é adicionado. **Ajuda → Guia de lógica visual**, também acessível por F1, funciona offline e abre cópias editáveis de doze receitas: as cinco bases anteriores e sete de movimento/câmera 3D. Atributos como `Vida` não criam regras ocultas.
8. **Jogar** usa uma cópia isolada na aba Jogo. Sair da aba, pausar ou Esc libera entrada; **Retomar** é explícito e **Parar** descarta o teste. Texto, imagem, botão e barra funcionam no player; `{valor}` exibe um atributo vinculado. Ctrl+S salva; Ctrl+Z desfaz; Ctrl+Y/Ctrl+Shift+Z refaz. Cada arrasto/pincelada é um gesto de histórico.

**Propriedades** mostra nome, visibilidade, camada, transformação, aparência, atributos e componentes presentes. **+ Adicionar componente** oferece apenas opções compatíveis e ausentes. Componentes extensos começam recolhidos e lembram a expansão por objeto durante a sessão. Remova componentes dentro das suas seções; o colisor necessário ao controlador não pode ser removido antes dele. Parentesco é editado por arrasto na Hierarquia; renomear, duplicar, agrupar, excluir e salvar modelo ficam no seu menu de contexto. Lógica e Animação continuam nas suas áreas.

**+ Objeto → Câmera** cria uma câmera independente. Nela, use **+ Adicionar componente → Controle da câmera → Básico → Alvo** para escolher o personagem. Os presets FP/TP já criam e vinculam ambos. Selecionar Jogador mostra seu personagem/colisor; selecionar Câmera principal mostra câmera/controle. Componentes 3D legados são preservados na leitura e execução, mas ficam fora do fluxo de autoria. O controlador e as caixas 2D continuam disponíveis em cenas 2D.

O botão **Interface: 100%** separa escala pendente/aplicada: ajuste 80–160%, use Aplicar, Cancelar ou Restaurar 100%. **Mostrar nomes das ferramentas** acompanha os ícones para iniciantes. Avisos não abrem o console nem mudam o viewport; **Console/Ver detalhes** abre o registro manualmente. Falhas de salvamento permanecem visíveis.

O Estúdio usa duas linhas de cabeçalho, incluindo suas abas. **Visualização** reúne enquadramento, grade, encaixe e colisores; ocultar a grade não desativa o encaixe. Em janelas pequenas, **Painéis** alterna Hierarquia, Propriedades e Biblioteca, **Malha** reúne operações e o menu da ferramenta guarda Mover/Girar/Escalar/Colisor/Pivô. Na pintura, **Textura** e o menu do pincel mantêm os controles completos. A timeline seleciona peças mesmo com a Hierarquia recolhida.

**Criar borda interna (Shift+U)** reduz a face em torno do centro: 50% reduz cada dimensão pela metade, mantendo a moldura no plano original. **Extrudir (Shift+E)** combina esse tamanho com distância positiva/negativa na normal da face, X/Y/Z ou vetor avançado. Com tamanho 100%, mantém a extrusão simples. Outra edição, Undo/Redo ou troca de peça encerra o ajuste da última operação; navegar pela câmera não o encerra.

## Controles e ferramentas

| Contexto | Atalhos |
|---|---|
| Modelagem | **1/2/3/4** Objeto/Face/Aresta/Vértice; **W/E/R** Mover/Girar/Escalar |
| Operações | **Shift+E** Extrudir; **Shift+U** Criar borda interna; **Shift+F** Criar face/aresta; **Shift+N** Inverter orientação |
| Cortes/encaixe | **Shift+K** Bisturi; **Shift+R** Corte em loop; **Shift+B** Arredondar; **Shift+V** Encaixar vértices |
| Seleção | **Shift+I** Inverter; **Shift+X** Selecionar através; **Ctrl+A** Selecionar no contexto ativo |
| Objeto | **C/P** Editar colisor/pivô; **F2** Renomear; **Ctrl+D/Delete** Duplicar/excluir |
| Operação em curso | Soltar aplica; **Esc** cancela; Bisturi termina com duplo clique ou Enter. Criar, excluir, inverter e triangular aplicam imediatamente |

Campos de texto, modais, guia, grafo e jogo têm foco próprio; combinações Shift não acionam também W/E/R. Botão central move a câmera, roda amplia/reduz, arraste direito orbita no 3D. Alças X/Y/Z restringem escala por eixo; centro escala uniformemente. Alt suspende encaixe em movimento/colisor/pivô; ao escalar, força o eixo já escolhido e não desliga encaixe. Unidades em metros, +Y para cima, sistema 3D destro; graus na interface, radianos nos documentos.

Selecionar uma peça **ou grupo vazio** com colisor mostra a caixa real da física. **Visualização → Colisores** mostra as demais; sólidos verdes, áreas amarelas, desativados tracejados. **Editar colisor** oferece bordas/cantos 2D e faces 3D; mover o centro altera só a caixa. **Ajustar ao objeto/grupo/filhos** calcula uma caixa inicial com prévia, margem e seleção das peças. Geometria editada não redimensiona o colisor automaticamente. No Jogo, Visualização junto dos controles de reprodução é somente leitura; mudanças da cena exigem Parar/Jogar novamente.

**Editar pivô** compensa a transformação para manter peça, filhos e colisores no lugar; campos numéricos e Centralizar usam a mesma operação. Mudanças de pivô/parentesco que afetariam animações existentes são recusadas com os clips envolvidos. Não há retargeting aproximado. No player, Esc pausa e F3 mostra diagnóstico.

## Verificar e distribuir

```powershell
cargo fmt --all --check
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
# Testes nativos: Windows e GPU; demais comandos no relatório de entrega.
./scripts/test-native.ps1
# Medição da interação na janela nativa, separada da suíte de correção.
./scripts/benchmark-v021.ps1 -Label minha-medicao
# Engine portátil: editor + player + exemplos. -SkipBuild reutiliza release já compilado.
./scripts/package-engine.ps1
& './dist/OXY-Engine-0.3.1-windows-x64/OXY Engine.exe'
./scripts/test-portable.ps1 -Zip './dist/OXY-Engine-0.3.1-windows-x64.zip'
# Exportar seu jogo: só runtime e dados; não depende do editor ou do diretório-fonte.
./scripts/package.ps1 -Project 'examples/validacao' -Destination 'dist/Meu-Jogo'
./dist/Meu-Jogo/oxy_player.exe
# Medições release/locked; rótulos novos preservam resultados anteriores.
./scripts/benchmark.ps1 -Label meu-teste-v020
./scripts/benchmark.ps1 -Label minhas-malhas-v020 -Example performance_mesh -Sizes '22,70,158'
./scripts/benchmark.ps1 -Label meu-movimento-v030 -Example performance_movement -Sizes '1,10,50'
./scripts/cargo.ps1 @('run','-p','oxy_core','--example','movement_replay','--release','--locked')
```

Execute cada teste nativo em um processo separado: o Winit permite uma janela de teste por processo. O script acima organiza essa sequência; medições de desempenho ficam separadas para evitar carga concorrente.

O player aceita arquivo/pasta; sem argumento procura `data/project.oxy.json` junto do executável. Empacotadores incluem assets referenciados e modelos/malhas no JSON, usam uma pasta temporária e preservam o pacote anterior. O guia e suas receitas são embarcados no executável. `-DebugBuild` no empacotador de jogos escolhe desenvolvimento; `-SkipBuild` usa os binários do perfil selecionado.

`portable-windows.yml` gera ZIP x64 e SHA-256: artifact por 14 dias em pushes para `main`, e anexos de Release **somente em tags `v*`** correspondentes à versão do workspace, como `v0.3.1`. MSVC usa runtime C estático. `ci.yml` verifica fmt/build/test/Clippy e invariantes; `performance.yml` permite benchmarks manuais sem reprovar por variação de milissegundos. `target/`, `dist/` e ferramentas temporárias permanecem ignorados. Não há instalador, updater ou assinatura digital.

O teste portátil confere o checksum adjacente quando disponível, versão/ícone, PE x64, DLLs e assets; extrai em caminho com espaços e abre o editor de outro diretório, sem Rust/Cargo no PATH. `-ContentOnly` omite a janela em runners sem GPU. Isso **não substitui outra máquina limpa**: Windows Sandbox não está disponível neste ambiente. Execução do CI remoto e reprodução audível de WAV também não estão verificadas. O [relatório de entrega](docs/v0.3.1.md) distingue testes automáticos, janelas/capturas reais, medições CPU e limitações do ambiente; [benchmarks da base](benchmarks/README.md) preservam a referência da 0.1.3. Não há promessa de FPS, medição inventada de GPU/VRAM ou validação universal de hardware.

## Compatibilidade e domínio suportado

O formato atual é **schema_version 3**. Documentos schema 1 e 2 são migrados em memória sem mudar IDs nem sobrescrever ao abrir. Ao salvar convertido, há backup identificado dos bytes originais e escrita segura. Versões futuras/inválidas são recusadas; leitores antigos não devem abrir o novo formato. As três demonstrações continuam em `examples/validacao`; doze receitas em `examples/guia` e a pista em `examples/laboratorio-3d` são projetos comuns.

- Malhas: triângulos, quads e polígonos simples planos, inclusive côncavos; bordas abertas e elementos soltos. Faces com buracos, cruzamentos, degenerações ou mais de duas faces por aresta são recusadas. Faces não planas exigem triangulação explícita ou edição válida.
- Extrusão: região conectada, por face, faixa de bordas/arestas soltas e cadeias de vértices. Criar face exige contorno/ordem inequívoca. Encaixe move a seleção efetiva ou escala no eixo com solução; operações que exigiriam cisalhamento são recusadas.
- Borda interna e extrusão com tamanho menor que 100%: faces convexas planas, cada face independente ou região coplanar convexa sem buracos. Recusa concavidades, cruzamentos, regiões não planas e colisões da nova superfície com geometria da própria peça. O limite de trabalho é explícito; não há booleanas ou solução de autointerseções. O tamanho é proporção linear, não uma distância uniforme de cada borda.
- Loop: faixa de quads, até 64 cortes; para em polos/triângulos/polígonos. Bisturi: percurso contínuo visível entre bordas de faces adjacentes, uma visita por face; não atravessa volume, vazio ou outras malhas.
- Arredondamento: arestas convexas expostas com duas faces, cadeias e junções de cubos/prismas; cantos convexos de três arestas, inclusive múltiplos não conflitantes. Largura abaixo de 45% da menor aresta incidente, 1–16 segmentos. Regiões abertas/côncavas/encobertas, valências complexas e sobreposições são recusadas.
- Pintura: um atlas RGBA por peça, UV por canto e espaço conservador para faces novas; sem camadas/editor UV avançado. A cópia ampliada preserva pixels antigos 1:1. Instâncias são independentes, sem variantes/herança.
- Colisões legadas continuam caixas alinhadas aos eixos. O novo personagem 3D é opcional: cápsula em pé com escala global uniforme positiva, consultas Rapier/Parry, caixas orientadas/esferas/convexos e triângulos estáticos preparados explicitamente. Não há corpos rígidos dinâmicos ou transporte por rotação. Personagens novos e controladores legados não ganham colisão mútua implícita; o novo solver consulta obstáculos legados, mas o antigo continua consultando caixas. Ajustar um colisor nunca segue a animação automaticamente. O overlay atravessa geometria sem distinguir todos os trechos ocultos.
- Animação é por peças rígidas; sem deformação, pesos, IK, canais independentes ou mistura avançada. Lua/plugins, física avançada, PBR, gêneros completos e exportação web/mobile ficam fora desta versão.

As otimizações da 0.1.3 permanecem: índices por fase, matrizes reutilizadas, grafos preparados e vida útil de ativações. Histórico guarda deltas/pixels e apenas malhas afetadas em armazenamento imutável; não copia todas as imagens. Texturas CPU sob demanda têm orçamento de 128 MiB, preservando buffers alterados/em uso. Caches de malhas CPU/GPU usam 64 entradas/64 MiB cada. Repouso redesenha sob demanda. Esses orçamentos não equivalem à RAM/VRAM total; custos e limites medidos de malhas 968/9.800/49.928 triângulos estão no relatório.

A seleção usa projeções cacheadas; o desenho de faces, arestas e vértices reutiliza streams GPU com profundidade. Os resultados antes/depois estão no relatório e nos arquivos JSON/CSV; CPU de interação não equivale a FPS ou tempo de GPU. Selecionar caixas visíveis em superfícies densas e preparar deltas de malhas grandes continuam sendo trabalhos distintos do desenho.
