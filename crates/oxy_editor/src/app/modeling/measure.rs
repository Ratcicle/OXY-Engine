//! Final-only service measurements, using the same immutable preview and delta-history API.
//! These are not RawInput timings or GPU timings. Full editor updates have their own workload.
use super::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn summary(raw: &[u64]) -> Value {
    let mut times = raw.to_vec();
    times.sort_unstable();
    let n = times.len();
    json!({"samples":n,"median_ns":times.get(n/2),"p95_ns":if n>=100 {times.get(((n-1) as f64*0.95).ceil() as usize)}else{None},"p99_ns":if n>=100 {times.get(((n-1) as f64*0.99).ceil() as usize)}else{None},"raw_ns":raw})
}
impl Editor {
    pub(crate) fn measure_direct_mesh_services(
        &mut self,
        warmup: usize,
        samples: usize,
        limit_seconds: u64,
    ) -> Value {
        let base = self.state.clone();
        let scene = self.scene_id.clone();
        let selected = self.selected.clone();
        let object_selection = self.selection.clone();
        let Some(id) = selected.as_ref() else {
            return json!({"error":"No selected entity"});
        };
        let source = base
            .project
            .scene(&scene)
            .and_then(|s| s.entity(id))
            .and_then(|e| e.mesh.clone())
            .expect("Benchmark authored fixture");
        let source_face = source.data().faces[source.data().faces.len() / 3].id;
        let mut rows = Vec::new();
        for (label, operation, percentage, distance, amend) in [
            ("extrusion_100", Operation::Extrude, 100., 0.02, false),
            ("inset_70", Operation::Inset, 70., 0., false),
            ("inset_extrusion_70", Operation::Extrude, 70., 0.02, false),
            (
                "adjust_last_inset_extrusion",
                Operation::Extrude,
                70.,
                0.02,
                true,
            ),
        ] {
            let mut values: BTreeMap<String, Vec<u64>> = BTreeMap::new();
            let started = Instant::now();
            let mut completed = 0;
            let mut history_bytes = 0;
            let mut last_source_bytes = 0;
            let mut output_faces = 0;
            let mut error = None;
            for cycle in 0..warmup + samples {
                if started.elapsed() > Duration::from_secs(limit_seconds) {
                    break;
                }
                // Fixture reset is deliberately outside every timed service region.
                self.state = base.clone();
                self.scene_id = scene.clone();
                self.selected = selected.clone();
                self.selection = object_selection.clone();
                self.history = Default::default();
                self.modeling = Default::default();
                self.modeling.selection = Components {
                    mode: Mode::Face,
                    ids: vec![source_face],
                    through: false,
                };
                let result = (|| -> Result<Vec<(&str, u64)>, String> {
                    let mut record = Vec::new();
                    let t = Instant::now();
                    self.begin_mesh_operation(operation);
                    let arm = t.elapsed().as_nanos() as u64;
                    let p = self
                        .modeling
                        .preview
                        .as_mut()
                        .ok_or("Could not arm operation")?;
                    p.values = [distance, 0., 0.];
                    p.inner_size = percentage;
                    p.previous = [f32::NAN; 3];
                    let t = Instant::now();
                    self.update_mesh_preview();
                    let generate = t.elapsed().as_nanos() as u64;
                    let p = self
                        .modeling
                        .preview
                        .as_ref()
                        .ok_or("Preview unexpectedly removed")?;
                    if let Some(reason) = &p.error {
                        return Err(reason.clone());
                    }
                    let t = Instant::now();
                    self.confirm_mesh_operation();
                    let confirm = t.elapsed().as_nanos() as u64;
                    if self.history.undo_len() != 1 || self.modeling.preview.is_some() {
                        return Err("Commit did not produce exactly one completed command".into());
                    }
                    if amend {
                        let last = self
                            .modeling
                            .last_operation
                            .as_ref()
                            .ok_or("Last operation missing")?;
                        let command = last.command;
                        let mut candidate = last.source.clone();
                        candidate.amendment = Some(command);
                        candidate.inner_size = 60.;
                        candidate.values = [0.03, 0., 0.];
                        candidate.previous = [f32::NAN; 3];
                        candidate.numeric_edit = true;
                        self.modeling.preview = Some(candidate);
                        let t = Instant::now();
                        self.update_mesh_preview();
                        let adjust = t.elapsed().as_nanos() as u64;
                        if let Some(reason) =
                            self.modeling.preview.as_ref().and_then(|p| p.error.clone())
                        {
                            return Err(reason);
                        }
                        let t = Instant::now();
                        self.confirm_mesh_operation();
                        let amend_commit = t.elapsed().as_nanos() as u64;
                        if self.history.undo_len() != 1
                            || self.history.last_command_id() != Some(command)
                        {
                            return Err("Adjustment added another history command".into());
                        }
                        record.extend([
                            ("adjust_generation_validation_atlas", adjust),
                            ("replace_last_history_command", amend_commit),
                        ]);
                    } else {
                        record.extend([
                            ("arm_source_lookup", arm),
                            ("generation_validation_atlas_candidate", generate),
                            ("history_commit", confirm),
                        ]);
                    }
                    history_bytes = self.history.estimated_bytes();
                    last_source_bytes = self
                        .modeling
                        .last_operation
                        .as_ref()
                        .map_or(0, |last| last.source.source.estimated_bytes());
                    let output = self
                        .scene()
                        .entity(id)
                        .and_then(|e| e.mesh.clone())
                        .ok_or("Output mesh missing")?;
                    output_faces = output.data().faces.len();
                    let t = Instant::now();
                    self.undo(false);
                    let undo = t.elapsed().as_nanos() as u64;
                    if self.scene().entity(id).and_then(|e| e.mesh.as_ref()) != Some(&source)
                        || self.history.undo_len() != 0
                    {
                        return Err("Undo failed to restore the original source".into());
                    }
                    let t = Instant::now();
                    self.undo(true);
                    let redo = t.elapsed().as_nanos() as u64;
                    if self.scene().entity(id).and_then(|e| e.mesh.as_ref()) != Some(&output)
                        || self.history.undo_len() != 1
                    {
                        return Err("Redo failed to restore the exact result".into());
                    }
                    record.extend([("undo", undo), ("redo", redo)]);
                    Ok(record)
                })();
                match result {
                    Ok(record) => {
                        if cycle >= warmup {
                            for (key, value) in record {
                                values.entry(key.to_owned()).or_default().push(value);
                            }
                            completed += 1;
                        }
                    }
                    Err(reason) => {
                        error = Some(reason);
                        break;
                    }
                }
            }
            let summaries: BTreeMap<_, _> = values
                .iter()
                .map(|(name, values)| (name, summary(values)))
                .collect();
            rows.push(json!({"case":label,"service_cpu":summaries,"complete_cycles":completed,"limited":completed<samples,"elapsed_seconds":started.elapsed().as_secs_f64(),"error":error,
                "result_faces":output_faces,"history_estimated_bytes":history_bytes,"last_source_logical_bytes":last_source_bytes,
                "last_source_ownership":"immutable Arc shared with history; this is not an additional full allocation"}));
        }
        self.state = base;
        self.scene_id = scene;
        self.selected = selected;
        self.selection = object_selection;
        self.history = Default::default();
        self.modeling = Default::default();
        json!({"method":"Final-only common editor operation service calls, outside RawInput and redraw. Source reset excluded; generation includes validation/atlas/preview. Commit, replacement, Undo and Redo timed separately. Every cycle checks exact source/result and one-command history.","warmup_cycles":warmup,"target_samples":samples,"limit_seconds_per_case":limit_seconds,"cases":rows})
    }
}
