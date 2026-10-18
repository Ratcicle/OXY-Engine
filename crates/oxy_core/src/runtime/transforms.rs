//! Moving, turning and scaling any object from logic (Mover / girar / escalar objeto), and
//! reading where it stands.
use super::*;
use glam::{EulerRot, Mat4, Quat};
use std::f32::consts::TAU;

/// The part of a transform a write changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformPart {
    /// In metres.
    Position,
    /// XYZ angles in degrees.
    Rotation,
    Scale,
}
/// The axes a transform is read or written in: the world's, or the parent's, as Properties
/// shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformSpace {
    World,
    Local,
}

impl Runtime {
    /// Where `id` stands and how it is turned, in `space`. In the world, where its pivot is, as
    /// Properties shows it and `write_transform` places it, or a 3D character's origin, where
    /// Teleportar com segurança puts it; locally, its own transform.
    pub fn object_pose(&self, id: &str, space: TransformSpace) -> Result<(Vec3, Quat), String> {
        let e = self.entity(id).ok_or("Objeto ausente")?;
        Ok(match space {
            TransformSpace::Local => (Vec3::from(e.transform.position), e.transform.quaternion()),
            TransformSpace::World => {
                let world = self.scene().world_matrix(id)?;
                let (origin, rotation, _) = crate::physics3d::world_pose(world)?;
                let position = if e.character3d.is_some() {
                    origin
                } else {
                    world.transform_point3(Vec3::from(e.transform.pivot))
                };
                (position, rotation)
            }
        })
    }
    /// How `id` is turned in the world.
    pub fn world_rotation(&self, id: &str) -> Result<Quat, String> {
        let (_, rotation, _) = crate::physics3d::world_pose(self.scene().world_matrix(id)?)?;
        Ok(rotation)
    }
    /// Writes one part of `id`'s transform, as Mover / girar / escalar objeto does, in `space`:
    /// sets it, or with `add` adds `value` to it (scale: multiplies it). A world position is
    /// where the pivot is, as Properties and the agents read it. A 3D character's root and an
    /// object a playing clip poses are refused, since their own motion would undo the write.
    /// `jump` makes the move a jump for the platforms in the moved hierarchy, which drop
    /// whoever rides them without launching them.
    pub fn write_transform(
        &mut self,
        id: &str,
        part: TransformPart,
        space: TransformSpace,
        value: Vec3,
        add: bool,
        jump: bool,
    ) -> Result<(), String> {
        let e = self.writable_transform(id, value)?;
        let name = e.name.clone();
        let world = space == TransformSpace::World;
        // Local writes keep to the object's own transform.
        let parent = match e.parent.as_deref().filter(|_| world) {
            Some(parent) => self.scene().world_matrix(parent)?,
            None => Mat4::IDENTITY,
        };
        let mut transform = e.transform.clone();
        match part {
            TransformPart::Position => {
                let position = Vec3::from(transform.position);
                let moved = match (world, add) {
                    (false, false) => value,
                    (false, true) => position + value,
                    (true, _) if !parent.is_finite() || parent.determinant().abs() < 1e-8 => {
                        return Err(format!(
                            "A escala de um grupo acima de {name} é zero; use o espaço Local"
                        ));
                    }
                    // The pivot's world position plus the value, without a trip through the
                    // world and back.
                    (true, true) => position + parent.inverse().transform_vector3(value),
                    (true, false) => parent.inverse().transform_point3(value),
                };
                transform.position = moved.to_array();
            }
            TransformPart::Rotation if !world => {
                let radians = value.to_array().map(f32::to_radians);
                transform.rotation = if add {
                    // Angle by angle, as Properties shows them: exact for a turn about one
                    // axis, with no drift from a trip through a quaternion. Past a full turn an
                    // angle comes back by whole turns.
                    std::array::from_fn(|i| {
                        let angle = transform.rotation[i] + radians[i];
                        if angle.abs() > TAU {
                            angle.rem_euclid(TAU)
                        } else {
                            angle
                        }
                    })
                } else {
                    radians
                };
            }
            TransformPart::Rotation => {
                let refused = |reason: &str| {
                    format!("{name} não pode girar no espaço Mundo: {reason}; use o espaço Local")
                };
                let (_, parent_rotation, parent_scale) = crate::physics3d::world_pose(parent)
                    .map_err(|_| refused("um grupo acima dele tem escala zero ou deformação"))?;
                // A mirrored group (glam keeps the mirror on X) turns its children the other
                // way about Y and Z. The object's own mirror stays in its scale.
                let mirrored = |q: Quat| {
                    if parent_scale.x < 0. {
                        Quat::from_xyzw(q.x, -q.y, -q.z, q.w)
                    } else {
                        q
                    }
                };
                let [x, y, z] = value.to_array().map(f32::to_radians);
                let mut rotation = Quat::from_euler(EulerRot::XYZ, x, y, z);
                if add {
                    rotation = rotation * parent_rotation * mirrored(transform.quaternion());
                }
                let local = mirrored(parent_rotation.inverse() * rotation);
                // The angles nearest the current ones, so that a turn goes on smoothly.
                transform.rotation = crate::editing::angles_near(local, transform.rotation);
                // Under a group scaled unevenly, most turns would shear the object; without
                // shear, the object's axes lie on the rotation's.
                if sheared(parent * transform.matrix()) {
                    return Err(refused(
                        "a escala não uniforme de um grupo acima dele o deformaria",
                    ));
                }
            }
            TransformPart::Scale => {
                if world {
                    return Err(format!(
                        "A escala de {name} só muda no espaço Local, relativa ao pai"
                    ));
                }
                let scale = if add {
                    Vec3::from(transform.scale) * value
                } else {
                    value
                };
                if scale.abs().min_element() <= 0.00001 {
                    return Err(format!(
                        "A escala de {name} chegaria a zero: cada eixo precisa ficar acima de \
                         0,00001 (em valor absoluto)"
                    ));
                }
                transform.scale = scale.to_array();
            }
        }
        if !transform.finite() {
            return Err(format!(
                "A nova transformação de {name} sai do intervalo suportado"
            ));
        }
        self.entity_mut(id).unwrap().transform = transform;
        self.physics_dirty = true;
        if jump {
            let moved = self
                .index()
                .map(|index| index.descendants(self.scene(), id))
                .unwrap_or_default();
            self.characters.looped.extend(moved);
        }
        Ok(())
    }
    /// `write_transform` with the part, the mode and the space a node names: position, rotation
    /// or scale; set or add; world or local. One it does not know is refused where the write
    /// used to read it: after the checks of the object and the value, the mode, then the space,
    /// then the part.
    pub(in crate::runtime) fn write_named_transform(
        &mut self,
        id: &str,
        part: &str,
        mode: &str,
        space: &str,
        value: Vec3,
        jump: bool,
    ) -> Result<(), String> {
        let add = match mode {
            "set" => Some(false),
            "add" => Some(true),
            _ => None,
        };
        let space = match space {
            "world" => Some(TransformSpace::World),
            "local" => Some(TransformSpace::Local),
            _ => None,
        };
        let part = match part {
            "position" => Some(TransformPart::Position),
            "rotation" => Some(TransformPart::Rotation),
            "scale" => Some(TransformPart::Scale),
            _ => None,
        };
        if let (Some(part), Some(space), Some(add)) = (part, space, add) {
            return self.write_transform(id, part, space, value, add, jump);
        }
        let e = self.writable_transform(id, value)?;
        add.ok_or("Como deve ser Definir ou Somar")?;
        let space = space.ok_or("Espaço deve ser Mundo ou Local")?;
        if let Some(parent) = e.parent.as_deref()
            && space == TransformSpace::World
        {
            self.scene().world_matrix(parent)?;
        }
        Err("O que muda deve ser Posição, Rotação ou Escala".into())
    }
    /// The object `id`, when a write of `value` to its transform may go on: it exists, logic
    /// may move it, and `value` is finite.
    fn writable_transform(
        &self,
        id: &str,
        value: Vec3,
    ) -> Result<&crate::document::Entity, String> {
        let e = self.entity(id).ok_or("Objeto ausente")?;
        let name = &e.name;
        if e.character3d.is_some() {
            return Err(format!(
                "{name} é um personagem 3D: mova-o com Teleportar com segurança e gire-o com \
                 Definir giro horizontal do personagem"
            ));
        }
        if let Some(clip) = self.posing_clip(id) {
            return Err(format!(
                "{name} é movido pela animação {clip}, que está tocando: pause-a com Ativar / \
                 desativar componente (Reprodução da animação) ou mova um grupo acima dele"
            ));
        }
        if !value.is_finite() {
            return Err("Valor precisa de números finitos".into());
        }
        Ok(e)
    }
    /// Moves and turns `id` so that its origin stands at `position`, turned by `rotation`, in
    /// the world, as Mover / girar / escalar objeto would; the scale stays. All or nothing.
    pub(in crate::runtime) fn place_at(
        &mut self,
        id: &str,
        position: Vec3,
        rotation: Quat,
    ) -> Result<(), String> {
        let before = self.entity(id).ok_or("Objeto ausente")?.transform.clone();
        let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
        let degrees = Vec3::new(x, y, z).to_array().map(f32::to_degrees);
        let placed = self
            .write_transform(
                id,
                TransformPart::Rotation,
                TransformSpace::World,
                degrees.into(),
                false,
                false,
            )
            .and_then(|()| {
                // The pivot, which the write places, lies off the origin by the turned pivot.
                let entity = self.entity(id).ok_or("Objeto ausente")?;
                let pivot = Vec3::from(entity.transform.pivot);
                let offset = self.scene().world_matrix(id)?.transform_vector3(pivot);
                self.write_transform(
                    id,
                    TransformPart::Position,
                    TransformSpace::World,
                    position + offset,
                    false,
                    false,
                )
            });
        if placed.is_err() {
            self.entity_mut(id).unwrap().transform = before;
        }
        placed
    }
    /// The name of the playing clip that poses `id` (see `advance_animations`), which would
    /// undo a write to it: the clip of the first owner in scene order when several do.
    fn posing_clip(&self, id: &str) -> Option<String> {
        if !self.entity(id).is_some_and(animated) {
            return None;
        }
        let index = self.index()?;
        self.animations
            .iter()
            .filter(|(_, player)| player.playing)
            .filter_map(|(owner, player)| {
                let clip = self
                    .entity(owner)?
                    .clips
                    .iter()
                    .find(|clip| clip.id == player.clip_id)?;
                clip.tracks
                    .iter()
                    .any(|track| track.target == id)
                    .then(|| (index.position(owner), clip.name.clone()))
            })
            .min()
            .map(|(_, name)| name)
    }
}

/// Whether the axes of `matrix` leave right angles, whatever their lengths and signs.
fn sheared(matrix: Mat4) -> bool {
    let [x, y, z] = [matrix.x_axis, matrix.y_axis, matrix.z_axis]
        .map(|axis| axis.truncate().normalize_or_zero());
    x.dot(y).abs().max(y.dot(z).abs()).max(z.dot(x).abs()) > 1e-4
}
