//! CPU acceleration shared by surface picking and component visibility. Rebuilt on geometry edit only.
use glam::Vec3;
#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
}
#[derive(Clone, Debug)]
struct Node {
    min: Vec3,
    max: Vec3,
    start: usize,
    end: usize,
    children: Option<[usize; 2]>,
}
impl Bvh {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn estimated_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>()
            + self.order.capacity() * std::mem::size_of::<usize>()
    }
    pub fn build(points: &[[Vec3; 3]]) -> Self {
        let mut b = Self {
            nodes: Vec::new(),
            order: (0..points.len()).collect(),
        };
        if !points.is_empty() {
            b.node(points, 0, points.len());
        }
        b
    }
    fn node(&mut self, points: &[[Vec3; 3]], start: usize, end: usize) -> usize {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &i in &self.order[start..end] {
            for p in points[i] {
                min = min.min(p);
                max = max.max(p);
            }
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            min,
            max,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let axis = (max - min).max_position();
            let middle = (start + end) / 2;
            self.order[start..end].select_nth_unstable_by(middle - start, |a, b| {
                let center = |i: usize| points[i].iter().map(|v| v[axis]).sum::<f32>();
                center(*a).total_cmp(&center(*b)).then(a.cmp(b))
            });
            let left = self.node(points, start, middle);
            let right = self.node(points, middle, end);
            self.nodes[index].children = Some([left, right]);
        }
        index
    }
    pub fn hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        points: impl Fn(usize) -> [Vec3; 3],
    ) -> Option<(usize, f32, [f32; 3])> {
        if self.nodes.is_empty() || !origin.is_finite() || !direction.is_finite() {
            return None;
        }
        let mut stack = vec![0];
        let mut best: Option<(usize, f32, [f32; 3])> = None;
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index];
            let limit = best.map_or(f32::INFINITY, |(_, d, _)| d);
            if !box_hit(origin, direction, node.min, node.max, limit) {
                continue;
            }
            if let Some([left, right]) = node.children {
                stack.push(right);
                stack.push(left);
            } else {
                for &i in &self.order[node.start..node.end] {
                    if let Some((d, bary)) = triangle(origin, direction, points(i))
                        && best.is_none_or(|(old, t, _)| d < t || (d == t && i < old))
                    {
                        best = Some((i, d, bary));
                    }
                }
            }
        }
        best
    }
}
fn box_hit(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3, mut far: f32) -> bool {
    let mut near = 0f32;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-20 {
            if origin[axis] < min[axis] - 1e-6 || origin[axis] > max[axis] + 1e-6 {
                return false;
            }
            continue;
        }
        let a = (min[axis] - origin[axis]) / direction[axis];
        let b = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if far + 1e-6 < near {
            return false;
        }
    }
    true
}
pub fn triangle(origin: Vec3, direction: Vec3, points: [Vec3; 3]) -> Option<(f32, [f32; 3])> {
    let [a, b, c] = points;
    let e1 = b - a;
    let e2 = c - a;
    let cross = direction.cross(e2);
    let det = e1.dot(cross);
    let tolerance = 1e-7 * e1.length() * e2.length() * direction.length();
    if det.abs() <= tolerance {
        return None;
    }
    let inv = det.recip();
    let offset = origin - a;
    let u = offset.dot(cross) * inv;
    if !(-1e-6..=1. + 1e-6).contains(&u) {
        return None;
    }
    let q = offset.cross(e1);
    let v = direction.dot(q) * inv;
    if v < -1e-6 || u + v > 1. + 1e-6 {
        return None;
    }
    let d = e2.dot(q) * inv;
    (d >= 0.).then_some((d, [1. - u - v, u, v]))
}
