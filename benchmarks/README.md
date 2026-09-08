# OXY Engine 0.1.3 — medições e decisões

Medições locais de 08/09/2026. Código de produção medido: `e7257dea5ee53d1dde9950cb3a82eaab0356e2bb`; os commits seguintes acrescentam testes, harness e evidências, sem alterar a simulação. Base encontrada, sem alterações locais: `3e2da709d4077458ac791c5c2528892fb64f7649`. Referência executável: `90cbcb04358addf2363a867743eb16e528254154`, que acrescenta instrumentação desligável e o harness, antes das otimizações. Não houve checkout destrutivo, tag, Release ou push.

## Ambiente e método

Windows 11 Pro 26100, Ryzen 5 5600 (12 processadores lógicos), 17.076.060.160 bytes de RAM instalada. Rust 1.92.0, `x86_64-pc-windows-gnu`, release `opt-level=2`, Cargo.lock, sem override de RUSTFLAGS. GPU dos testes nativos: Radeon RX 6600, driver Windows 32.0.21045.1000; a prova gráfica identificou Vulkan / AMD 26.7.1. Janela de medição 1280×720 pontos (capturas 1280×720), monitor 1920×1080. Não foi feita medição de tempo GPU/VRAM.

- Microbenchmarks: 3 aquecimentos, até 101 amostras, limite suave de 3 segundos por operação. Um trabalho já iniciado termina; o script limita a suíte a 15 minutos. p95/p99 só são publicados com pelo menos 100 amostras. Os cenários lentos da referência atingiram o limite; seus percentis indisponíveis permanecem nulos, sem extrapolação. Parâmetros e opções foram mantidos nos dois lados.
- Criação, desserialização/validação, consultas, matrizes, subárvores, preparação CPU do renderer, passo fixo e acumulação de quatro passos têm medições separadas. Cada chamada ao runtime verifica o avanço efetivo do relógio. Os primeiros três passos são aquecimento e não entram nas amostras; o estado evolui identicamente, até o limite de duração.
- A saída e os dados são observáveis via `black_box`, checksums, atributos e replays. IDs das fixtures têm formato UUID e são determinísticos (semente 0); cópias criadas pelas ações usam o mecanismo normal de IDs da engine e são removidas ao final.
- Contadores detalhados estão ativos nos microbenchmarks **dos dois lados**: acrescentam acessos thread-local nos caminhos medidos, inclusive em cada comparação linear antiga. Não se deve transferir esses tempos diretamente para o jogo distribuído. Os hosts nativos foram comparados separadamente **sem essa instrumentação**.
- Nos hosts reais, cronometra-se `App::update`, incluindo construção da UI, renderer e chamadas ao driver. Não inclui tesselação posterior do eframe, término da GPU ou VSync. O player recebe exatamente um passo manual por atualização e fica pausado durante a chamada do host para impedir passos acumulados adicionais. 20 aquecimentos + 101 amostras, limite de 120 segundos. Isso testa o host completo, mas **não mede a latência total do quadro nem prova FPS**.

Resultados brutos e metadados estão em `results/`. `baseline.json`, `indexed.json`, `shared.json`, `spatial.json` e `final.json` preservam os marcos. O binário da referência foi conservado localmente em `.tools/perf-baseline/`, com SHA-256 em `baseline-environment.json`. A comparação adicional usa `extra-complete-reference.json` e `extra-complete-final.json`: o mesmo harness foi copiado para um worktree separado em 90cbcb0, sem copiar as otimizações. Os arquivos `extra-reference`/`extra-final` registram a rodada anterior desse protocolo, sem as fases separadas do histórico.

## Carga e passo fixo

Todos os números N incluem controladores e áreas. `move`: 10 controladores, N/2 colisores, restante decoração; `move_decor`: 10 controladores, apenas 40 colisores. Os cenários de áreas têm 10 controladores e 10 áreas dentro de N/2 colisores; esparso em grade com espaçamento 4, denso em quatro posições próximas. Os outros objetos continuam visuais. `hierarchy`: cadeias de 16 peças, pivôs, escala não uniforme e espelhamento. `graph_small`/`graph_large`: 8/64 nós, 7/63 conexões, evento de entrada, dano e espera; 8/64 nós de execução efetivamente processados por passo em regime estável. Não há comportamento especial no runtime para essas cenas.

Medianas de **um** passo em ms. C/S/A/D = controladores / sólidos / áreas / decoração. p95/p99 finais em ms; percentis e contagens da referência estão no [CSV completo](results/baseline-to-final.csv).

| Cenário | N | C/S/A/D | Antes | Depois | p95 depois | p99 depois |
|---|---:|---|---:|---:|---:|---:|
| static | 100 | 0/0/0/100 | 0.001 | 0.001 | 0.001 | 0.001 |
| move | 100 | 10/50/0/50 | 1.141 | 0.087 | 0.096 | 0.110 |
| move_decor | 100 | 10/40/0/60 | 1.027 | 0.077 | 0.090 | 0.104 |
| area_sparse | 100 | 10/40/10/50 | 1.476 | 0.120 | 0.181 | 0.196 |
| area_dense | 100 | 10/40/10/50 | 1.573 | 0.251 | 0.423 | 0.569 |
| hierarchy | 100 | 0/0/0/100 | 0.001 | 0.001 | 0.001 | 0.001 |
| graph_small | 100 | 0/0/0/100 | 0.039 | 0.009 | 0.013 | 0.020 |
| graph_large | 100 | 0/0/0/100 | 1.979 | 0.061 | 0.081 | 0.111 |
| static | 200 | 0/0/0/200 | 0.001 | 0.001 | 0.001 | 0.001 |
| move | 200 | 10/100/0/100 | 4.036 | 0.091 | 0.096 | 0.096 |
| move_decor | 200 | 10/40/0/160 | 3.066 | 0.100 | 0.104 | 0.105 |
| area_sparse | 200 | 10/90/10/100 | 5.486 | 0.199 | 0.204 | 0.208 |
| area_dense | 200 | 10/90/10/100 | 5.664 | 0.570 | 0.737 | 1.033 |
| hierarchy | 200 | 0/0/0/200 | 0.001 | 0.001 | 0.001 | 0.001 |
| graph_small | 200 | 0/0/0/200 | 0.037 | 0.009 | 0.011 | 0.015 |
| graph_large | 200 | 0/0/0/200 | 1.898 | 0.062 | 0.106 | 0.117 |
| static | 400 | 0/0/0/400 | 0.002 | 0.001 | 0.001 | 0.001 |
| move | 400 | 10/200/0/200 | 14.613 | 0.163 | 0.194 | 0.247 |
| move_decor | 400 | 10/40/0/360 | 10.739 | 0.154 | 0.160 | 0.170 |
| area_sparse | 400 | 10/190/10/200 | 20.181 | 0.344 | 0.356 | 0.381 |
| area_dense | 400 | 10/190/10/200 | 22.368 | 1.035 | 1.064 | 1.100 |
| hierarchy | 400 | 0/0/0/400 | 0.002 | 0.001 | 0.001 | 0.001 |
| graph_small | 400 | 0/0/0/400 | 0.038 | 0.009 | 0.012 | 0.014 |
| graph_large | 400 | 0/0/0/400 | 2.004 | 0.062 | 0.068 | 0.075 |
| static | 800 | 0/0/0/800 | 0.004 | 0.002 | 0.002 | 0.003 |
| move | 800 | 10/400/0/400 | 54.355 | 0.304 | 0.314 | 0.325 |
| move_decor | 800 | 10/40/0/760 | 40.014 | 0.253 | 0.266 | 0.269 |
| area_sparse | 800 | 10/390/10/400 | 77.030 | 0.616 | 0.634 | 0.645 |
| area_dense | 800 | 10/390/10/400 | 78.331 | 2.128 | 2.368 | 2.951 |
| hierarchy | 800 | 0/0/0/800 | 0.004 | 0.002 | 0.002 | 0.002 |
| graph_small | 800 | 0/0/0/800 | 0.040 | 0.011 | 0.013 | 0.016 |
| graph_large | 800 | 0/0/0/800 | 1.898 | 0.067 | 0.071 | 0.081 |
| static | 1600 | 0/0/0/1600 | 0.007 | 0.005 | 0.006 | 0.007 |
| move | 1600 | 10/800/0/800 | 228.876 | 0.564 | 0.580 | 0.585 |
| move_decor | 1600 | 10/40/0/1560 | 160.476 | 0.453 | 0.476 | 0.487 |
| area_sparse | 1600 | 10/790/10/800 | 307.870 | 1.172 | 1.236 | 1.367 |
| area_dense | 1600 | 10/790/10/800 | 313.909 | 4.393 | 5.264 | 6.037 |
| hierarchy | 1600 | 0/0/0/1600 | 0.007 | 0.004 | 0.005 | 0.005 |
| graph_small | 1600 | 0/0/0/1600 | 0.046 | 0.016 | 0.019 | 0.022 |
| graph_large | 1600 | 0/0/0/1600 | 1.878 | 0.069 | 0.081 | 0.114 |

Uma cena estática ter um passo vazio barato não significa que seja barata de desenhar. A tabela seguinte mede trabalho de cena/renderização, separadamente, em 1.600 entidades. Preparação CPU usa a mesma função chamada pelo renderer, mas não engloba upload, draw ou GPU.

| Operação | Estática antes/depois (ms) | Hierarquia antes/depois (ms) |
|---|---:|---:|
| create | 0.594 / 0.613 | 0.697 / 0.728 |
| load_validate | 15.220 / 2.963 | 71.513 / 3.233 |
| queries | 7.457 / 0.151 | 7.426 / 0.250 |
| transforms | 7.520 / 0.387 | 66.593 / 0.536 |
| subtrees | 0.458 / 0.125 | 16.474 / 0.362 |
| render_cpu | 7.856 / 0.411 | 118.738 / 0.891 |
| gesture_undo_redo | 43.879 / 37.345 | 43.428 / 37.779 |

## Ferramentas e hosts nativos

Harness adicional: todas as peças têm colisor e trilha de dois quadros; a prévia inclui cópia da cena, amostragem real em t=0,37 e preparação CPU. Parentesco mede ida/volta na pose-base com escala uniforme, pois a API recusa corretamente cisalhamento. Picking e overlays usam 1280×720. Medianas antes/depois em ms; demais percentis e amostras no [CSV adicional](results/extra-complete-reference-to-extra-complete-final.csv).

| N | Seleção por raio | Overlays | Cópia + prévia + preparação | Mudar pai e restaurar |
|---:|---:|---:|---:|---:|
| 100 | 0.550 / 0.063 | 0.464 / 0.157 | 0.609 / 0.128 | 0.020 / 0.021 |
| 200 | 2.016 / 0.115 | 1.461 / 0.241 | 2.260 / 0.190 | 0.022 / 0.023 |
| 400 | 7.887 / 0.230 | 5.112 / 0.430 | 8.956 / 0.383 | 0.026 / 0.026 |
| 800 | 30.595 / 0.467 | 19.309 / 0.752 | 33.784 / 0.779 | 0.035 / 0.035 |
| 1600 | 122.339 / 0.957 | 75.519 / 1.510 | 133.275 / 1.578 | 0.056 / 0.057 |

Histórico, medido em fases isoladas (3 aquecimentos + 101 amostras; teto de 12 s para o conjunto). Esta carga tem colisor em todas as peças; não é a mesma do `gesture_undo_redo` acima. Em 1.600 peças:

| Fase | Antes (ms) | Depois (ms) |
|---|---:|---:|
| begin | 0.235 | 0.222 |
| commit | 32.708 | 33.895 |
| undo | 8.517 | 8.729 |
| redo | 7.940 | 8.044 |

Ambos retêm estimados 69.600 bytes de deltas após os gestos. Isso não inclui estruturas temporárias: o diff/JSON continua custando cerca de 34 ms ao concluir um gesto grande. O código do histórico não foi reescrito; houve variação de aproximadamente 4% na conclusão, dentro da dispersão observada, **sem ganho demonstrado nessa fase**. Serialização/diff do documento é o próximo gargalo do editor medido nesta rodada.

A carga nativa é outra fixture explícita: retângulos em grade de espaçamento 1, N/2 sólidos, 10 controladores, sem áreas/texturas. Editor em Cena com Colisores ligado; player com um passo e a UI/renderização reais. Medianas CPU do host, sem instrumentação:

| Host | N | Antes (ms) | Depois (ms) |
|---|---:|---:|---:|
| editor | 100 | 0.730 | 0.738 |
| editor | 1600 | 26.063 | 6.885 |
| player | 100 | 0.955 | 0.327 |
| player | 1600 | 111.339 | 2.382 |

As [capturas da referência](results/native-reference-editor-1600.png) e [da versão otimizada](results/native-final-editor-1600.png) mostram a mesma carga/overlays. O editor de 100 objetos ficou estável (variação de 0,008 ms); não se atribui ganho a esse caso. Os JSON nativos incluem todas as 101 amostras e os percentis. O mesmo computador foi usado nos dois lados; o arquivo `native-final-environment.json` registra GPU/driver e as opções. A referência nativa foi compilada em 90cbcb0 com apenas o harness de testes e suas dependências de desenvolvimento adicionados.

## O que mudou e por quê

1. **Índices por fase:** `SceneView` empresta uma cena imutável, com ID→posição e pai→filhos; não pode sobreviver a uma mutação dessa cena. Percursos seguem a ordem do vetor, com conjuntos apenas para visitas/deduplicação. Ancestralidade sobe pelos pais; irmãos continuam interagindo. Runtime mantém índice próprio invalidado por `scene_mut`, remoção/criação e mudança de cena; seu Project tornou-se privado para impedir desvio dessa invalidação. Igual contagem ou reordenação não são usados como sinal de validade. Duplicados falham; não há fallback silencioso para varredura. Os métodos lineares de `Scene` permanecem para operações isoladas e como referência nos testes; os percursos frequentes migraram para a visão indexada.
2. **Transformações:** a visão calcula cada matriz uma vez e reutiliza ancestrais. Movimento prepara matrizes/caixas numa fase e atualiza a subárvore de cada controlador antes do próximo. Animação e áreas usam uma fase nova após as modificações. Picking, overlays, enquadramento, seleção e amostragem utilizam os dados do documento/prévia/runtime correto. O cálculo físico central permanece o mesmo, inclusive pivôs, escalas negativas e caixas alinhadas aos eixos.
3. **Grafos e acertos:** catálogo `OnceLock`, grafos preparados imutáveis compartilhados e índices de nós/portas. Contextos têm saídas próprias; leitura de atributo permanece dinâmica. O grafo de 64 nós copiava 4.096 nós por passo, agora zero em regime estável. Registros de acerto têm posse pelas tarefas prontas/atrasadas e pela área ativa; o mapa de localização guarda referências fracas e remove ativações encerradas. Stop/remoção/troca de cena cancelam usos. Sair e reentrar na mesma área não renova o acerto.
4. **Grade conservadora 2D, decidida após as etapas anteriores:** antes dela, em 1.600 entidades, movimento gastava 1,44 ms e examinava 7.990 candidatos; áreas esparsas, 1,91 ms e 15.890 candidatos. O movimento respondia por aproximadamente 73% desse último passo. A filtragem ainda comparava muitos objetos distantes, justificando o experimento. A grade de células de 4 unidades reduziu os candidatos a cerca de 73 / 237 e o tempo a 0,56 / 1,17 ms. Preparação de matrizes/caixas/grade agora custa cerca de 0,50 ms no movimento, filtro 0,012 ms e resolução 0,0014 ms: o próximo custo é preparar dados, não sofisticar a física.

A primeira grade piorou o denso: 4,78→5,39 ms (`shared`→`spatial`). A versão final detecta concentrações e mantém força bruta: 4,39 ms, mesmos 15.890 candidatos. Cenas com até 64 colisores e todas as 3D também usam força bruta. Caixas grandes viram extras, consultas muito grandes retornam todos; resultados são ordenados/deduplicados. O volume consultado cobre o deslocamento. Se há penetração inicial, a resolução recebe todos os obstáculos para preservar a recuperação sequencial e seus efeitos em cascata. Subárvores movidas viram extras conservadores até a próxima fase. Não há nova física ou culling visual.

O marco só de índices também expôs uma regressão em passos sem física (0,218 ms em 1.600 estáticos). As saídas antecipadas antes de criar avaliações a corrigiram: aproximadamente 0,005 ms no final. Os números de todos os marcos permanecem nos arquivos brutos; não foram substituídos pelo resultado mais favorável.

## Retenção, equivalência e limites

O ensaio prolongado adicional tem 100 objetos iniciais, 18 nós/16 conexões: 2.000 ataques, 2.000 cópias criadas/removidas por nós, duas escritas com leitura atualizada do atributo por ataque, esperas e 32 ativações de área via ações nativas. Depois de desativar a área e consumir as esperas, **100 objetos, Contador=4.000 e os mesmos valores de Vida** nos dois runtimes. Referência: 2.001 registros de acerto e 1 ativação ainda retidos, inclusive após Stop. Final: zero registros, ativações e tarefas. O ensaio inicial de 2.000 ataques sem criação também cai de 2.000 registros retidos para zero. O trabalho prolongado completo levou 231,34 → 104,58 ms no total; é uma medição única de duração, sem percentis.

`extra-complete-*.json` guarda checkpoints e estados finais. A comparação verifica igualdade exata de objetos/atributos ao drenar esse trabalho e de transforms, atributos, logs e rastros ordenados em cinco replays independentes de 60 passos/200 objetos. A grade tem testes contra força bruta em 1.200 movimentos reproduzíveis, esparsos/densos/2D/3D, além de pares, caixas grandes, movimento rápido, alterações durante a fase e penetração inicial. Matrizes têm referência independente e tolerância de 1e-5; há testes de cadeia de 256 articulações e mutações com mesma quantidade de entidades.

Memória: registros/tarefas são contagens reais de dados vivos; histórico informa estimativa interna de bytes. Os arquivos de ambiente adicionais medem `PeakWorkingSet64` do Windows enquanto o processo vive (poll a cada 100 ms), incluindo biblioteca/alocador. A retenção do alocador não é equiparada a vazamento. No harness adicional completo, o pico observado foi 39.710.720 → 40.235.008 bytes (37,87 → 38,37 MiB): **não houve redução demonstrada do pico residente**. Índices e temporários têm custo próprio; sem um profiler de alocação não se atribui essa diferença de 0,5 MiB a uma estrutura específica. O ganho de memória demonstrado é encerrar os dados vivos dos ataques, não diminuir toda a RAM do processo. Esta carga sem texturas não equivale a um projeto real com imagens. Os resultados originais não mediram memória residente, portanto não se inventa uma comparação retroativa para `baseline.json`. Não há RAM/VRAM sintética no painel.

Validação: 105 testes regulares passaram em release; o workspace e a feature `profiling` também foram testados, incluindo dois testes adicionais de contadores. fmt, build locked e Clippy `--all-targets --all-features -- -D warnings`. Cinco testes nativos passaram: editor, colisor/pivô, player, GPU/depth/UV/resize e tela inicial. A QA cobre pintura/PNG, animação, grafos, multisseleção, salvar/reabrir, as três demonstrações, pan/zoom/DPI e 920×600. Evidências em `../qa/v0.1.3/`. Houve uma execução inicial de testes de escrita bloqueada pelo sandbox; a mesma suíte passou com acesso aos temporários do Windows. A QA da tela inicial em release passou após provisionar a pasta `data` adjacente, como no ZIP, sem habilitar fallback para o código-fonte.

Pacote 0.1.3 gerado e inspecionado (x64, ícone/metadados, dependências e três cenas). O EXE extraído abriu fora do repositório com Rust/Cargo ausentes do PATH. **Não houve segunda máquina/Windows Sandbox, validação MSVC local ou execução do CI remoto.** Os testes de GPU validam imagens/resultados, não tempo GPU. A medição nativa usa entrada egui isolada e janelas reais; não é um playthrough humano prolongado. Não há promessa universal de objetos/FPS. 16,67 ms é apenas o orçamento total de um quadro a 60 Hz.

Histórico por deltas, CPU texture cache sob demanda, caches GPU, repouso sob demanda, tela inicial, player e distribuição portátil foram preservados. O ensaio de repouso registrou uma atualização espontânea em 500 ms depois da estabilização; isso não é uma medição de consumo total de energia/CPU. Schema permanece 1. Os próximos gargalos medidos são diff/serialização do histórico e preparação de matrizes em cenas com muita decoração; GPU e projetos pesados em texturas ainda precisam de medições próprias.

## Reproduzir

Na raiz do projeto, Windows PowerShell (rótulos novos preservam os resultados anteriores):

```powershell
./scripts/benchmark.ps1 -Label minha-medicao
./scripts/benchmark.ps1 -Label minhas-ferramentas -Example performance_extra
./scripts/benchmark-native.ps1 -Label meus-hosts
./scripts/compare-benchmarks.ps1 -Before baseline -After minha-medicao
./scripts/compare-benchmarks.ps1 -Before extra-complete-reference -After minhas-ferramentas
./scripts/cargo.ps1 test -p oxy_core --features profiling --locked
./scripts/cargo.ps1 run --locked -p oxy_editor --features oxy_core/profiling
./scripts/package-engine.ps1
& '.\dist\OXY-Engine-0.1.3-windows-x64\OXY Engine.exe'
./scripts/test-portable.ps1 -Zip './dist/OXY-Engine-0.1.3-windows-x64.zip'
```

A instrumentação aparece em **Desempenho** quando compilada com `oxy_core/profiling`; o pacote padrão a desliga. Contadores do painel são totais desde sua última atualização, não médias por frame. Em sistemas com Cargo no PATH, os comandos Rust podem usar `cargo` diretamente. Workflow `performance.yml` é manual e guarda artifacts; o CI comum testa invariantes/contadores, sem falhar por milissegundos de runner compartilhado.

Para recriar a referência: `git worktree add --detach .tools/perf-reference 90cbcb0`; copie **somente** `crates/oxy_render/examples/performance*.rs` atuais para o mesmo diretório nesse worktree. Compile com `./scripts/cargo.ps1 build --manifest-path .tools/perf-reference/Cargo.toml --target-dir target/perf-reference --release --locked -p oxy_render --example performance_extra --features oxy_core/profiling`. Execute `benchmark.ps1` com `-Executable` apontando para esse EXE, `-Example performance_extra`, `-ReferenceCommit 90cbcb0` e outro rótulo. O harness principal original já está em 90cbcb0. Não use um benchmark atualizado de um lado apenas para comparar operações que mudaram.

Para a referência nativa, acrescente apenas `benchmarks/native_host.rs` e os módulos de teste `native_perf` aos mesmos caminhos do worktree, registre os módulos com `#[cfg(all(test, target_os = "windows"))]` e acrescente `serde_json`/`image` como dev-dependencies do player. `benchmark-native.ps1 -ReferenceDirectory .tools/perf-reference -Label outros-hosts-base` compila e executa esses hosts. Atualize somente a lista de dev-dependencies no Cargo.lock desse worktree (sem atualizar versões resolvidas) antes do build locked. Nenhuma mudança de runtime/renderização é necessária. Não há binários versionados; `target/`, `dist/` e `.tools/` continuam ignorados.
