//! The console as agents read it (oxy_console): editor, agent, game and logic lines, each with a
//! number that grows while the editor stays open, so an agent asks only for what came after its
//! last read. Numbering starts over when the editor is reopened: `session` names each run, and
//! a `since` past the last line reads from the start again (`reset`). Lines are copied from the
//! console and from the play test once per frame, so the game's lines remain after the test
//! stops. The Console panel itself is unchanged.
use super::*;
use oxy_core::query::{ConsoleOrigin, ConsoleQuery};
use oxy_core::runtime::{LogLine, ScriptLocation};
use serde_json::{Value as Json, json};
use std::collections::{HashSet, VecDeque};

/// Editor, agent and game lines kept.
const KEPT: usize = 1000;
/// Logic lines, kept apart: logic that runs every step would otherwise push the rest out.
const LOGIC_KEPT: usize = 300;

/// The object a game or logic line comes from, with the node that logged it or the
/// operation that ran.
struct Source {
    object: Id,
    name: String,
    node: Option<Id>,
    operation: Option<String>,
}

struct Line {
    seq: u64,
    origin: ConsoleOrigin,
    text: String,
    warning: bool,
    source: Option<Source>,
    /// The script line a game line came from.
    script: Option<ScriptLocation>,
}

pub(crate) struct ConsoleFeed {
    lines: VecDeque<Line>,
    logic: VecDeque<Line>,
    last_seq: u64,
    /// Names this run of the editor: `seq` restarts from 1 in another.
    session: Id,
    /// What was already copied, counted as each source counts it: console messages (see
    /// [`ConsoleLog::written`]), and the play test's logs and node traces.
    editor_seen: u64,
    game_seen: u64,
    logic_seen: u64,
}

impl Default for ConsoleFeed {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            logic: VecDeque::new(),
            last_seq: 0,
            session: new_id(),
            editor_seen: 0,
            game_seen: 0,
            logic_seen: 0,
        }
    }
}

/// Of `kept`, the last lines of the `written` a source ever wrote, those after the first
/// `seen`. Lines the source dropped or cleared before they were copied are not there to copy.
fn unseen<T>(kept: &[T], written: u64, seen: u64) -> &[T] {
    let first = written.saturating_sub(kept.len() as u64);
    &kept[(seen.clamp(first, written) - first) as usize..]
}

impl ConsoleFeed {
    fn push(
        &mut self,
        origin: ConsoleOrigin,
        text: String,
        warning: bool,
        source: Option<Source>,
        script: Option<ScriptLocation>,
    ) {
        self.last_seq += 1;
        let (list, kept) = if origin == ConsoleOrigin::Logica {
            (&mut self.logic, LOGIC_KEPT)
        } else {
            (&mut self.lines, KEPT)
        };
        list.push_back(Line {
            seq: self.last_seq,
            origin,
            text,
            warning,
            source,
            script,
        });
        while list.len() > kept {
            list.pop_front();
        }
    }
    /// A new play test: its logs and traces count from zero.
    pub(crate) fn new_test(&mut self) {
        self.game_seen = 0;
        self.logic_seen = 0;
    }
    /// The console was cleared: agents no longer read its lines either. Numbers keep growing.
    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.logic.clear();
    }
    /// Copies the console messages written since the last copy. Those that also became notices
    /// (`warnings`) read as warnings, as in the Console.
    fn copy_console(&mut self, console: &ConsoleLog, warnings: &HashSet<&str>) {
        let written = console.written();
        for message in unseen(&console.messages, written, self.editor_seen) {
            let warning = warnings.contains(message.as_str());
            // The script line a message names, as a reload that failed names it.
            let script = oxy_core::script::location_in(message);
            match message.strip_prefix(agent::AGENT_PREFIX) {
                Some(text) => self.push(
                    ConsoleOrigin::Agente,
                    text.to_owned(),
                    warning,
                    None,
                    script,
                ),
                None => self.push(
                    ConsoleOrigin::Editor,
                    message.clone(),
                    warning,
                    None,
                    script,
                ),
            }
        }
        self.editor_seen = written;
    }
    /// Copies the play test's logs and the logic actions it ran since the last copy.
    fn copy_test(&mut self, rt: &Runtime) {
        let name = |id: &str| {
            rt.scene()
                .entity(id)
                .map_or_else(|| id.to_owned(), |e| e.name.clone())
        };
        let written = rt.logs_written();
        for line in unseen(&rt.logs, written, self.game_seen) {
            self.copy_log(line, name);
        }
        self.game_seen = written;
        let written = rt.traces_written();
        let new = unseen(&rt.traces, written, self.logic_seen);
        if !new.is_empty() {
            let operations = oxy_core::graph::registry();
            for trace in new {
                let label = operations
                    .iter()
                    .find(|op| op.id == trace.operation)
                    .map_or("Ação não reconhecida", |op| op.label);
                let object = name(&trace.object);
                // As the Console writes them.
                self.push(
                    ConsoleOrigin::Logica,
                    format!("{:.3} s · {object} · {label}", trace.time),
                    false,
                    Some(Source {
                        object: trace.object.clone(),
                        name: object,
                        node: Some(trace.node.clone()),
                        operation: Some(trace.operation.clone()),
                    }),
                    None,
                );
            }
        }
        self.logic_seen = written;
    }
    /// A play test's log line, with the script line it came from. "[object / node] message"
    /// from debug.message, or a node's error: the IDs go to their own fields.
    fn copy_log(&mut self, line: &LogLine, name: impl Fn(&str) -> String) {
        let script = line.script.clone();
        match oxy_core::headless::node_line(&line.text) {
            Some((object, node, message)) => self.push(
                ConsoleOrigin::Jogo,
                message.to_owned(),
                false,
                Some(Source {
                    object: object.to_owned(),
                    name: name(object),
                    node: Some(node.to_owned()),
                    operation: None,
                }),
                script,
            ),
            None => self.push(ConsoleOrigin::Jogo, line.text.clone(), false, None, script),
        }
    }
    /// The lines `query` asks for, oldest first, with `last_seq` for the next read, `session`,
    /// and `omitted` when the limit left older ones out.
    fn answer(&self, query: &ConsoleQuery) -> Json {
        // A since past the last line was read from an editor since closed, whose numbers went
        // further: the read starts from the oldest line kept instead of answering nothing.
        let since = query.since.filter(|since| *since <= self.last_seq);
        let reset = since != query.since;
        let query = ConsoleQuery {
            since,
            ..query.clone()
        };
        let mut wanted: Vec<&Line> = self
            .lines
            .iter()
            .chain(&self.logic)
            .filter(|line| query.wants(line.seq, line.origin))
            .collect();
        wanted.sort_by_key(|line| line.seq);
        let omitted = wanted.len().saturating_sub(query.limit);
        let lines: Vec<Json> = wanted[omitted..]
            .iter()
            .map(|line| {
                let mut entry = json!({"seq": line.seq, "origin": line.origin, "text": line.text});
                if line.warning {
                    entry["warning"] = json!(true);
                }
                if let Some(source) = &line.source {
                    entry["object"] = json!({"id": source.object, "name": source.name});
                    if let Some(node) = &source.node {
                        entry["node"] = json!(node);
                    }
                    if let Some(operation) = &source.operation {
                        entry["operation"] = json!(operation);
                    }
                }
                if let Some(script) = &line.script {
                    entry["script"] = json!(script);
                }
                entry
            })
            .collect();
        let mut answer =
            json!({"lines": lines, "last_seq": self.last_seq, "session": self.session});
        if omitted > 0 {
            answer["omitted"] = json!(omitted);
        }
        if reset {
            answer["reset"] = json!(true);
        }
        answer
    }
}

impl Editor {
    /// Copies the console messages and the play test's logs and logic actions that arrived
    /// since the last copy. Called every frame, before a test stops and when an agent reads.
    pub(crate) fn sync_console_feed(&mut self) {
        let warnings: HashSet<&str> = self.notices.texts().collect();
        self.console_feed.copy_console(&self.console, &warnings);
        if let Some(rt) = &self.play.runtime {
            self.console_feed.copy_test(rt);
        }
    }
    /// The console lines an agent asks for (oxy_console).
    pub(crate) fn console_answer(&mut self, params: &Json) -> Result<Json, String> {
        let query = ConsoleQuery::parse(params)?;
        self.sync_console_feed();
        Ok(self.console_feed.answer(&query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::runtime::{FIXED_DT, InputFrame};

    fn feed(lines: &[(ConsoleOrigin, &str)]) -> ConsoleFeed {
        let mut feed = ConsoleFeed::default();
        for (origin, text) in lines {
            feed.push(*origin, (*text).into(), false, None, None);
        }
        feed
    }

    fn texts(feed: &ConsoleFeed, origin: ConsoleOrigin) -> Vec<&str> {
        let mut lines: Vec<&Line> = feed
            .lines
            .iter()
            .chain(&feed.logic)
            .filter(|line| line.origin == origin)
            .collect();
        lines.sort_by_key(|line| line.seq);
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    /// A play test whose object "Falante" logs "Passo" every step: two logic actions and one
    /// log line a step.
    fn chatty_test() -> Runtime {
        let mut project = editing::blank_project("Console", SceneKind::TwoD).unwrap();
        let scene = project.start_scene.clone();
        oxy_core::command::apply(
            &mut project,
            &scene,
            &oxy_core::command::parse(&json!([
                {"op": "create_entity", "kind": {"type": "group"}, "name": "Falante"},
                {"op": "add_node", "entity": "$0", "operation": "event.step"},
                {"op": "add_node", "entity": "$0", "operation": "debug.message",
                 "params": {"message": {"Text": "Passo"}}},
                {"op": "connect", "entity": "$0", "from_node": "$1", "from_port": "exec",
                 "to_node": "$2", "to_port": "exec"},
            ]))
            .unwrap(),
        )
        .unwrap();
        Runtime::new(&project, &scene).unwrap()
    }

    #[test]
    fn reads_take_the_lines_after_since_of_an_origin_up_to_the_limit() {
        use ConsoleOrigin::*;
        let feed = feed(&[
            (Editor, "Projeto aberto"),
            (Agente, "Criar objeto"),
            (Jogo, "Olá"),
            (Logica, "0.000 s · Porta · Mensagem"),
            (Jogo, "Tchau"),
        ]);
        let read = |args: Json| feed.answer(&ConsoleQuery::parse(&args).unwrap());
        let all = read(json!({}));
        assert_eq!(all["last_seq"], 5);
        assert_eq!(all["lines"].as_array().unwrap().len(), 5);
        assert_eq!(all["lines"][3]["origin"], "logica", "merged in seq order");
        assert!(all.get("omitted").is_none());
        let game = read(json!({"origin": "jogo", "since": 3}));
        assert_eq!(
            game["lines"],
            json!([{"seq": 5, "origin": "jogo", "text": "Tchau"}])
        );
        let last = read(json!({"origin": ["agente", "jogo"], "limit": 1}));
        assert_eq!(last["lines"][0]["text"], "Tchau");
        assert_eq!(last["omitted"], 2);
        assert!(ConsoleQuery::parse(&json!({"origin": "lógica"})).is_err());
        assert!(ConsoleQuery::parse(&json!({"since": -1})).is_err());
    }

    #[test]
    fn a_since_from_an_editor_since_closed_reads_from_the_start_and_says_so() {
        use ConsoleOrigin::*;
        // The editor was reopened: numbering started over, and the agent kept 120.
        let feed = feed(&[(Editor, "Projeto aberto"), (Jogo, "Apertei J")]);
        let read = |args: Json| feed.answer(&ConsoleQuery::parse(&args).unwrap());
        let stale = read(json!({"since": 120, "origin": "jogo"}));
        assert_eq!(stale["reset"], true);
        assert_eq!(
            stale["lines"],
            json!([{"seq": 2, "origin": "jogo", "text": "Apertei J"}])
        );
        assert_eq!(stale["last_seq"], 2);
        // A current since reads as before: nothing new, no reset.
        let current = read(json!({"since": 2}));
        assert_eq!(current["lines"], json!([]));
        assert!(current.get("reset").is_none());
        // Each run of the editor has its own session, the same on every read.
        assert!(stale["session"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(stale["session"], current["session"]);
        assert_ne!(
            read(json!({}))["session"],
            ConsoleFeed::default().answer(&ConsoleQuery::parse(&json!({})).unwrap())["session"]
        );
    }

    #[test]
    fn logic_lines_cannot_push_the_others_out_and_clearing_keeps_numbering() {
        let mut feed = feed(&[(ConsoleOrigin::Jogo, "Olá")]);
        for step in 0..LOGIC_KEPT + 50 {
            feed.push(
                ConsoleOrigin::Logica,
                format!("passo {step}"),
                false,
                Some(Source {
                    object: "porta".into(),
                    name: "Porta".into(),
                    node: Some("no".into()),
                    operation: Some("event.step".into()),
                }),
                None,
            );
        }
        let all = feed.answer(&ConsoleQuery::parse(&json!({"limit": 10_000})).unwrap());
        let lines = all["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 1 + LOGIC_KEPT);
        assert_eq!(lines[0]["text"], "Olá");
        assert_eq!(lines[1]["object"], json!({"id": "porta", "name": "Porta"}));
        assert_eq!(lines[1]["operation"], "event.step");
        assert_eq!(lines[1]["node"], "no");
        feed.clear();
        feed.push(ConsoleOrigin::Editor, "Depois".into(), true, None, None);
        let after = feed.answer(&ConsoleQuery::parse(&json!({})).unwrap());
        assert_eq!(
            after["lines"],
            json!([{"seq": LOGIC_KEPT as u64 + 52, "origin": "editor", "text": "Depois",
                    "warning": true}])
        );
    }

    #[test]
    fn console_messages_are_copied_once_past_the_consoles_trims_and_clears() {
        let mut feed = ConsoleFeed::default();
        let mut console = ConsoleLog {
            messages: Vec::new(),
            dropped: 0,
        };
        let warned = format!("{}Erro do agente", agent::AGENT_PREFIX);
        let warnings = HashSet::from([warned.as_str()]);
        // Copied every frame, as the editor does, past the console's 600 kept.
        for n in 0..700 {
            console.push(format!("Mensagem {n}"));
            feed.copy_console(&console, &warnings);
        }
        let expected: Vec<String> = (0..700).map(|n| format!("Mensagem {n}")).collect();
        assert_eq!(texts(&feed, ConsoleOrigin::Editor), expected);
        // Many at once, across three trims: each copied once, in order.
        for n in 700..1000 {
            console.push(format!("Mensagem {n}"));
        }
        assert_eq!(console.dropped, 400);
        feed.copy_console(&console, &warnings);
        let expected: Vec<String> = (0..1000).map(|n| format!("Mensagem {n}")).collect();
        assert_eq!(texts(&feed, ConsoleOrigin::Editor), expected);
        assert_eq!(feed.last_seq, 1000, "no message copied twice");
        // A cleared console: only what comes after is copied, an agent's line by its origin.
        console.clear();
        feed.clear();
        console.push(warned.clone());
        console.push("Depois".into());
        feed.copy_console(&console, &warnings);
        feed.copy_console(&console, &warnings);
        let after = feed.answer(&ConsoleQuery::parse(&json!({})).unwrap());
        assert_eq!(
            after["lines"],
            json!([
                {"seq": 1001, "origin": "agente", "text": "Erro do agente", "warning": true},
                {"seq": 1002, "origin": "editor", "text": "Depois"},
            ])
        );
    }

    #[test]
    fn editor_and_agent_lines_name_the_script_line_they_point_at() {
        let mut feed = ConsoleFeed::default();
        let mut console = ConsoleLog {
            messages: Vec::new(),
            dropped: 0,
        };
        console.push(
            "Script não recarregado: Pista/porta.luau:12: erro de sintaxe. O teste segue com a \
             versão anterior."
                .into(),
        );
        console.push(format!(
            "{}Comando 0 (attach_script, o 1º do lote): Pista/porta.luau:3: audio.play: …",
            agent::AGENT_PREFIX
        ));
        console.push("Script recarregado: Pista/porta.luau (1 objeto)".into());
        feed.copy_console(&console, &HashSet::new());
        let read = feed.answer(&ConsoleQuery::parse(&json!({})).unwrap());
        let lines = read["lines"].as_array().unwrap();
        assert_eq!(
            lines[0]["script"],
            json!({"path": "Pista/porta.luau", "line": 12})
        );
        assert_eq!(lines[1]["origin"], "agente");
        assert_eq!(
            lines[1]["script"],
            json!({"path": "Pista/porta.luau", "line": 3})
        );
        assert!(lines[2].get("script").is_none(), "no line to open");
    }

    #[test]
    fn a_play_test_is_copied_once_past_the_runtimes_drains_clears_and_restarts() {
        let mut feed = ConsoleFeed::default();
        let mut runtime = chatty_test();
        let frame = |runtime: &mut Runtime, feed: &mut ConsoleFeed| {
            runtime.advance(FIXED_DT, &InputFrame::default());
            feed.copy_test(runtime);
        };
        // Copied every frame past the runtime's drains: 500 logs and 256 traces.
        for _ in 0..600 {
            frame(&mut runtime, &mut feed);
        }
        let logs = runtime.logs_written();
        assert!(logs > 500 && runtime.traces_written() > 256, "{logs}");
        let game = texts(&feed, ConsoleOrigin::Jogo);
        assert_eq!(game.len() as u64, logs, "every log line, once");
        assert!(game.iter().all(|text| *text == "Passo"), "{game:?}");
        assert_eq!(
            feed.last_seq,
            logs + runtime.traces_written(),
            "every line copied once, none lost"
        );
        assert_eq!(texts(&feed, ConsoleOrigin::Logica).len(), LOGIC_KEPT);
        let logic = feed.logic.back().unwrap();
        assert_eq!(logic.source.as_ref().unwrap().name, "Falante");
        // Clearing the Console during the test (as clear_console does): the lines after it only.
        let seq = feed.last_seq;
        let (logs, traces) = (runtime.logs_written(), runtime.traces_written());
        runtime.logs.clear();
        feed.clear();
        feed.copy_test(&runtime);
        assert_eq!(feed.last_seq, seq, "nothing new, nothing copied again");
        for _ in 0..30 {
            frame(&mut runtime, &mut feed);
        }
        assert_eq!(
            texts(&feed, ConsoleOrigin::Jogo).len() as u64,
            runtime.logs_written() - logs
        );
        assert_eq!(
            feed.last_seq - seq,
            runtime.logs_written() - logs + runtime.traces_written() - traces
        );
        // A new test counts from zero; its lines come after the last one's.
        let seq = feed.last_seq;
        let mut runtime = chatty_test();
        feed.new_test();
        for _ in 0..10 {
            frame(&mut runtime, &mut feed);
        }
        assert_eq!(
            feed.last_seq - seq,
            runtime.logs_written() + runtime.traces_written()
        );
        assert!(runtime.logs_written() >= 9, "{}", runtime.logs_written());
    }

    #[test]
    fn game_lines_keep_the_script_line_they_came_from() {
        let mut feed = ConsoleFeed::default();
        let name = |id: &str| id.to_owned();
        let line = |text: &str, script: Option<ScriptLocation>| LogLine {
            text: text.into(),
            script,
        };
        feed.copy_log(&line("Abriu", None), name);
        let script = ScriptLocation {
            path: "scripts/porta.luau".into(),
            line: 12,
        };
        feed.copy_log(&line("Porta aberta", Some(script)), name);
        let read = feed.answer(&ConsoleQuery::parse(&json!({})).unwrap());
        assert_eq!(
            read["lines"],
            json!([
                {"seq": 1, "origin": "jogo", "text": "Abriu"},
                {"seq": 2, "origin": "jogo", "text": "Porta aberta",
                 "script": {"path": "scripts/porta.luau", "line": 12}},
            ])
        );
    }
}
