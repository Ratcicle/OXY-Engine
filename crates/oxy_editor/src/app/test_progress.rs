//! Progresso do teste: what Salvar progresso recorded in the editor's tests. It lives in memory
//! for the editor session, so Parar and Jogar keep it, and opening another project or Apagar
//! progresso do teste clears it. The exported game keeps its own file, which the editor never
//! reads or writes.
use super::*;
use oxy_core::{
    save::{SaveRequest, SaveSlot},
    variables,
};
use std::collections::BTreeMap;

/// Values a Console line names before it counts the rest.
const LINE_VALUES: usize = 5;

impl PlaySession {
    /// How a new test starts: draws from `seed`, and the progress earlier tests saved.
    pub(crate) fn options(&self, seed: u64) -> RuntimeOptions {
        RuntimeOptions {
            seed,
            save: Some(self.test_save.clone()),
            ..Default::default()
        }
    }

    /// Carries the running test's Salvar progresso and Apagar progresso into the test progress,
    /// oldest first, with one Console line each.
    pub(crate) fn take_saves(&mut self, project: &Project) -> Vec<String> {
        let Some(runtime) = &mut self.runtime else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for request in std::mem::take(&mut runtime.save_requests) {
            lines.push(match &request {
                SaveRequest::Write(values) => saved_line(project, &self.test_save, values),
                SaveRequest::Clear => "Teste: progresso apagado".into(),
            });
            self.test_save.apply(request);
        }
        lines
    }
}

impl Editor {
    /// The running test's progress requests into the test progress, each with its Console line.
    pub(super) fn take_test_saves(&mut self) {
        for line in self.play.take_saves(&self.state.project) {
            self.log(line);
        }
    }
}

/// "Teste: progresso salvo — Name · key = value, …" with the values the write changed: from
/// what the test progress held, or from the initial value where it held none.
fn saved_line(
    project: &Project,
    before: &SaveSlot,
    values: &BTreeMap<(Id, String), Value>,
) -> String {
    let changed = values.iter().filter(|(entry, value)| {
        let previous = before
            .values
            .get(*entry)
            .or_else(|| variables::get(project, &entry.0).map(|v| &v.value));
        previous != Some(*value)
    });
    let (changed, _) = variables::labeled(project, changed, usize::MAX);
    if changed.is_empty() {
        return "Teste: progresso salvo, sem mudanças".into();
    }
    let mut named: Vec<String> = changed
        .iter()
        .take(LINE_VALUES)
        .map(|(label, value)| format!("{label} = {}", shown(value)))
        .collect();
    if changed.len() > LINE_VALUES {
        named.push(format!("e mais {}", changed.len() - LINE_VALUES));
    }
    format!("Teste: progresso salvo — {}", named.join(", "))
}

/// The values of `slot` as the Variáveis do jogo dialog lists them: name, key and value, the
/// variables in the project's order, each one's own value before its keys. Values of variables
/// the project no longer has are left out.
pub(crate) fn saved_rows(project: &Project, slot: &SaveSlot) -> Vec<(String, String, String)> {
    project
        .variables
        .iter()
        .flat_map(|variable| {
            slot.values
                .range((variable.id.clone(), String::new())..)
                .take_while(move |((id, _), _)| *id == variable.id)
                .map(move |((_, key), value)| (variable.name.clone(), key.clone(), shown(value)))
        })
        .collect()
}

/// A saved value as the Console and the dialog show it: numbers with at most 4 decimals, since
/// a time the game measured has many more, and vectors with ";" between their numbers.
fn shown(value: &Value) -> String {
    let number = |number: f64| {
        let text = format!("{number:.4}");
        match text.trim_end_matches('0').trim_end_matches('.') {
            "-0" => "0".to_owned(),
            text => text.to_owned(),
        }
    };
    let vector = |numbers: &[f32]| {
        numbers
            .iter()
            .map(|&n| number(n.into()))
            .collect::<Vec<_>>()
            .join("; ")
    };
    match value {
        Value::Number(n) => number(*n),
        Value::Vector2(v) => vector(v),
        Value::Vector3(v) => vector(v),
        other => logic::value_text(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxy_core::graph::{Edge, Node};

    /// A game whose start logic adds 1 to the saved Recorde, keyed by Fase when `keyed`, and
    /// saves; Tentativas counts too, but is never saved.
    fn counter() -> (Project, Id) {
        let mut project = Project::new("Contagem");
        let record = variables::create(&mut project, "Recorde", Value::Number(0.), true).unwrap();
        let tries =
            variables::create(&mut project, "Tentativas", Value::Number(0.), false).unwrap();
        let mut entity = Entity::new("Contador", None);
        let node = |entity: &mut Entity, operation: &str, params: &[(&str, Value)]| {
            let mut node = Node::new(operation, [0.; 2]);
            for (key, value) in params {
                node.params.insert((*key).into(), value.clone());
            }
            let id = node.id.clone();
            entity.graph.nodes.push(node);
            id
        };
        let wire = |entity: &mut Entity, from: &str, port: &str, to: &str, input: &str| {
            entity.graph.edges.push(Edge {
                from_node: from.into(),
                from_port: port.into(),
                to_node: to.into(),
                to_port: input.into(),
            });
        };
        let start = node(&mut entity, "event.scene_start", &[]);
        let key = ("key", Value::Text("Fase".into()));
        let variable = |id: &Id| ("variable", Value::Text(id.clone()));
        let read = node(
            &mut entity,
            "variable.get",
            &[variable(&record), key.clone()],
        );
        let add = node(&mut entity, "math.binary", &[("b", Value::Number(1.))]);
        let write = node(&mut entity, "variable.set", &[variable(&record), key]);
        let count = node(
            &mut entity,
            "variable.set",
            &[variable(&tries), ("value", Value::Number(5.))],
        );
        let save = node(&mut entity, "save.write", &[]);
        wire(&mut entity, &start, "exec", &write, "exec");
        wire(&mut entity, &read, "value", &add, "a");
        wire(&mut entity, &add, "value", &write, "value");
        wire(&mut entity, &write, "exec", &count, "exec");
        wire(&mut entity, &count, "exec", &save, "exec");
        project.scenes[0].entities.push(entity);
        validate_project(&project).unwrap();
        (project, record)
    }

    #[test]
    fn the_test_progress_survives_parar_and_jogar_until_cleared() {
        let (project, record) = counter();
        let scene = project.start_scene.clone();
        let mut play = PlaySession::default();
        for expected in [1., 2., 3.] {
            // Jogar: the test starts from what the earlier ones saved.
            play.runtime = Some(Runtime::with_options(&project, &scene, play.options(7)).unwrap());
            assert_eq!(
                play.take_saves(&project),
                [format!(
                    "Teste: progresso salvo — Recorde · Fase = {expected}"
                )]
            );
            assert!(play.take_saves(&project).is_empty());
            // Parar.
            play.runtime = None;
        }
        assert!(play.test_save.exists);
        assert_eq!(
            play.test_save.values[&(record.clone(), "Fase".to_owned())],
            Value::Number(3.)
        );
        let row = |key: &str, value: &str| ("Recorde".to_owned(), key.to_owned(), value.to_owned());
        // Only the persistent variable is kept, its own value first.
        assert_eq!(
            saved_rows(&project, &play.test_save),
            [row("", "0"), row("Fase", "3")]
        );
        // A write that changes nothing says so; Apagar progresso empties it.
        let unchanged = play.test_save.values.clone();
        assert_eq!(
            saved_line(&project, &play.test_save, &unchanged),
            "Teste: progresso salvo, sem mudanças"
        );
        play.runtime = Some(Runtime::with_options(&project, &scene, play.options(7)).unwrap());
        play.runtime
            .as_mut()
            .unwrap()
            .save_requests
            .push(SaveRequest::Clear);
        let lines = play.take_saves(&project);
        assert_eq!(lines.last().unwrap(), "Teste: progresso apagado");
        assert!(!play.test_save.exists && play.test_save.values.is_empty());
        assert!(saved_rows(&project, &play.test_save).is_empty());
    }

    #[test]
    fn another_project_starts_without_the_test_progress() {
        let (project, record) = counter();
        let mut play = PlaySession::default();
        let runtime = Runtime::with_options(&project, &project.start_scene, play.options(7));
        play.runtime = Some(runtime.unwrap());
        play.seed = Some(7);
        play.capture = true;
        play.take_saves(&project);
        assert!(
            play.test_save
                .values
                .contains_key(&(record, "Fase".to_owned()))
        );
        play.forget_project();
        assert!(play.runtime.is_none() && !play.capture && play.seed.is_none());
        assert_eq!(play.test_save, SaveSlot::default());
        assert_eq!(play.options(3).save, Some(SaveSlot::default()));
    }

    #[test]
    fn saved_numbers_show_at_most_four_decimals() {
        let mut project = Project::new("Tempos");
        let time = variables::create(&mut project, "Tempo", Value::Number(0.), true).unwrap();
        let spot = variables::create(&mut project, "Ponto", Value::Vector2([0.; 2]), true).unwrap();
        // A time measured by the game, in steps of 1/60 s, and a vector of f32 noise.
        let values: BTreeMap<(Id, String), Value> = [
            ((time.clone(), "Fase".into()), Value::Number(61. / 60.)),
            ((time, String::new()), Value::Number(-0.00001)),
            ((spot, String::new()), Value::Vector2([0.1, 2.5])),
        ]
        .into();
        assert_eq!(
            saved_line(&project, &SaveSlot::default(), &values),
            "Teste: progresso salvo — Ponto = 0.1; 2.5, Tempo = 0, Tempo · Fase = 1.0167"
        );
        let slot = SaveSlot {
            values,
            exists: true,
            ..SaveSlot::default()
        };
        let shown: Vec<_> = saved_rows(&project, &slot)
            .into_iter()
            .map(|(_, _, value)| value)
            .collect();
        assert_eq!(shown, ["0", "1.0167", "0.1; 2.5"]);
    }

    #[test]
    fn console_lines_name_at_most_five_values() {
        let mut project = Project::new("Muitas");
        let id = variables::create(&mut project, "Pontos", Value::Number(0.), true).unwrap();
        let values: BTreeMap<(Id, String), Value> = (1..=7)
            .map(|level| ((id.clone(), format!("F{level}")), Value::Number(10.5)))
            .collect();
        assert_eq!(
            saved_line(&project, &SaveSlot::default(), &values),
            "Teste: progresso salvo — Pontos · F1 = 10.5, Pontos · F2 = 10.5, Pontos · F3 = \
             10.5, Pontos · F4 = 10.5, Pontos · F5 = 10.5, e mais 2"
        );
    }
}
