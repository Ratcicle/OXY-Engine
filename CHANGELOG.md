# Histórico de versões

Resumo de cada versão. Os relatórios completos, com testes, capturas e medições, ficam em [`docs/`](docs/); as capturas de versões anteriores continuam acessíveis pelas tags do repositório.

## 0.4.2 — 2026-10-11

- **Tela inicial no visual novo:** barra superior e barra de status iguais às do editor, cartões para **Novo projeto** e **Abrir projeto**, um bloco **Exemplos** com a explicação de cada exemplo, e **Projetos recentes** num bloco com busca, há quanto tempo cada projeto foi salvo, aviso de arquivo não encontrado e as ações (Abrir, Remover da lista) no "⋯". Em janelas estreitas as colunas se empilham.
- **Novo projeto:** a primeira cena (2D ou 3D) é escolhida em dois cartões lado a lado, com ícone e marca de seleção.
- **Diálogos no mesmo padrão:** cabeçalho com fechar e a ação principal à direita do rodapé em Nova cena, Renomear cena, Excluir cena, Alterações não salvas e Preferências do editor (antes "Escala da interface"). Excluir cena explica por que não pode excluir e lista os nós pelo nome do objeto, sem identificadores; em Alterações não salvas, "Descartar alterações" fica separado de "Salvar e continuar".
- **Propriedades sem seleção e com vários objetos:** sem seleção, um bloco Projeto com nome, arquivo, cena inicial e conteúdo, e uma indicação de como começar; com vários objetos, cabeçalho com os nomes, visibilidade de todos e o bloco Transformar o conjunto em linhas alinhadas.
- **Forma em prévia e Encaixar vértices:** no mesmo bloco das operações em prévia, com Cancelar/Aplicar; o encaixe mostra os passos Origem e Destino. Clicar fora da prévia de forma continua aplicando.
- **Ajuda da modelagem:** tópicos com tabela de atalhos no lugar dos parágrafos.
- **Borda interna sem fragmentar a face:** a face interna sai como uma face só, com um quad de moldura por aresta, e as faces vizinhas não ganham vértices. Antes, cada triângulo interno da face (por exemplo, a base de um cone) virava um pedaço próprio, o que também fazia o arredondamento recusar arestas próximas. Faces com UVs que não são lineares no plano continuam divididas para preservar a pintura.
- **Arrastar valores das operações:** arrastar Largura em Arredondar (ou outro campo do painel da operação) não passa mais para o campo de baixo quando a dica do painel desaparece.

## 0.4.1 — 2026-10-10

- **Estúdio no visual novo:** Modelagem, Pintura e Animação usam as duas linhas de cabeçalho, o chip de status no viewport e blocos em Propriedades (Malha, Textura, operação em prévia com Cancelar/Aplicar). A Animação ganhou a linha do tempo no painel inferior, com trilhas por peça, régua adaptável, barra em duas linhas em janelas estreitas e as ações da animação no próprio seletor.
- **Lógica redesenhada:** barra com o objeto e a contagem do grafo, catálogo **Adicionar nó** por categorias (ou menu, em janelas estreitas), nós com setas de execução e resumo dos parâmetros, legenda de portas, resultado de **Validar** sobre o canvas e sugestões de início para objetos sem lógica. O nó selecionado é editado em Propriedades, com Parâmetros e Conexões. **Ações de entrada** virou uma tabela com "Usada por" e exclusão confirmada na linha; o **Guia** tem navegação lateral com receitas e nós por categoria.
- **Mensagens sem identificadores:** validação, conexões recusadas e referências de ações citam nós e objetos pelo nome, e todo nó registrado tem descrição.
- **Hierarquia:** na Lógica, objetos com nós mostram a quantidade de nós.
- **Propriedades:** componentes em linhas alinhadas, com o rótulo à esquerda e o campo à direita. No Colisor 3D, uma única escolha de **Forma** (Caixa, Cápsula, Esfera, Convexo da peça, Malha estática da peça) substitui os botões de gerar, **Tipo** Sólido | Área substitui a caixa de seleção e os grupos de colisão ficam numa subseção recolhida; Personagem 3D, Corpo de movimento, Controle da câmera, Plataforma e os componentes 2D seguem o mesmo padrão. O painel não cresce mais sozinho, e um componente recém-adicionado abre o próprio bloco.
- **Fonte:** IBM Plex Sans e IBM Plex Mono embutidas no editor e no player, sob a licença OFL 1.1 (em `licenses/`).

## 0.4.0 — 2026-10-09

- **Nova estrutura da tela:** barra superior única (projeto, desfazer/refazer e salvamento; modos Cena · Estúdio · Lógica no centro; Jogar/Pausar/Parar à direita), painel inferior com abas Biblioteca, Console e Avisos, e barra de status. A aba Jogo deixa de existir: o teste aparece na área central.
- **Hierarquia:** seletor de cena e busca no topo, ícone por tipo de objeto e chevrons desenhados.
- **Propriedades:** cabeçalho do objeto e blocos recolhíveis com campos alinhados; interruptor de ativo no cabeçalho e Remover componente no menu ⋯.
- **Estúdio isolado:** o objeto selecionado e seus filhos aparecem sozinhos, com câmera própria enquadrada nele. A Hierarquia mostra só a peça em edição e **Voltar à cena**.
- **Biblioteca:** cartões compactos; Localizar na biblioteca filtra pelo nome do recurso.

## 0.3.4 — 2026-10-09

- **Aparência:** tema neutro com um único destaque azul dessaturado, aplicado ao editor e ao player. Barras e botões de ícone mais compactos (24 px), espaçamentos e fonte reduzidos, viewport com fundo e grade neutros. As cores da interface passam a vir de tokens em `oxy_render::theme`.
- **Repositório:** testes nativos e medições gravam capturas em `target/qa/` (ou `OXY_QA_DIR`), fora do Git. A pasta `qa/` guarda só as evidências da versão atual; as anteriores ficam nas tags.
- **Erros tipados:** `migration` e `persistence` devolvem `MigrationError` e `PersistenceError` (thiserror), com as mesmas mensagens de antes. Quem chama pode distinguir, por exemplo, versão incompatível, asset ausente e salvamento revertido; `?` continua convertendo para `String` onde a interface só exibe o texto.
- **Código do editor:** a struct `Editor` passou de 59 para 41 campos. Opções de visualização, sessão de jogo, hierarquia, biblioteca, transformação, console e tempos de quadro viraram sub-estados próprios, sem mudança de comportamento.
- **Documentação:** README curto; fluxo de trabalho, atalhos e domínio suportado movidos para o [guia do editor](docs/guia-do-editor.md).

## 0.3.3.1 — 2026-09-13

Corpo de movimento alinhável por deslocamento local X/Y/Z, com campos e gizmo, sem mover a origem nem a aparência. No viewport 3D, RMB + **E/C** sobe/desce no eixo global Y. [Relatório](docs/v0.3.3.1.md).

## 0.3.3 — 2026-09-12

Navegação livre no viewport 3D: **RMB + mouse + WASD**, Shift rápido, Ctrl preciso e roda para ajustar a velocidade. Soltar RMB devolve W/E/R às ferramentas. A vista não altera o projeto, o histórico nem a câmera de jogo. [Relatório](docs/v0.3.3.md).

## 0.3.2 — 2026-09-12

Corpo de movimento configurável (cápsula, caixa, esfera e convexo), guias editáveis de câmera e prévia nativa sem iniciar o jogo. Jogador e Câmera continuam separados; a cápsula da 0.3.1 é migrada para o novo corpo. [Relatório](docs/v0.3.2.md).

## 0.3.1 — 2026-09-11

Inspetor refinado e autoria por objetos explícitos: componentes compatíveis em **+ Adicionar componente**, seções recolhíveis e câmera independente com controle vinculado ao personagem. [Relatório](docs/v0.3.1.md).

## 0.3.0 — 2026-09-11

Movimento 3D: consultas de forma com Rapier/Parry, controlador em primeira pessoa com entrada relativa, motor persistente com superfícies, postura e plataformas, perfis de parkour (pulos encadeados, deslize, sensores varridos), terceira pessoa protegida, nós de movimento tipados e o Laboratório 3D editável. [Relatório](docs/v0.3.0.md).

## 0.2.1 — 2026-09-09

Modelagem direta: gestos que aplicam um único comando de histórico, ajuste da última operação, borda interna com preservação de UV, extrusão pela normal e overlays de componentes em buffers GPU reutilizáveis. [Relatório](docs/v0.2.1.md).

## 0.2.0 — 2026-09-09

Malhas poligonais editáveis com criação paramétrica, extrusão, criação de face/aresta, alinhamento de vértices, corte em loop, bisturi e arredondamento. Pintura em malhas modeladas, ações de entrada, guia offline com receitas, migração segura de schema, avisos, escala de interface e gerenciamento de cenas. [Relatório](docs/v0.2.0.md).

## 0.1.3 — 2026-09-08

Otimizações com harness determinístico: índices por fase, transformações e grafos reutilizados, filtros de colisão antecipados. Distribuição como aplicativo portátil para Windows. [Medições](benchmarks/README.md).

## 0.1.2 — 2026-09-07

Edição de colisores e pivôs preservando animações existentes.

## 0.1.1 — 2026-09-07

Refinamentos de UX e desempenho.

## 0.1.0 — 2026-09-07

Primeira versão: editor e player em Rust com cenas 2D/3D, pintura PNG, animação rígida e lógica visual.
