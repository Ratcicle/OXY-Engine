# Agentes de IA (MCP)

A OXY Engine traz um servidor MCP, o `oxy-mcp.exe`, que deixa agentes de IA como o Claude ajudarem no desenvolvimento dos jogos. Com ele, o agente:
- lê o projeto e consulta o catálogo de nós;
- edita cenas, objetos, componentes e lógica;
- valida o projeto;
- roda o jogo sem janela para testar;
- captura imagens.

## Configurar

1. **No editor:** abra **Preferências do editor** (o botão **Interface: 100%** na barra de status) e ligue **Permitir agentes de IA**. A opção começa desligada. Com ela ligada, a barra de status mostra **Agentes ligados**, ou **Agente conectado** quando um agente está conectado.
2. **No Claude Code:** registre o servidor, apontando para a pasta onde está o `oxy-mcp.exe`:

   ```
   claude mcp add oxy -- "C:\caminho\da\OXY Engine\oxy-mcp.exe"
   ```

3. **No Claude Desktop:** acrescente em `claude_desktop_config.json`:

   ```json
   {
     "mcpServers": {
       "oxy": { "command": "C:\\caminho\\da\\OXY Engine\\oxy-mcp.exe" }
     }
   }
   ```

Para compilar a partir do código-fonte, use `cargo build -p oxy_mcp --release`. O executável sai em `target/release/oxy-mcp.exe`.

## Como o agente trabalha

**Ao vivo (projeto aberto no editor).** O servidor encontra o editor aberto pelo canal local e age sobre ele:
- Cada lote de comandos aparece na hora e vira um passo no histórico, com o rótulo **Agente: …**.
- O Console mostra as ações com a origem **Agente**, e você pode desfazer tudo com Ctrl+Z.
- O agente nunca salva; quem salva é você.
- Se você estiver arrastando ou digitando, o pedido espera o gesto terminar.
- Durante um teste do jogo, uma operação de modelagem em prévia ou uma pose provisória da animação, o editor recusa a edição e explica o motivo.

**Arquivos (sem editor aberto).** Com o caminho do projeto, o agente edita os arquivos diretamente:
- Antes de salvar, a versão anterior é guardada em `project.oxy.json.agente.bak`.
- Se o mesmo projeto estiver aberto num editor, o servidor sempre usa o editor, para não haver duas versões.

**Atomicidade.** Os lotes valem por inteiro: se um comando falha, nada muda, e a mensagem diz qual comando falhou e por quê. As regras são as mesmas do editor, porque a validação é a mesma de salvar.

## Ferramentas

| Ferramenta | O que faz |
| --- | --- |
| `oxy_status` | Editores abertos com agentes permitidos, projeto, cena ativa, alterações não salvas, desfazer/refazer |
| `oxy_project` | Cenas, recursos e ações de entrada |
| `oxy_scene` | Árvore de objetos da cena, com componentes e tamanho da lógica |
| `oxy_entity` | Um objeto completo e os componentes que ainda pode receber |
| `oxy_find` | Busca objetos pelo nome em todas as cenas |
| `oxy_catalog` | Nós da lógica (portas, parâmetros, descrições), componentes, formas e presets |
| `oxy_apply` | Aplica um lote de comandos (criar, transformar, componentes, atributos, nós, ligações…) |
| `oxy_validate` | Valida o projeto com as regras de salvar |
| `oxy_simulate` | Roda o jogo sem janela por um tempo, com entradas programadas, e relata logs, ações, sons, trocas de cena e objetos observados |
| `oxy_screenshot` | Imagem da vista do editor (ao vivo) ou da câmera do jogo |
| `oxy_select`, `oxy_frame` | Ao vivo: seleciona e enquadra objetos para você ver do que o agente fala |
| `oxy_undo`, `oxy_redo` | Ao vivo: desfaz ou refaz o último passo |

As descrições das ferramentas estão em inglês, para os agentes. As mensagens e erros do editor continuam em português.

## Limites desta versão

- **Imagem do jogo:** não inclui a interface do jogo (textos, botões e barras desenhados por cima da cena).
- **Simulação:** nem sempre é determinística. Objetos criados durante o jogo (`action.spawn`) recebem IDs novos a cada execução. Isso fica para a 0.6.
- **Importação:** o agente ainda não importa PNG/WAV; use **Importar** na Biblioteca.
- **Pintura e animação:** pintura e quadros-chave de animação ainda não têm comandos próprios. `update_entity` edita os campos do objeto, mas não os pixels das texturas.
- **Segurança:** o canal só aceita conexões do próprio computador (`127.0.0.1`) e exige o token que o editor grava em `%LOCALAPPDATA%\OXY Engine\agents`. Desligar a opção fecha o canal e apaga esse arquivo.
