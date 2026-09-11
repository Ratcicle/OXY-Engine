use super::*;
use rapier3d::parry::{bounding_volume::BoundingVolume, query};
#[derive(Clone, Debug)]
pub struct SweepSpan {
    pub object: Id,
    pub enter: f32,
    pub exit: f32,
    pub starts_inside: bool,
    pub ends_inside: bool,
}
impl PhysicsWorld {
    /// Conservative BVH candidates, exact shape tests. Fractions span one actual
    /// resolved segment; no hypothetical line through walls is introduced here.
    pub fn sweep_all(
        &self,
        shape: &CollisionShape,
        position: Vec3,
        delta: Vec3,
        options: &QueryOptions,
    ) -> Result<Vec<SweepSpan>, String> {
        if !position.is_finite() || !delta.is_finite() || !(position + delta).is_finite() {
            return Err("Varredura com posição inválida.".into());
        }
        let shape = shape.prepare(Vec3::ONE)?;
        let start = pose(position, Quat::IDENTITY);
        let end = pose(position + delta, Quat::IDENTITY);
        let bounds = shape.compute_aabb(&start).merged(&shape.compute_aabb(&end));
        let predicate = |h, _: &Collider| self.accepts(h, options);
        let pipeline = self.broad.as_query_pipeline(
            self.narrow.query_dispatcher(),
            &self.bodies,
            &self.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        self.queries.set(self.queries.get() + 1);
        let mut result = Vec::new();
        let cast_options = ShapeCastOptions {
            max_time_of_impact: 1.,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: true,
            ..Default::default()
        };
        for (handle, collider) in pipeline.intersect_aabb_conservative(bounds) {
            let starts_inside =
                query::intersection_test(&start, &*shape, collider.position(), collider.shape())
                    .map_err(|_| "Interseção de formas não suportada")?;
            let ends_inside =
                query::intersection_test(&end, &*shape, collider.position(), collider.shape())
                    .map_err(|_| "Interseção de formas não suportada")?;
            let forward = query::cast_shapes(
                &start,
                vector(delta),
                &*shape,
                collider.position(),
                Vector::ZERO,
                collider.shape(),
                cast_options,
            )
            .map_err(|_| "Varredura de formas não suportada")?;
            if forward.is_none() && !starts_inside && !ends_inside {
                continue;
            }
            let backward = if !ends_inside {
                query::cast_shapes(
                    &end,
                    vector(-delta),
                    &*shape,
                    collider.position(),
                    Vector::ZERO,
                    collider.shape(),
                    cast_options,
                )
                .map_err(|_| "Varredura de saída não suportada")?
            } else {
                None
            };
            result.push(SweepSpan {
                object: self.ids[&handle].clone(),
                enter: if starts_inside {
                    0.
                } else {
                    forward.map_or(1., |h| h.time_of_impact)
                },
                exit: if ends_inside {
                    1.
                } else {
                    backward.map_or(1., |h| 1. - h.time_of_impact)
                },
                starts_inside,
                ends_inside,
            });
        }
        result.sort_by(|a, b| {
            a.enter
                .total_cmp(&b.enter)
                .then_with(|| a.object.cmp(&b.object))
        });
        Ok(result)
    }
}
