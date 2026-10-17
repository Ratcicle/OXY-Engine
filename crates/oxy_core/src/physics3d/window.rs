//! Parry's GJK builds its simplices from support points of whole shapes. Against a box, hull
//! or triangle larger than the other shape, those are far corners, and the simplex loses the
//! precision of the millimetres at the contact. Where the other shape's lowest point lies on
//! the vertical plane through a face diagonal (always over the middle of the face), the
//! simplex flattens into that plane and stops converging above GJK's fixed tolerance; its
//! fallback then advances the cast along a plane that does not bound the shapes. A cast
//! starting within or at its target distance misses, or stops millimetres to centimetres too
//! deep with a sideways normal, and Rapier's character controller walks the body into the
//! floor. Queries here see such a polytope cut to a window around the other shape's reach, in
//! coordinates near the contact: the same surface there, without the far corners, and with the
//! other shape off the diagonals of the cut's faces, which flatten a simplex of near corners
//! too.
use super::*;
use rapier3d::parry::{
    bounding_volume::BoundingVolume,
    query::{
        ClosestPoints, Contact, DefaultQueryDispatcher, NonlinearRigidMotion, QueryDispatcher,
        ShapeCastHit, ShapeCastStatus, Unsupported, details,
    },
    shape::{Capsule, Cuboid, SupportMap},
};

/// Clip planes stay this far, plus the query's own margin, from anywhere the other shape can
/// reach, so what it touches, and the shortest way out of an overlap shallower than that, are
/// those of the whole shape.
const ROOM: Real = 0.05;

/// The margin for cutting again around an overlap `dist` deep (negative), whose way out found
/// on the cut may cross a clip plane: twice that, so a few cuts reach the real way out.
fn deeper(dist: Real) -> Real {
    -2. * dist + ROOM
}

/// Parry's queries on the parts of large polytopes near the other shape.
pub(super) struct Windowed;

/// A polytope cut to a window, in coordinates centred on the cut.
enum Cut {
    Box(Cuboid),
    Points(Vec<Vector>),
}
impl SupportMap for Cut {
    fn local_support_point(&self, dir: Vector) -> Vector {
        match self {
            Self::Box(cuboid) => cuboid.local_support_point(dir),
            Self::Points(points) => points
                .iter()
                .copied()
                .reduce(|best, p| if p.dot(dir) > best.dot(dir) { p } else { best })
                .unwrap(),
        }
    }
}

/// How far the window reaches below and above the other shape's reach on each axis, as shares
/// of the reach. A cut centred on the other shape would put its middle on the diagonals of the
/// cut's faces, where GJK's simplex is as degenerate as over the middle of the whole face;
/// these put it at fractions 0.4, 0.56 and 0.55 of the cut.
const BELOW: Vector = Vector::new(0.1, 0.2, 0.3);
const ABOVE: Vector = Vector::new(0.4, 0.05, 0.15);
/// Where the shape ends inside the window, the least difference kept between the middle's
/// distances from the middle of the cut on two axes, as fractions of the cut: equal distances
/// put it on a diagonal.
const APART: Real = 0.04;

/// One axis of a window: the shape's extent, the reach within it and the reach's middle.
struct Axis {
    low: Real,
    high: Real,
    lo: Real,
    hi: Real,
    middle: Real,
}
impl Axis {
    fn at(&self, lo: Real, hi: Real) -> Real {
        (self.middle - lo) / (hi - lo)
    }
    /// The fractions the middle can sit at, the cut reaching past the reach or ending with the
    /// shape; none when the middle is off the shape on this axis.
    fn span(&self) -> Option<(Real, Real)> {
        (self.low < self.middle && self.middle < self.high)
            .then(|| (self.at(self.lo, self.high), self.at(self.low, self.hi)))
    }
    /// The shortest cut with the middle at fraction `t` of it.
    fn range(&self, t: Real) -> (Real, Real) {
        let hi = self.lo + (self.middle - self.lo) / t;
        if hi >= self.hi {
            (self.lo, hi.min(self.high))
        } else {
            let lo = self.hi - (self.hi - self.middle) / (1. - t);
            (lo.max(self.low), self.hi)
        }
    }
}

/// A fraction within `least..=most` as far from the middle as `wanted` (on its side first), or
/// the nearest one at least `APART` from each distance in `taken`, or else the one farthest
/// from them.
fn fraction(least: Real, most: Real, wanted: Real, taken: &[Real]) -> Real {
    let off = |t: Real| (t - 0.5).abs();
    let clash = |t: Real| {
        taken
            .iter()
            .map(|&u| (off(t) - u).abs())
            .fold(Real::MAX, Real::min)
    };
    let rank = |t: Real| {
        let clash = clash(t);
        if clash >= APART * 0.999 {
            (false, (off(t) - off(wanted)).abs())
        } else {
            (true, -clash)
        }
    };
    let edges = taken
        .iter()
        .flat_map(|&u| [u - APART, u + APART])
        .filter(|&u| u >= 0.)
        .flat_map(|u| [0.5 - u, 0.5 + u]);
    [wanted, 1. - wanted]
        .into_iter()
        .chain(edges)
        .chain([least, most])
        .filter(|t| (least..=most).contains(t))
        .min_by(|&a, &b| {
            let (a, b) = (rank(a), rank(b));
            a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
        })
        .unwrap_or(least)
}

/// The window for cutting a shape with local `bounds`: what `other` (posed at `pose`) covers
/// while moving by `motion`, grown by `margin`, then by uneven amounts on each side. On an axis
/// where the shape ends inside it, the cut ends there too and the middle moves within it: there
/// the window is placed again so the middle stays off the diagonals.
fn window(
    other: &dyn Shape,
    pose: &Pose,
    motion: Vector,
    margin: Real,
    bounds: &Aabb,
) -> Option<Aabb> {
    let start = other.compute_aabb(pose);
    let end = Aabb::new(start.mins + motion, start.maxs + motion);
    let reach = start.merged(&end).loosened(margin);
    let size = reach.maxs - reach.mins;
    let mut window = Aabb::new(reach.mins - size * BELOW, reach.maxs + size * ABOVE);
    if !(window.mins.is_finite() && window.maxs.is_finite()) {
        return None;
    }
    let middle = reach.center();
    let axes = [0, 1, 2].map(|i| Axis {
        low: bounds.mins[i],
        high: bounds.maxs[i],
        lo: reach.mins[i].max(bounds.mins[i]),
        hi: reach.maxs[i].min(bounds.maxs[i]),
        middle: middle[i],
    });
    let spans = axes.each_ref().map(Axis::span);
    let wanted = |i: usize| (0.5 + BELOW[i]) / (1. + BELOW[i] + ABOVE[i]);
    let ends = |i: usize| window.mins[i] < bounds.mins[i] || window.maxs[i] > bounds.maxs[i];
    let mut taken: Vec<_> = (0..3)
        .filter(|&i| spans[i].is_some() && !ends(i))
        .map(|i| (wanted(i) - 0.5).abs())
        .collect();
    let mut moved: Vec<_> = (0..3)
        .filter(|&i| ends(i))
        .filter_map(|i| spans[i].map(|(least, most)| (i, least, most)))
        .collect();
    // The axes with the fewest fractions to choose from go first.
    moved.sort_by(|a, b| (a.2 - a.1).total_cmp(&(b.2 - b.1)));
    for (i, least, most) in moved {
        let t = fraction(least, most, wanted(i), &taken);
        taken.push((t - 0.5).abs());
        (window.mins[i], window.maxs[i]) = axes[i].range(t);
    }
    Some(window)
}

/// The part of convex polygon `points` inside `window`.
fn clip(mut points: Vec<Vector>, window: &Aabb) -> Vec<Vector> {
    let mut clipped = Vec::with_capacity(points.len() + 6);
    for axis in 0..3 {
        for (bound, side) in [(window.mins[axis], -1.), (window.maxs[axis], 1.)] {
            clipped.clear();
            for (i, &a) in points.iter().enumerate() {
                let b = points[(i + 1) % points.len()];
                let (da, db) = (side * (a[axis] - bound), side * (b[axis] - bound));
                if da <= 0. {
                    clipped.push(a);
                }
                if (da <= 0.) != (db <= 0.) {
                    let mut crossing = a + (b - a) * (da / (da - db));
                    crossing[axis] = bound;
                    clipped.push(crossing);
                }
            }
            std::mem::swap(&mut points, &mut clipped);
        }
    }
    points
}

/// `shape` cut to `window` (its own space), with the cut's centre there, when the window
/// leaves part of it out; an empty cut when nothing of it is inside.
fn cut(shape: &dyn Shape, window: &Aabb) -> Option<Option<(Cut, Vector)>> {
    if let Some(cuboid) = shape.as_cuboid() {
        let half = cuboid.half_extents;
        let (low, high) = (window.mins.max(-half), window.maxs.min(half));
        if low == -half && high == half {
            return None;
        }
        if low.cmpgt(high).any() {
            return Some(None);
        }
        let centre = (low + high) * 0.5;
        return Some(Some((Cut::Box(Cuboid::new((high - low) * 0.5)), centre)));
    }
    let bounds = shape.compute_local_aabb();
    if window.contains(&bounds) {
        return None;
    }
    let mut points = if let Some(triangle) = shape.as_triangle() {
        clip(triangle.vertices().to_vec(), window)
    } else if let Some(hull) = shape.as_convex_polyhedron() {
        // Vertices of the cut: the faces' parts in the window and the window's corners inside.
        let ids = hull.vertices_adj_to_face();
        let faces: Vec<(Vector, Vec<Vector>)> = hull
            .faces()
            .iter()
            .map(|face| {
                let first = face.first_vertex_or_edge as usize;
                let polygon = ids[first..first + face.num_vertices_or_edges as usize]
                    .iter()
                    .map(|&i| hull.points()[i as usize])
                    .collect();
                (face.normal, polygon)
            })
            .collect();
        let inside: Vec<_> = window
            .vertices()
            .into_iter()
            .filter(|&p| faces.iter().all(|(n, polygon)| n.dot(p - polygon[0]) <= 0.))
            .collect();
        let mut points: Vec<_> = faces
            .into_iter()
            .flat_map(|(_, polygon)| clip(polygon, window))
            .collect();
        points.extend(inside);
        points
    } else {
        return None;
    };
    if points.is_empty() {
        return Some(None);
    }
    let centre = points
        .iter()
        .fold(Aabb::new_invalid(), |b, &p| b.merged(&Aabb::new(p, p)))
        .center();
    for p in &mut points {
        *p -= centre;
    }
    Some(Some((Cut::Points(points), centre)))
}

/// A shape's cut, if any, and the cut's centre in the shape's space.
type Side = (Option<Cut>, Vector);

/// Each shape cut to where it can meet the other while `g2` moves by `motion` relative to
/// `g1` (in `g1`'s space), within `margin`; `None` when they cannot meet.
fn cuts(
    pos12: &Pose,
    motion: Vector,
    margin: Real,
    g1: &dyn Shape,
    g2: &dyn Shape,
) -> Option<[Side; 2]> {
    let pos21 = pos12.inverse();
    let side = |shape: &dyn Shape, other: &dyn Shape, pose: &Pose, motion: Vector| {
        let polytope = shape.as_cuboid().is_some()
            || shape.as_triangle().is_some()
            || shape.as_convex_polyhedron().is_some();
        if !polytope {
            return Some((None, Vector::ZERO));
        }
        let bounds = shape.compute_local_aabb();
        match window(other, pose, motion, margin, &bounds).and_then(|w| cut(shape, &w)) {
            None => Some((None, Vector::ZERO)),
            Some(Some((cut, centre))) => Some((Some(cut), centre)),
            Some(None) => None,
        }
    };
    Some([
        side(g1, g2, pos12, motion)?,
        side(g2, g1, &pos21, -(pos21.rotation * motion))?,
    ])
}

/// `v` divided by its length. Parry scales by the reciprocal, which can leave a face normal
/// along an axis a hair shorter than 1; Rapier's character controller then sees the slide
/// along a flat floor dip into it, takes it for slipping down a slope and drops it.
fn unit(v: Vector) -> Vector {
    let length = v.length();
    if length > 0. { v / length } else { v }
}

/// Two capsules whose axes overlap side by side, as two characters standing in one another:
/// their Minkowski difference is round around the origin, Parry's EPA gives up there and the
/// pair reads as apart, so a teleport or a recovery took the space for free. Their contact is
/// the one of the axes' closest points, grown by the radii; axes that cross or run along each
/// other part across the first axis.
fn capsule_contact(pos12: &Pose, c1: &Capsule, c2: &Capsule, prediction: Real) -> Option<Contact> {
    let axis2 = c2.segment.transformed(pos12);
    let (at1, at2) = details::closest_points_segment_segment_with_locations_nD(
        (&c1.segment.a, &c1.segment.b),
        (&axis2.a, &axis2.b),
    );
    let (p1, p2) = (c1.segment.point_at(&at1), axis2.point_at(&at2));
    let length = (p2 - p1).length();
    let dist = length - c1.radius - c2.radius;
    if dist > prediction {
        return None;
    }
    let normal1 = if length > 1e-6 {
        (p2 - p1) / length
    } else {
        c1.segment
            .direction()
            .map_or(Vector::X, |axis| axis.any_orthonormal_vector())
    };
    let point2 = pos12.inverse_transform_point(p2 - normal1 * c2.radius);
    Some(Contact::new(
        p1 + normal1 * c1.radius,
        point2,
        normal1,
        pos12.rotation.inverse() * -normal1,
        dist,
    ))
}

/// The pose of `g2`'s cut in `g1`'s cut, from the centres of the cuts in their shapes.
fn between(pos12: &Pose, at1: Vector, at2: Vector) -> Pose {
    Pose::from_parts(
        pos12.translation + pos12.rotation * at2 - at1,
        pos12.rotation,
    )
}

impl QueryDispatcher for Windowed {
    fn intersection_test(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
    ) -> Result<bool, Unsupported> {
        DefaultQueryDispatcher.intersection_test(pos12, g1, g2)
    }
    fn distance(&self, pos12: &Pose, g1: &dyn Shape, g2: &dyn Shape) -> Result<Real, Unsupported> {
        DefaultQueryDispatcher.distance(pos12, g1, g2)
    }
    fn contact(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
        prediction: Real,
    ) -> Result<Option<Contact>, Unsupported> {
        // A ball has an exact contact of its own.
        if g1.as_ball().is_some() || g2.as_ball().is_some() {
            return DefaultQueryDispatcher.contact(pos12, g1, g2, prediction);
        }
        if let (Some(c1), Some(c2)) = (g1.as_capsule(), g2.as_capsule()) {
            return Ok(capsule_contact(pos12, c1, c2, prediction));
        }
        if let Some(c1) = g1.as_composite_shape() {
            return Ok(details::contact_composite_shape_shape(
                self, pos12, c1, g2, prediction,
            ));
        }
        if let Some(c2) = g2.as_composite_shape() {
            return Ok(details::contact_shape_composite_shape(
                self, pos12, g1, c2, prediction,
            ));
        }
        let (Some(s1), Some(s2)) = (g1.as_support_map(), g2.as_support_map()) else {
            return DefaultQueryDispatcher.contact(pos12, g1, g2, prediction);
        };
        let mut margin = prediction.max(0.) + ROOM;
        loop {
            let Some([(cut1, at1), (cut2, at2)]) = cuts(pos12, Vector::ZERO, margin, g1, g2) else {
                return Ok(None);
            };
            let cut = cut1.is_some() || cut2.is_some();
            let s1: &dyn SupportMap = cut1.as_ref().map_or(s1, |c| c);
            let s2: &dyn SupportMap = cut2.as_ref().map_or(s2, |c| c);
            let pos = between(pos12, at1, at2);
            let contact = details::contact_support_map_support_map(&pos, s1, s2, prediction);
            match contact {
                Some(c) if cut && -c.dist >= margin => margin = deeper(c.dist),
                _ => {
                    return Ok(contact.map(|c| Contact {
                        point1: c.point1 + at1,
                        point2: c.point2 + at2,
                        ..c
                    }));
                }
            }
        }
    }
    fn closest_points(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
        max_dist: Real,
    ) -> Result<ClosestPoints, Unsupported> {
        DefaultQueryDispatcher.closest_points(pos12, g1, g2, max_dist)
    }
    fn cast_shapes(
        &self,
        pos12: &Pose,
        vel12: Vector,
        g1: &dyn Shape,
        g2: &dyn Shape,
        options: ShapeCastOptions,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        if g1.as_ball().is_some() && g2.as_ball().is_some() {
            return DefaultQueryDispatcher.cast_shapes(pos12, vel12, g1, g2, options);
        }
        if let Some(c1) = g1.as_composite_shape() {
            return Ok(details::cast_shapes_composite_shape_shape(
                self, pos12, vel12, c1, g2, options,
            ));
        }
        if let Some(c2) = g2.as_composite_shape() {
            return Ok(details::cast_shapes_shape_composite_shape(
                self, pos12, vel12, g1, c2, options,
            ));
        }
        let (Some(s1), Some(s2)) = (g1.as_support_map(), g2.as_support_map()) else {
            return DefaultQueryDispatcher.cast_shapes(pos12, vel12, g1, g2, options);
        };
        let motion = vel12 * options.max_time_of_impact;
        let mut margin = options.target_distance + ROOM;
        loop {
            let Some([(cut1, at1), (cut2, at2)]) = cuts(pos12, motion, margin, g1, g2) else {
                return Ok(None);
            };
            let cut = cut1.is_some() || cut2.is_some();
            let s1: &dyn SupportMap = cut1.as_ref().map_or(s1, |c| c);
            let s2: &dyn SupportMap = cut2.as_ref().map_or(s2, |c| c);
            let pos = between(pos12, at1, at2);
            let hit = details::cast_shapes_support_map_support_map(&pos, vel12, s1, s2, options);
            // A cast starting in an overlap reports the way out of it.
            if cut
                && hit.is_some_and(|h| h.status == ShapeCastStatus::PenetratingOrWithinTargetDist)
                && let Some(c) = details::contact_support_map_support_map(&pos, s1, s2, Real::MAX)
                && -c.dist >= margin
            {
                margin = deeper(c.dist);
                continue;
            }
            return Ok(hit.map(|hit| ShapeCastHit {
                witness1: hit.witness1 + at1,
                witness2: hit.witness2 + at2,
                normal1: unit(hit.normal1),
                normal2: unit(hit.normal2),
                ..hit
            }));
        }
    }
    fn cast_shapes_nonlinear(
        &self,
        motion1: &NonlinearRigidMotion,
        g1: &dyn Shape,
        motion2: &NonlinearRigidMotion,
        g2: &dyn Shape,
        start_time: Real,
        end_time: Real,
        stop_at_penetration: bool,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        DefaultQueryDispatcher.cast_shapes_nonlinear(
            motion1,
            g1,
            motion2,
            g2,
            start_time,
            end_time,
            stop_at_penetration,
        )
    }
}
