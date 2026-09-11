use super::*;
#[derive(Clone, Debug)]
pub struct SensorCrossing {
    pub area: Id,
    pub object: Id,
    pub entered: bool,
    /// Ordered fraction in the list of resolved segments, not seconds.
    pub path_time: f32,
}
fn active_sensor(entity: &crate::document::Entity) -> bool {
    entity
        .physics3d
        .as_ref()
        .is_some_and(|c| c.enabled && c.sensor)
        || entity
            .collider
            .as_ref()
            .is_some_and(|c| c.enabled && c.is_trigger)
}
impl Characters {
    pub(in crate::runtime) fn remove_entities(
        &mut self,
        ids: &HashSet<Id>,
        scene: &Scene,
        time: f64,
    ) {
        self.states.retain(|id, _| !ids.contains(id));
        for (id, state) in &mut self.states {
            if state
                .support
                .as_ref()
                .is_some_and(|s| ids.contains(&s.object))
            {
                let config = scene.entity(id).and_then(|e| e.character3d.as_ref());
                let support = state.support.take().unwrap();
                state.velocity += motor::inherited(
                    support.velocity,
                    config.map_or(InheritPlatform::None, |c| c.inherit_platform),
                );
                state.grounded = false;
                state.coyote_until = time + config.map_or(0., |c| f64::from(c.coyote_ms) * 0.001);
                self.pending_records.push(MovementRecord {
                    object: id.clone(),
                    event: MovementEvent::LeftSupport,
                    state: Arc::new(state.clone()),
                });
            }
        }
        self.paths.retain(|id, _| !ids.contains(id));
        self.overrides.retain(|id, _| !ids.contains(id));
        self.platforms.retain(|id, _| !ids.contains(id));
        self.intents.retain(|id, _| !ids.contains(id));
        self.jumps.retain(|id| !ids.contains(id));
        self.crouches.retain(|id, _| !ids.contains(id));
        self.sprints.retain(|id, _| !ids.contains(id));
        self.slides.retain(|id| !ids.contains(id));
        self.sensor_pairs
            .retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
        self.lateral
            .retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
        self.sensor_events
            .retain(|e| !ids.contains(&e.area) && !ids.contains(&e.object));
        self.events.retain(|(id, _)| !ids.contains(id));
        self.records.retain(|record| !ids.contains(&record.object));
        self.pending_records
            .retain(|record| !ids.contains(&record.object));
        self.reported.retain(|id, _| !ids.contains(id));
        if let Some(world) = &mut self.world {
            for id in ids {
                world.remove(id);
            }
            world.flush();
        }
    }
    fn detect_sensors(&mut self, scene: &Scene) -> Result<(), String> {
        self.sensor_events.clear();
        let Some(world) = &self.world else {
            self.sensor_pairs.clear();
            return Ok(());
        };
        if !scene.entities.iter().any(active_sensor) {
            self.sensor_pairs.clear();
            return Ok(());
        }
        let index = SceneIndex::new(scene)?;
        let mut next = HashSet::new();
        for entity in &scene.entities {
            let Some(state) = self.states.get(&entity.id) else {
                continue;
            };
            let collider = entity.physics3d.as_ref().ok_or("Personagem sem forma")?;
            if !collider.enabled {
                continue;
            }
            let crate::physics3d::CollisionShape::Capsule { radius, .. } = collider.shape else {
                continue;
            };
            let shape = crate::physics3d::CollisionShape::Capsule {
                height: state.height,
                radius: radius * state.uniform_scale,
            };
            let mut options = QueryOptions {
                category: collider.filter.category,
                mask: collider.filter.mask,
                include_sensors: true,
                only_sensors: true,
                ..QueryOptions::excluding(&entity.id)
            };
            options.exclude.extend(index.descendants(scene, &entity.id));
            let empty_path = [(state.position, state.position)];
            let path = self
                .paths
                .get(&entity.id)
                .filter(|p| !p.is_empty())
                .map_or(empty_path.as_slice(), Vec::as_slice);
            let center = Vec3::Y * (state.height * 0.5);
            let mut spans: BTreeMap<Id, (f32, f32)> = BTreeMap::new();
            for (i, (from, to)) in path.iter().enumerate() {
                for span in world.sweep_all(&shape, *from + center, *to - *from, &options)? {
                    if index.related(scene, &span.object, &entity.id) {
                        continue;
                    }
                    let a = i as f32 + span.enter;
                    let b = i as f32 + span.exit;
                    spans
                        .entry(span.object)
                        .and_modify(|(first, last)| {
                            *first = first.min(a);
                            *last = last.max(b);
                        })
                        .or_insert((a, b));
                }
            }
            let final_inside: HashSet<_> = world
                .overlaps(&shape, state.position + center, &options)?
                .into_iter()
                .filter(|id| !index.related(scene, id, &entity.id))
                .collect();
            for area in &final_inside {
                spans
                    .entry(area.clone())
                    .or_insert((path.len() as f32, path.len() as f32));
            }
            for (area, other) in &self.sensor_pairs {
                if other == &entity.id
                    && index
                        .position(area)
                        .is_some_and(|i| active_sensor(&scene.entities[i]))
                {
                    spans.entry(area.clone()).or_insert((0., 0.));
                }
            }
            let mut events = Vec::new();
            for (area, (enter, exit)) in spans {
                let pair = (area.clone(), entity.id.clone());
                let previous = self.sensor_pairs.contains(&pair);
                let inside = final_inside.contains(&area);
                if !previous {
                    events.push(SensorCrossing {
                        area: area.clone(),
                        object: entity.id.clone(),
                        entered: true,
                        path_time: enter,
                    });
                }
                if !inside {
                    events.push(SensorCrossing {
                        area,
                        object: entity.id.clone(),
                        entered: false,
                        path_time: exit.max(enter),
                    });
                } else {
                    next.insert(pair);
                }
            }
            events.sort_by(|a, b| {
                a.path_time
                    .total_cmp(&b.path_time)
                    .then_with(|| index.position(&a.area).cmp(&index.position(&b.area)))
                    .then_with(|| b.entered.cmp(&a.entered))
            });
            self.sensor_events.extend(events);
        }
        self.sensor_pairs = next;
        Ok(())
    }
}
impl Runtime {
    pub(in crate::runtime) fn detect_character_sensors(&mut self) {
        let mut characters = std::mem::take(&mut self.characters);
        if let Err(error) = characters.detect_sensors(self.scene()) {
            self.log(format!("Áreas 3D: {error}"));
        }
        let crossings = characters.sensor_events.clone();
        self.characters = characters;
        for crossing in &crossings {
            let activation = self
                .area_activations
                .get(&crossing.area)
                .copied()
                .unwrap_or_else(|| self.activate_area(&crossing.area));
            self.emit(if crossing.entered {
                RuntimeEvent::AreaEnter {
                    area: crossing.area.clone(),
                    other: crossing.object.clone(),
                    activation,
                }
            } else {
                RuntimeEvent::AreaExit {
                    area: crossing.area.clone(),
                    other: crossing.object.clone(),
                    activation,
                }
            });
        }
    }
}
