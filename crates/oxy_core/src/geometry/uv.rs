use super::{ComponentId, EditableMesh};
use glam::Vec2;
/// Pick actual triangulated corner UVs. Preserve a selected face when authored islands overlap.
pub fn pick(mesh: &EditableMesh, uv: Vec2, preferred: &[ComponentId]) -> Option<ComponentId> {
    let mut first = None;
    for t in &mesh.prepared().triangles {
        let face = mesh.face(t.face).unwrap();
        let [a, b, c] = t.corners.map(|i| Vec2::from(face.corners[i].uv));
        let determinant = (b - a).perp_dot(c - a);
        if determinant.abs() < 1e-12 {
            continue;
        }
        let u = (uv - a).perp_dot(c - a) / determinant;
        let v = (b - a).perp_dot(uv - a) / determinant;
        if u >= -1e-6 && v >= -1e-6 && u + v <= 1. + 1e-6 {
            if preferred.contains(&t.face) {
                return Some(t.face);
            }
            first.get_or_insert(t.face);
        }
    }
    first
}
