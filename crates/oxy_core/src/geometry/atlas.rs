//! Conservative island packing. Occupied face rectangles are never reused implicitly.
use super::{ComponentId, EditableMesh, operations::project_polygon};
use glam::{Vec2, Vec3};
use std::collections::HashSet;
pub struct Allocation {
    pub mesh: EditableMesh,
    pub expansion: u32,
}
/// `expansion` is explicit permission: 1 preserves old UVs, 2 doubles both PNG dimensions.
/// Existing pixels must be copied at (0,0), unchanged, by the caller when expanding a texture.
pub fn allocate(
    mesh: &EditableMesh,
    new_faces: &[ComponentId],
    size: [u32; 2],
    expansion: u32,
) -> Result<Allocation, String> {
    if new_faces.is_empty() {
        return Ok(Allocation {
            mesh: mesh.clone(),
            expansion: 1,
        });
    }
    if size.iter().any(|n| n.saturating_mul(expansion) < 8) {
        return Err(
            "As novas ilhas precisam de uma imagem com pelo menos 8 pixels em cada dimensão."
                .into(),
        );
    }
    if !matches!(expansion, 1 | 2)
        || size.contains(&0)
        || size.iter().any(|n| n.saturating_mul(expansion) > 8192)
    {
        return Err("O atlas ampliado excederia o limite de 8192 pixels. Cancele ou use menos superfícies novas.".into());
    }
    let ids = new_faces.iter().copied().collect::<HashSet<_>>();
    let columns = (size[0] * expansion / 16).clamp(1, 64) as usize;
    let rows = (size[1] * expansion / 16).clamp(1, 64) as usize;
    let mut occupied = vec![false; columns * rows];
    let factor = 1. / expansion as f32;
    for face in &mesh.data().faces {
        if !ids.contains(&face.id) {
            let min = face
                .corners
                .iter()
                .map(|c| Vec2::from(c.uv) * factor)
                .fold(Vec2::splat(f32::INFINITY), Vec2::min);
            let max = face
                .corners
                .iter()
                .map(|c| Vec2::from(c.uv) * factor)
                .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
            if min.min_element() < 0. || max.max_element() > 1. {
                return Err("O mapeamento usa repetição fora da imagem. Preserve a textura e organize esse atlas antes de adicionar ilhas.".into());
            }
            // Include a one-pixel gutter to protect bilinear sampling at existing seams.
            let start = (min
                - Vec2::new(
                    1. / (size[0] * expansion) as f32,
                    1. / (size[1] * expansion) as f32,
                ))
            .max(Vec2::ZERO);
            let end = (max
                + Vec2::new(
                    1. / (size[0] * expansion) as f32,
                    1. / (size[1] * expansion) as f32,
                ))
            .min(Vec2::ONE);
            for y in (start.y * rows as f32).floor() as usize
                ..((end.y * rows as f32).ceil() as usize).min(rows)
            {
                for x in (start.x * columns as f32).floor() as usize
                    ..((end.x * columns as f32).ceil() as usize).min(columns)
                {
                    occupied[y * columns + x] = true;
                }
            }
        }
    }
    let free = occupied
        .iter()
        .enumerate()
        .filter_map(|(i, &v)| (!v).then_some(i))
        .take(new_faces.len())
        .collect::<Vec<_>>();
    if free.len() != new_faces.len() {
        return Err("Sem espaço livre no atlas para pintar as novas faces separadamente. Crie uma cópia ampliada da textura ou cancele.".into());
    }
    let mut data = mesh.data().clone();
    let mut slots = free.into_iter();
    for face in &mut data.faces {
        if ids.contains(&face.id) {
            let slot = slots.next().unwrap();
            let base = Vec2::new(
                (slot % columns) as f32 / columns as f32,
                (slot / columns) as f32 / rows as f32,
            );
            let cell = Vec2::new(1. / columns as f32, 1. / rows as f32);
            let inset = Vec2::new(
                2. / (size[0] * expansion) as f32,
                2. / (size[1] * expansion) as f32,
            );
            let points = face
                .corners
                .iter()
                .map(|c| mesh.position(c.vertex).unwrap())
                .collect::<Vec<Vec3>>();
            for (c, uv) in face.corners.iter_mut().zip(project_polygon(&points)?) {
                c.uv = (base + inset + uv * (cell - inset * 2.)).to_array();
            }
        } else if expansion != 1 {
            for c in &mut face.corners {
                c.uv = (Vec2::from(c.uv) * factor).to_array();
            }
        }
    }
    Ok(Allocation {
        mesh: EditableMesh::new(data)?,
        expansion,
    })
}
