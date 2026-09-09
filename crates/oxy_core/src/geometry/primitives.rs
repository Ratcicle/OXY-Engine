//! Deterministic polygon primitives. Unit diameter/height; object dimensions stay external.
use super::{Corner, EditableMesh, MeshData, Shading};
use crate::document::{Entity, Primitive};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub latitude: u32,
    pub height_divisions: u32,
    pub plane_divisions: [u32; 2],
    /// Wall thickness divided by the outer radius; independent of object dimensions.
    pub wall_fraction: f32,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            latitude: 8,
            height_divisions: 1,
            plane_divisions: [1, 1],
            wall_fraction: 0.25,
        }
    }
}
impl Parameters {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=256).contains(&self.latitude)
            || !(1..=256).contains(&self.height_divisions)
            || self.plane_divisions.iter().any(|v| !(1..=256).contains(v))
            || !self.wall_fraction.is_finite()
            || !(0.001..=0.999).contains(&self.wall_fraction)
        {
            return Err("Use 2–256 divisões de latitude, 1–256 subdivisões e espessura positiva menor que o raio externo.".into());
        }
        Ok(())
    }
    pub fn for_entity(entity: &Entity) -> Self {
        entity.primitive_parameters.unwrap_or(Self {
            latitude: (entity.segments.clamp(3, 256) / 2).max(2),
            ..Default::default()
        })
    }
}

pub fn generate(
    primitive: Primitive,
    segments: u32,
    parameters: Parameters,
) -> Result<EditableMesh, String> {
    if !(3..=256).contains(&segments) {
        return Err("Use de 3 a 256 lados.".into());
    }
    parameters.validate()?;
    let mut b = Builder::default();
    match primitive {
        Primitive::Cube => cube(&mut b)?,
        Primitive::Plane => grid(&mut b, parameters.plane_divisions, true)?,
        Primitive::Rectangle | Primitive::Sprite => grid(&mut b, [1, 1], false)?,
        Primitive::Circle => circle(&mut b, segments)?,
        Primitive::Sphere => sphere(&mut b, segments, parameters.latitude)?,
        Primitive::Cylinder | Primitive::Pyramid | Primitive::Cone | Primitive::Tube => {
            radial(&mut b, primitive, segments, parameters)?
        }
    }
    b.data.complete_edges()?;
    EditableMesh::new(b.data)
}
pub fn for_entity(entity: &Entity) -> Result<EditableMesh, String> {
    generate(
        entity
            .primitive
            .ok_or("Este grupo não possui forma própria.")?,
        entity.segments.clamp(3, 256),
        Parameters::for_entity(entity),
    )
}
/// Converts unit primitive coordinates into authored local coordinates. Transform, hierarchy,
/// material, pivot, collider and clips are deliberately untouched.
pub fn convert(entity: &mut Entity) -> Result<(), String> {
    if entity.mesh.is_some() {
        return Ok(());
    }
    let unit = for_entity(entity)?;
    let mut data = unit.data().clone();
    let dimensions = Vec3::from(entity.dimensions);
    if !dimensions.is_finite() || dimensions.min_element() <= 0. {
        return Err("Dimensões inválidas; conversão cancelada.".into());
    }
    for vertex in &mut data.vertices {
        vertex.position = (Vec3::from(vertex.position) * dimensions).to_array();
    }
    for face in &mut data.faces {
        for corner in &mut face.corners {
            if let Some(normal) = corner.normal {
                corner.normal = Some((Vec3::from(normal) / dimensions).normalize().to_array());
            }
        }
    }
    let mesh = EditableMesh::new(data)?;
    entity.mesh = Some(mesh);
    entity.primitive = None;
    entity.primitive_parameters = None;
    entity.dimensions = [1.; 3];
    Ok(())
}
#[derive(Default)]
struct Builder {
    data: MeshData,
}
impl Builder {
    fn vertex(&mut self, p: Vec3) -> Result<u32, String> {
        self.data.add_vertex(p)
    }
    fn face(&mut self, ids: &[u32], uvs: &[Vec2], normals: &[Vec3]) -> Result<(), String> {
        self.data.add_face(
            ids.iter()
                .enumerate()
                .map(|(i, id)| Corner {
                    vertex: *id,
                    uv: uvs[i].to_array(),
                    normal: Some(normals[if normals.len() == 1 { 0 } else { i }].to_array()),
                })
                .collect(),
        )?;
        Ok(())
    }
}
fn cube(b: &mut Builder) -> Result<(), String> {
    let mut ids = Vec::new();
    for z in [-0.5, 0.5] {
        for y in [-0.5, 0.5] {
            for x in [-0.5, 0.5] {
                ids.push(b.vertex(Vec3::new(x, y, z))?);
            }
        }
    }
    for (fi, (indices, normal)) in [
        ([4, 5, 7, 6], Vec3::Z),
        ([5, 1, 3, 7], Vec3::X),
        ([1, 0, 2, 3], Vec3::NEG_Z),
        ([0, 4, 6, 2], Vec3::NEG_X),
        ([6, 7, 3, 2], Vec3::Y),
        ([0, 1, 5, 4], Vec3::NEG_Y),
    ]
    .into_iter()
    .enumerate()
    {
        let min = Vec2::new((fi % 3) as f32 / 3., (fi / 3) as f32 / 2.);
        let max = min + Vec2::new(1. / 3., 0.5);
        b.face(
            &indices.map(|i| ids[i]),
            &[Vec2::new(min.x, max.y), max, Vec2::new(max.x, min.y), min],
            &[normal],
        )?;
    }
    Ok(())
}
fn grid(b: &mut Builder, divisions: [u32; 2], horizontal: bool) -> Result<(), String> {
    let [columns, rows] = divisions;
    let mut ids = Vec::new();
    let mut uvs = Vec::new();
    for row in 0..=rows {
        for col in 0..=columns {
            let uv = Vec2::new(col as f32 / columns as f32, row as f32 / rows as f32);
            let p = if horizontal {
                Vec3::new(uv.x - 0.5, 0., uv.y - 0.5)
            } else {
                Vec3::new(uv.x - 0.5, 0.5 - uv.y, 0.)
            };
            ids.push(b.vertex(p)?);
            uvs.push(uv);
        }
    }
    for row in 0..rows {
        for col in 0..columns {
            let a = (row * (columns + 1) + col) as usize;
            let d = a + columns as usize + 1;
            let indices = [d, d + 1, a + 1, a];
            b.face(
                &indices.map(|i| ids[i]),
                &indices.map(|i| uvs[i]),
                &[if horizontal { Vec3::Y } else { Vec3::Z }],
            )?;
        }
    }
    Ok(())
}
fn circle(b: &mut Builder, segments: u32) -> Result<(), String> {
    let mut ids = Vec::new();
    let mut uv = Vec::new();
    for i in 0..segments {
        let angle = i as f32 / segments as f32 * TAU;
        let p = Vec3::new(angle.cos(), angle.sin(), 0.) * 0.5;
        ids.push(b.vertex(p)?);
        uv.push(Vec2::new(p.x + 0.5, 0.5 - p.y));
    }
    b.face(&ids, &uv, &[Vec3::Z])
}
fn sphere(b: &mut Builder, columns: u32, rows: u32) -> Result<(), String> {
    b.data.shading = Shading::Smooth;
    let top = b.vertex(Vec3::Y * 0.5)?;
    let bottom = b.vertex(Vec3::NEG_Y * 0.5)?;
    let mut rings = Vec::new();
    let mut normals = Vec::new();
    for row in 1..rows {
        let mut ring = Vec::new();
        let mut norm = Vec::new();
        for col in 0..columns {
            let latitude = row as f32 / rows as f32 * PI;
            let longitude = col as f32 / columns as f32 * TAU;
            let n = Vec3::new(
                latitude.sin() * longitude.sin(),
                latitude.cos(),
                latitude.sin() * longitude.cos(),
            );
            ring.push(b.vertex(n * 0.5)?);
            norm.push(n);
        }
        rings.push(ring);
        normals.push(norm);
    }
    let c = columns as usize;
    for col in 0..c {
        let next = (col + 1) % c;
        let u = col as f32 / columns as f32;
        let v = (col + 1) as f32 / columns as f32;
        b.face(
            &[top, rings[0][col], rings[0][next]],
            &[
                Vec2::new(v, 0.),
                Vec2::new(u, 1. / rows as f32),
                Vec2::new(v, 1. / rows as f32),
            ],
            &[Vec3::Y, normals[0][col], normals[0][next]],
        )?;
        for row in 0..rings.len() - 1 {
            let a = (row + 1) as f32 / rows as f32;
            let d = (row + 2) as f32 / rows as f32;
            b.face(
                &[
                    rings[row][next],
                    rings[row][col],
                    rings[row + 1][col],
                    rings[row + 1][next],
                ],
                &[
                    Vec2::new(v, a),
                    Vec2::new(u, a),
                    Vec2::new(u, d),
                    Vec2::new(v, d),
                ],
                &[
                    normals[row][next],
                    normals[row][col],
                    normals[row + 1][col],
                    normals[row + 1][next],
                ],
            )?;
        }
        let last = rings.len() - 1;
        b.face(
            &[rings[last][col], bottom, rings[last][next]],
            &[
                Vec2::new(u, 1. - 1. / rows as f32),
                Vec2::new(u, 1.),
                Vec2::new(v, 1. - 1. / rows as f32),
            ],
            &[normals[last][col], Vec3::NEG_Y, normals[last][next]],
        )?;
    }
    Ok(())
}
fn radial(
    b: &mut Builder,
    primitive: Primitive,
    columns: u32,
    p: Parameters,
) -> Result<(), String> {
    let pointed = matches!(primitive, Primitive::Cone | Primitive::Pyramid);
    let tube = primitive == Primitive::Tube;
    let rows = if primitive == Primitive::Pyramid {
        1
    } else {
        p.height_divisions
    };
    let c = columns as usize;
    let ring_normal = |i: usize| {
        let a = i as f32 / columns as f32 * TAU;
        Vec3::new(a.sin(), 0., a.cos())
    };
    let mut outer = Vec::new();
    let mut inner = Vec::new();
    for row in 0..=rows {
        let t = row as f32 / rows as f32;
        let radius = if pointed { t * 0.5 } else { 0.5 };
        if pointed && row == 0 {
            let apex = b.vertex(Vec3::Y * 0.5)?;
            outer.push(vec![apex; c]);
            continue;
        }
        let mut ring = Vec::new();
        let mut inside = Vec::new();
        for col in 0..c {
            let n = ring_normal(col);
            ring.push(b.vertex(n * radius + Vec3::Y * (0.5 - t))?);
            if tube {
                inside.push(b.vertex(n * (0.5 * (1. - p.wall_fraction)) + Vec3::Y * (0.5 - t))?);
            }
        }
        outer.push(ring);
        if tube {
            inner.push(inside);
        }
    }
    for row in 0..rows as usize {
        for col in 0..c {
            let next = (col + 1) % c;
            let u = col as f32 / columns as f32;
            let v = (col + 1) as f32 / columns as f32;
            let a = row as f32 / rows as f32 * 0.5;
            let d = (row + 1) as f32 / rows as f32 * 0.5;
            let n = |i| {
                if pointed {
                    (ring_normal(i) + Vec3::Y * 0.5).normalize()
                } else {
                    ring_normal(i)
                }
            };
            if pointed && row == 0 {
                let normal = (ring_normal(col) * 0.5 - Vec3::Y)
                    .cross(ring_normal(next) * 0.5 - Vec3::Y)
                    .normalize();
                let normals = if primitive == Primitive::Pyramid {
                    [normal; 3]
                } else {
                    [(n(col) + n(next)).normalize(), n(col), n(next)]
                };
                b.face(
                    &[outer[0][col], outer[1][col], outer[1][next]],
                    &[
                        Vec2::new((u + v) * 0.5, a),
                        Vec2::new(u, d),
                        Vec2::new(v, d),
                    ],
                    &normals,
                )?;
            } else {
                b.face(
                    &[
                        outer[row][next],
                        outer[row][col],
                        outer[row + 1][col],
                        outer[row + 1][next],
                    ],
                    &[
                        Vec2::new(v, a),
                        Vec2::new(u, a),
                        Vec2::new(u, d),
                        Vec2::new(v, d),
                    ],
                    &[n(next), n(col), n(col), n(next)],
                )?;
            }
            if tube {
                b.face(
                    &[
                        inner[row][col],
                        inner[row][next],
                        inner[row + 1][next],
                        inner[row + 1][col],
                    ],
                    &[
                        Vec2::new(u, 0.5 + a * 0.5),
                        Vec2::new(v, 0.5 + a * 0.5),
                        Vec2::new(v, 0.5 + d * 0.5),
                        Vec2::new(u, 0.5 + d * 0.5),
                    ],
                    &[-n(col), -n(next), -n(next), -n(col)],
                )?;
            }
        }
    }
    if tube {
        // The end rings are quads, leaving the hole genuinely open.
        for (row, normal, center) in [
            (0, Vec3::Y, Vec2::new(0.25, 0.875)),
            (rows as usize, Vec3::NEG_Y, Vec2::new(0.75, 0.875)),
        ] {
            for col in 0..c {
                let next = (col + 1) % c;
                let mut ids = [
                    outer[row][col],
                    outer[row][next],
                    inner[row][next],
                    inner[row][col],
                ];
                let uv = |i: usize, r: f32| {
                    let n = ring_normal(i);
                    center + Vec2::new(n.x * 0.24, n.z * 0.12) * r
                };
                let mut uvs = [
                    uv(col, 1.),
                    uv(next, 1.),
                    uv(next, 1. - p.wall_fraction),
                    uv(col, 1. - p.wall_fraction),
                ];
                if row > 0 {
                    ids.reverse();
                    uvs.reverse();
                }
                b.face(&ids, &uvs, &[normal])?;
            }
        }
    } else {
        for (row, sign, center_u) in [(0, 1., 0.25), (rows as usize, -1., 0.75)] {
            if pointed && row == 0 {
                continue;
            }
            let mut ids = outer[row].clone();
            let mut uvs: Vec<_> = (0..c)
                .map(|i| {
                    let n = ring_normal(i);
                    Vec2::new(center_u + n.x * 0.25, 0.75 + n.z * 0.25)
                })
                .collect();
            if sign < 0. {
                ids.reverse();
                uvs.reverse();
            }
            b.face(&ids, &uvs, &[Vec3::Y * sign])?;
        }
    }
    Ok(())
}
