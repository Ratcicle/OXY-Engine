//! The console as agents read it (oxy_console): editor, agent, game and logic lines, each with a
//! number that only grows, so an agent asks only for what came after its last read. Lines are
//! copied from the console and from the play test once per frame, so the game's lines remain
//! after the test stops. The Console panel itself is unchanged.
use super::*;
use oxy_core::query::{ConsoleOrigin, ConsoleQuery};
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
}

#[derive(Default)]
pub(crate) struct ConsoleFeed {
    lines: VecDeque<Line>,
    logic: VecDeque<Line>,
    last_seq: u64,
    /// What was already copied, counted as each source counts it: console messages (see
    /// [`ConsoleLog::written`]), and the play test's logs and node traces.
    editor_seen: u64,
    game_seen: u64,
    logic_seen: u64,
}

impl ConsoleFeed {
    fn push(&mut self, origin: ConsoleOrigin, text: String, warning: bool, source: Option<Source>) {
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
    /// The lines `query` asks for, oldest first, with `last_seq` for the next read and
    /// `omitted` when the limit left older ones out.
    fn answer(&self, query: &ConsoleQuery) -> Json {
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
                entry
            })
            .collect();
        let mut answer = json!({"lines": lines, "last_seq": self.last_seq});
        if omitted > 0 {
            answer["omitted"] = json!(omitted);
        }
        answer
    }
}

impl Editor {
    /// Copies the console messages and the play test's logs and logic actions that arrived
    /// since the last copy. Called every frame, before a test stops and when an agent reads.
    pub(crate) fn sync_console_feed(&mut self) {
        let feed = &mut self.console_feed;
        // Messages that also became notices read as warnings, as in the Console.
        let warnings: HashSet<&str> = self.notices.texts().collect();
        let first = self.console.dropped;
        let written = self.console.written();
        let start = feed.editor_seen.clamp(first, written);
        for message in &self.console.messages[(start - first) as usize..] {
            let warning = warnings.contains(message.as_str());
            match message.strip_prefix(agent::AGENT_PREFIX) {
                Some(text) => feed.push(ConsoleOrigin::Agente, text.to_owned(), warning, None),
                None => feed.push(ConsoleOrigin::Editor, message.clone(), warning, None),
            }
        }
        feed.editor_seen = written;
        let Some(rt) = &self.play.runtime else {
            return;
        };
        let name = |id: &str| {
            rt.scene()
                .entity(id)
                .map_or_else(|| id.to_owned(), |e| e.name.clone())
        };
        let written = rt.logs_written();
        let first = written.saturating_sub(rt.logs.len() as u64);
        let start = feed.game_seen.clamp(first, written);
        for line in &rt.logs[(start - first) as usize..] {
            // "[object / node] message" from debug.message, or a node's error: the IDs go
            // to their own fields.
            match oxy_core::headless::node_line(line) {
                Some((object, node, message)) => feed.push(
                    ConsoleOrigin::Jogo,
                    message.to_owned(),
                    false,
                    Some(Source {
                        object: object.to_owned(),
                        name: name(object),
                        node: Some(node.to_owned()),
                        operation: None,
                    }),
                ),
                None => feed.push(ConsoleOrigin::Jogo, line.clone(), false, None),
            }
        }
        feed.game_seen = written;
        let written = rt.traces_written();
        let first = written.saturating_sub(rt.traces.len() as u64);
        let start = feed.logic_seen.clamp(first, written);
        let new = &rt.traces[(start - first) as usize..];
        if !new.is_empty() {
            let operations = oxy_core::graph::registry();
            for trace in new {
                let label = operations
                    .iter()
                    .find(|op| op.id == trace.operation)
                    .map_or("Ação não reconhecida", |op| op.label);
                let object = name(&trace.object);
                // As the Console writes them.
                feed.push(
                    ConsoleOrigin::Logica,
                    format!("{:.3} s · {object} · {label}", trace.time),
                    false,
                    Some(Source {
                        object: trace.object.clone(),
                        name: object,
                        node: Some(trace.node.clone()),
                        operation: Some(trace.operation.clone()),
                    }),
                );
            }
        }
        feed.logic_seen = written;
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

    fn feed(lines: &[(ConsoleOrigin, &str)]) -> ConsoleFeed {
        let mut feed = ConsoleFeed::default();
        for (origin, text) in lines {
            feed.push(*origin, (*text).into(), false, None);
        }
        feed
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
        feed.push(ConsoleOrigin::Editor, "Depois".into(), true, None);
        let after = feed.answer(&ConsoleQuery::parse(&json!({})).unwrap());
        assert_eq!(
            after["lines"],
            json!([{"seq": LOGIC_KEPT as u64 + 52, "origin": "editor", "text": "Depois",
                    "warning": true}])
        );
    }
}
