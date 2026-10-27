# Notas para agentes

Para quem trabalha no código da OXY Engine, pessoa ou agente de IA: como compilar sem cair nas armadilhas do Luau, como trabalhar em paralelo sem estragar o checkout e onde mora cada parte dos scripts. A visão geral está no [README](README.md); o uso do editor, no [guia do editor](docs/guia-do-editor.md); o servidor MCP, em [Agentes de IA (MCP)](docs/agentes-mcp.md).

## Compilar e conferir

- Rust **1.92.0**, fixado no `rust-toolchain.toml`. Na máquina de desenvolvimento, use `./scripts/cargo.ps1`: ele põe no PATH uma cópia do MinGW de `.tools/mingw64` (em `~/.cargo/oxy-native-toolchain`, feita na primeira vez), acha o cargo em `~/.cargo/bin` mesmo quando ele não está no PATH e repassa os argumentos. No PowerShell, escreva o `--` entre aspas (`'--'`), porque o PowerShell o engole ao repassar ao script:

  ```powershell
  ./scripts/cargo.ps1 fmt --all '--' --check
  ./scripts/cargo.ps1 clippy --workspace --all-targets '--' -D warnings
  ./scripts/cargo.ps1 test --workspace
  ```

- Os testes de integração do `oxy_core` são um binário só: um arquivo novo vai em `crates/oxy_core/tests/integration/<arquivo>.rs`, com a linha `mod <arquivo>;` no `main.rs` da pasta, e roda com `./scripts/cargo.ps1 test -p oxy_core --test integration <arquivo>::`. Eles dividem o processo: cada teste usa uma pasta temporária própria (com `new_id()`) e nenhum mexe em variáveis de ambiente nem na pasta atual.
- A suíte nativa (`./scripts/test-native.ps1`) abre janelas de verdade, precisa da GPU e grava capturas em `target/qa/`; rode-a só quando a pessoa liberar o computador.
- O `clippy.toml` proíbe `std::fs::rename` e `std::fs::copy`, que substituem o destino sem perguntar: mova e copie arquivos do projeto pelo `oxy_core::fs_ops`, que só cria.
- A versão do `Cargo.toml` só sobe no commit de lançamento. Um teste confere que o README e o CHANGELOG citam a versão do workspace.

## Luau, a parte em C++

Os scripts rodam no Luau 0.740, pelo `mlua` 0.12.2 (`features = ["luau"]`, sem JIT e sem `send`, fixado com `=` como o resto). O Luau é C++, a primeira dependência C++ da OXY, e isso pede quatro cuidados:

- **`libstdc++` estática no GNU.** O `.cargo/config.toml` da raiz põe `CXXSTDLIB_x86_64_pc_windows_gnu = "static:-bundle=stdc++"` no `[env]` de toda invocação do cargo. Sem isso, os executáveis dependem de `libstdc++-6.dll` e o jogo exportado não abre fora do PATH de desenvolvimento (erro `0xC0000135`): a exportação copia só o player, nunca as DLLs que estejam ao lado dele. O nome com o alvo GNU nunca chega ao MSVC.
- **Rode o cargo de dentro do repositório.** O cargo acha o `.cargo/config.toml` pela pasta atual, e o `cargo.ps1` não muda de pasta: chame-o como `./scripts/cargo.ps1`, da raiz. Um cargo chamado de outra pasta (ou o `cargo check` de um editor aberto fora do repositório) compila o `mlua-sys` sem a variável, e esse build fica no cache do `target/`.
- **`cargo clean -p mlua-sys` depois de mudar a variável.** O `luau0-src` lê a variável sem `rerun-if-env-changed`: o primeiro build do `mlua-sys` num perfil decide estática ou dinâmica para sempre naquele `target/`. Ao mudar a variável, ou ao desconfiar de um build dinâmico no cache, rode `cargo clean -p mlua-sys` (e de novo com `--release`) antes de compilar. O `test-portable.ps1` recusa um executável do pacote que importe o runtime do GCC (`libstdc++-6.dll`, `libgcc_s`, `libwinpthread`), o do Visual C++ ou qualquer DLL que não seja do Windows.
- **`LUAU_CXXFLAGS=/EHsc` no MSVC.** O Luau lança exceções C++ e o `cc` não passa nenhum `/EH` ao MSVC. O `ci.yml` e o `portable-windows.yml` definem a variável, e `scripts/check-luau-ehsc.ps1` confere, nos comandos que o `cc` escreve com `CC_ENABLE_DEBUG_OUTPUT=1`, que os 75 arquivos `.cpp` do `luau0-src` 0.22.0 foram compilados com ela (outra versão pede conferir a contagem de novo). O ZIP sai do MSVC em release com `+crt-static` (`-MT`), e o `portable-windows.yml` roda os testes de script nesse build; num ramo, ele roda por `gh workflow run portable-windows.yml --ref <ramo>`, depois de um push que a pessoa autorize.

Atualizar o `mlua` troca a versão do Luau: confira junto as licenças em `licenses/`, a contagem do `check-luau-ehsc.ps1` e a versão do luau-lsp que lê os tipos gerados (a 1.70.1 acompanha o Luau 0.740).

## Spikes e trabalho em paralelo

- Nunca troque o ramo do checkout compartilhado: outro agente pode estar trabalhando nele. Um spike, uma revisão ou uma etapa em paralelo vão numa `git worktree` própria: `git worktree add --detach <pasta> <ramo>` (ou `-b <ramo novo>`).
- Na worktree, rode o cargo com a pasta atual dentro dela, para valer o `.cargo/config.toml` do ramo; `CARGO_TARGET_DIR` pode apontar para o `target/` do repositório, para reaproveitar as dependências.
- A `.tools` não está no Git. Para o `cargo.ps1` funcionar na worktree, crie nela uma junção para a `.tools` do repositório (`cmd /c mklink /J <pasta>\.tools "<repositório>\.tools"`).
- Ao terminar, **apague primeiro a junção** com `cmd /c rmdir "<pasta>\.tools"` (o `rmdir` sem `/s` tira só a junção), confira que a `.tools` do repositório continua cheia e só então rode `git worktree remove <pasta>` e `git worktree prune`. Nunca use `git worktree remove --force` com a junção no lugar: a remoção pode atravessar a junção e apagar a `.tools` de verdade.

## Scripts: onde mora cada coisa

- **Host:** `crates/oxy_core/src/script.rs` e `script/`: a VM (`host.rs`), a caixa de areia (`sandbox.rs`), a carga com a conferência do bytecode (`load.rs`, `bytecode.rs`), os objetos como valores estáveis (`handles.rs`), a memória do script, os temporizadores, as mensagens, os erros em português, a conferência de nomes (`names.rs`) e o analisador léxico que reescreve os `require` (`lexer.rs`). O comentário do topo do `script.rs` explica as regras do código `unsafe`. Os efeitos que o grafo e os scripts dividem são métodos do `Runtime`, em `runtime/operations.rs`: uma correção ali vale para os dois.
- **Tabela da API (o registro):** `crates/oxy_core/src/script/registry.rs` define a forma de uma linha; as linhas ficam em `script/registry/rows.rs`, área por área, e os tipos compartilhados em `script/registry/types.rs`. Cada nome que um script alcança é uma linha, com assinatura, ajuda em português (Guia e VS Code) e em inglês (agentes), o que faz a chamada parar com erro, um exemplo que um teste roda, o domínio na 0.8 e a versão em que entrou. Função nova = uma linha, a implementação sobre a camada de operações e o exemplo, que já é o teste. Nada que um script alcança existe fora da tabela, e um teste recusa o que escapar dela.
- **Gerado da tabela** (`script/generate.rs`): `crates/oxy_core/src/script/oxy.d.luau`, os tipos que a OXY grava em `.oxy/tipos/oxy.d.luau` de cada projeto para o VS Code; `docs/api-scripts.md`, a referência; as páginas da Referência da API do Guia e a seção `api` do `oxy_catalog`, montadas na hora. Os dois arquivos ficam no repositório, e o teste `script::generate::tests::the_reference_files_match_the_table` falha quando eles não são o que a tabela gera. Para gravá-los de novo, rode os testes da biblioteca do `oxy_core` com `OXY_UPDATE_API=1` e revise a diferença:

  ```powershell
  $env:OXY_UPDATE_API = '1'; ./scripts/cargo.ps1 test -p oxy_core --lib script::generate; Remove-Item Env:OXY_UPDATE_API
  ```

- **Guia:** as páginas de Scripts estão em `crates/oxy_core/src/guide_scripts.rs`, e cada código delas é um script inteiro em `guide_scripts/*.luau`, que um teste carrega e roda. As receitas em script ficam em `examples/guia-scripts/` (lidas por `guide_recipes.rs`); as catorze em nós, em `examples/guia/`, são legado.
- **Arquivos do projeto:** `project_tree.rs` varre a pasta do projeto (a Biblioteca); `script_files.rs` grava scripts e prepara o VS Code (`.oxy/tipos`, `.luaurc`, `.vscode/settings.json`); `names.rs` tem as regras de nome do Windows; `conversion.rs` organiza em pastas um projeto antigo; `journal.rs` guarda o diário das operações de arquivo; `fs_ops.rs` move e copia sem substituir.
- **Exemplos e guardas:** o parkour (`examples/parkour`, com `Pista/`, `Menu/` e `Comum/`) roda em Luau; a versão em nós da 0.6 é a fixture `crates/oxy_core/tests/fixtures/parkour-0.6`, que `tests/integration/parkour_ab.rs` joga lado a lado com a nova. O grafo de nós está congelado desde a 0.7: nenhum nó novo, e uma refatoração mantém o `state_hash` do parkour da fixture (`071503f82e92a690`), os testes de nós e as mensagens iguais. Um teste confere que cada uma das 86 operações dos nós tem equivalente na API ou no Luau puro.
- **Editor e MCP:** o cartão Scripts fica em `crates/oxy_editor/src/app/properties/scripts.rs`, a aba do código em `app/code_view.rs`, o Console em `app/console_panel.rs`, a Biblioteca em `app/library.rs` e `app/library/`, e o Guia em `app/logic/guide.rs`. As ferramentas MCP ficam em `crates/oxy_mcp/src/tools.rs`, e a tabela de `docs/agentes-mcp.md` precisa listar todas (um teste confere).

## Textos

O que a pessoa lê é em português do Brasil: o editor, as mensagens de erro (também as dos scripts), o Guia e os documentos. As descrições das ferramentas MCP são em inglês, como os nomes da API dos scripts (com sublinhado: `scene.find`, `on_area_enter`) e os comentários do código. Nomes de objetos, atributos, sons e cenas são os que a pessoa deu. Nenhuma mensagem mostra IDs internos.
