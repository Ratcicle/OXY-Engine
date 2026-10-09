# Histórico de versões

Resumo de cada versão. Os relatórios completos, com testes, capturas e medições, ficam em [`docs/`](docs/); as capturas de versões anteriores continuam acessíveis pelas tags do repositório.

## Não publicado

- **Aparência:** tema neutro com um único destaque azul dessaturado, aplicado ao editor e ao player. Barras e botões de ícone mais compactos (24 px), espaçamentos e fonte reduzidos, viewport com fundo e grade neutros. As cores da interface passam a vir de tokens em `oxy_render::theme`.
- **Repositório:** testes nativos e medições gravam capturas em `target/qa/` (ou `OXY_QA_DIR`), fora do Git. A pasta `qa/` guarda só as evidências da versão atual; as anteriores ficam nas tags.
- **Erros tipados:** `migration` e `persistence` devolvem `MigrationError` e `PersistenceError` (thiserror), com as mesmas mensagens de antes. Quem chama pode distinguir, por exemplo, versão incompatível, asset ausente e salvamento revertido; `?` continua convertendo para `String` onde a interface só exibe o texto.
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
