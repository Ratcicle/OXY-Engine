//! Same deterministic movement harness, one fixed step, shape variants explicit.
#[allow(dead_code)]
#[path = "performance_movement.rs"]
mod harness;
use harness::movement;
use oxy_core::{document::*, movement_body::MovementBody, physics3d::CollisionShape};
fn main() {
    let counts: Vec<usize> = std::env::args()
        .nth(1)
        .unwrap_or("1,10,50".into())
        .split(',')
        .map(|n| n.parse().unwrap())
        .collect();
    let mut rows = vec![];
    for count in counts {
        for name in ["capsule", "box", "sphere", "convex"] {
            if name == "convex" && count > 10 {
                continue;
            }
            let mut p = movement::fixture(count, "sparse");
            let mut piece = Entity::new("Volume", Some(Primitive::Cube));
            piece.dimensions = [0.6, 1.8, 0.6];
            let shape = match name {
                "box" => CollisionShape::Box {
                    size: piece.dimensions,
                },
                "sphere" => CollisionShape::Sphere { radius: 0.5 },
                "convex" => MovementBody::convex_from(&piece).unwrap(),
                _ => MovementBody::default().standing,
            };
            for e in &mut p.scenes[0].entities {
                if let Some(c) = &mut e.character3d {
                    c.body.standing = shape.clone();
                }
            }
            validate_project(&p).unwrap();
            eprintln!(
                "{name}: {count} characters / {} total",
                p.scenes[0].entities.len()
            );
            rows.push(serde_json::json!({"shape":name,"characters":count,"total_entities":p.scenes[0].entities.len(),"seed":0,"single":harness::measure(&p,1)}));
        }
    }
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"method":"CPU release profiling. 120 warmup steps, 301 samples, 8 second limit per case. Exactly one fixed step per sample. No GPU or FPS inference.","rows":rows})).unwrap());
}
