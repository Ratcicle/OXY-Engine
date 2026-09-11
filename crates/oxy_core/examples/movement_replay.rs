#[path = "support/movement.rs"]
mod movement;
#[path = "support/replay.rs"]
mod replay;
fn main() {
    let mut runs = Vec::new();
    for mode in ["sparse", "platforms"] {
        let reference = replay::run(60, mode);
        for hz in [30, 144, 240] {
            let run = replay::run(hz, mode);
            replay::compare(&reference, &run);
            runs.push(run);
        }
        runs.push(reference);
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"12 simulation seconds, seed 0, timestamped input, 720 fixed steps; compare all graph event traces and physics state every 30 ticks at 30/60/144/240 Hz; tolerance 0.2 mm; apex/peak are sampled at presentation and not used as exact trajectory equivalence", "runs":runs})).unwrap());
}
