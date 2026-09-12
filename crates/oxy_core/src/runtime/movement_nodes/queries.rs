use super::*;
use crate::physics3d::{CollisionShape, QueryHit, QueryOptions};
#[derive(Clone, Debug)]
pub enum SceneQueryShape {
    Ray,
    Sphere { radius: f32 },
    Capsule { height: f32, radius: f32 },
    Space { height: f32, radius: f32 },
}
#[derive(Clone, Debug)]
pub struct SceneQuery {
    pub shape: SceneQueryShape,
    pub origin: Vec3,
    pub direction: Vec3,
    pub distance: f32,
    pub include_sensors: bool,
    pub exclude: Option<Id>,
    pub category: u32,
    pub mask: u32,
}
impl Runtime {
    /// Budget is per fixed step. Prepared collider shapes/BVH remain shared;
    /// structural/physical mutations refresh this view before the next query.
    pub fn query_scene(&mut self, query: &SceneQuery) -> Result<Option<QueryHit>, String> {
        const LIMIT: usize = 256;
        if self.query_work >= LIMIT {
            return Err("Limite de 256 consultas explícitas por passo excedido.".into());
        }
        self.query_work += 1;
        if !query.origin.is_finite()
            || !query.direction.is_finite()
            || !query.distance.is_finite()
            || query.distance < 0.
        {
            return Err("Origem, direção e distância da consulta devem ser finitas; distância não pode ser negativa.".into());
        }
        self.refresh_character_queries()?;
        let mut options = QueryOptions {
            include_sensors: query.include_sensors,
            category: query.category,
            mask: query.mask,
            ignore_roles: true,
            ..Default::default()
        };
        if let Some(id) = &query.exclude {
            if self.entity(id).is_none() {
                return Err("Objeto excluído da consulta está ausente.".into());
            }
            options.exclude.extend(self.scene().descendants(id));
        }
        let world = self.characters.world.as_ref().unwrap();
        if let SceneQueryShape::Space { height, radius } = query.shape {
            let contacts = world.penetration_contacts(
                &CollisionShape::Capsule { height, radius },
                query.origin,
                &options,
            )?;
            return Ok(contacts.into_iter().next());
        }
        let direction = query
            .direction
            .as_dvec3()
            .try_normalize()
            .ok_or("Direção da consulta não pode ser zero.")?
            .as_vec3();
        match query.shape {
            SceneQueryShape::Ray => world.ray(query.origin, direction, query.distance, &options),
            SceneQueryShape::Sphere { radius } => world.cast(
                &CollisionShape::Sphere { radius },
                query.origin,
                direction * query.distance,
                &options,
            ),
            SceneQueryShape::Capsule { height, radius } => world.cast(
                &CollisionShape::Capsule { height, radius },
                query.origin,
                direction * query.distance,
                &options,
            ),
            SceneQueryShape::Space { .. } => unreachable!(),
        }
    }
    pub fn apply_surface(&mut self, target: &str, surface: Option<Id>) -> Result<(), String> {
        if surface
            .as_ref()
            .is_some_and(|id| !self.project.surfaces.iter().any(|s| &s.id == id))
        {
            return Err("Superfície física ausente.".into());
        }
        let entity = self.entity_mut(target).ok_or("Objeto ausente")?;
        if let Some(config) = &mut entity.character3d {
            config.body.surface = surface;
        } else {
            entity
                .physics3d
                .as_mut()
                .ok_or("Objeto não possui forma física 3D")?
                .surface = surface;
        }
        self.physics_dirty = true;
        Ok(())
    }
}
