//! Conservative 2D candidates. Ordering always follows the original vector.
//! Large boxes/queries and 3D use the bounded brute-force path.
use crate::collision::Aabb;
use glam::Vec3;
use std::collections::{BTreeSet, HashMap};
const CELL: f32 = 4.0;
const MAX_CELLS: i64 = 64;
pub struct Candidates {
    all: Vec<usize>,
    cells: Option<HashMap<(i32, i32), Vec<usize>>>,
    extra: BTreeSet<usize>,
}
fn cells(bounds: Aabb) -> Option<(i32, i32, i32, i32)> {
    let lo = (bounds.min / CELL).floor();
    let hi = (bounds.max / CELL).floor();
    if !lo.is_finite()
        || !hi.is_finite()
        || lo.abs().max_element() > 1e8
        || hi.abs().max_element() > 1e8
    {
        return None;
    }
    let (x0, y0, x1, y1) = (lo.x as i32, lo.y as i32, hi.x as i32, hi.y as i32);
    let count = (i64::from(x1) - i64::from(x0) + 1) * (i64::from(y1) - i64::from(y0) + 1);
    (count > 0 && count <= MAX_CELLS).then_some((x0, y0, x1, y1))
}
impl Candidates {
    pub fn new(boxes: impl IntoIterator<Item = (usize, Aabb)>, dimensions: usize) -> Self {
        let boxes: Vec<_> = boxes.into_iter().collect();
        let mut result = Self {
            all: boxes.iter().map(|(i, _)| *i).collect(),
            cells: None,
            extra: BTreeSet::new(),
        };
        if dimensions != 2 || boxes.len() <= 64 {
            return result;
        }
        let extent = boxes.iter().fold(boxes[0].1, |a, (_, b)| Aabb {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        });
        let span = (extent.max / CELL).floor() - (extent.min / CELL).floor() + Vec3::ONE;
        if span.x * span.y <= boxes.len() as f32 / 8. {
            return result;
        }
        let mut map: HashMap<_, Vec<_>> = HashMap::new();
        for (i, bounds) in boxes {
            if let Some((x0, y0, x1, y1)) = cells(bounds) {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        map.entry((x, y)).or_default().push(i);
                    }
                }
            } else {
                result.extra.insert(i);
            }
        }
        // A tiny occupied region is cheaper to traverse directly (dense scenes).
        if map.len() > 4 {
            result.cells = Some(map);
        }
        result
    }
    /// During this immutable structural phase, changed boxes become conservative
    /// extras. Stale cell entries can add work, but can never hide the new box.
    pub fn changed(&mut self, i: usize) {
        self.extra.insert(i);
    }
    pub fn query(&self, bounds: Aabb) -> Vec<usize> {
        let (Some(map), Some((x0, y0, x1, y1))) = (&self.cells, cells(bounds)) else {
            return self.all.clone();
        };
        let mut result = self.extra.clone();
        for x in x0..=x1 {
            for y in y0..=y1 {
                if let Some(indices) = map.get(&(x, y)) {
                    result.extend(indices);
                }
            }
        }
        result.into_iter().collect()
    }
    pub fn motion(
        &self,
        body: Aabb,
        delta: Vec3,
        mut overlapping: impl FnMut(usize) -> bool,
    ) -> Vec<usize> {
        if self.cells.is_none() {
            return self.all.clone();
        }
        let end = body.translated(delta);
        let swept = Aabb {
            min: body.min.min(end.min) - Vec3::splat(0.0001),
            max: body.max.max(end.max) + Vec3::splat(0.0001),
        };
        let candidates = self.query(swept);
        // Sequential penetration recovery can push beyond the swept region.
        // Preserve the original ordered recovery, including cascading overlaps.
        if candidates.iter().any(|i| overlapping(*i)) {
            self.all.clone()
        } else {
            candidates
        }
    }
}
