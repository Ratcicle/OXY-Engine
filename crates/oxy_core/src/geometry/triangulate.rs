//! Ear clipping of a validated simple planar polygon, in a dominant-axis projection.
use glam::{DVec2, Vec3};
fn cross(a: DVec2, b: DVec2, c: DVec2) -> f64 {
    (b - a).perp_dot(c - a)
}
pub(super) fn polygon(points: &[Vec3]) -> Result<(Vec3, Vec<[usize; 3]>), String> {
    let origin = points[0];
    let scale = points
        .iter()
        .map(|p| p.distance(origin))
        .fold(0f32, f32::max);
    if scale < 1e-8 {
        return Err("face degenerada.".into());
    }
    // Relative coordinates avoid cancellation far from the origin.
    let p: Vec<_> = points
        .iter()
        .map(|p| ((*p - origin) / scale).as_dvec3())
        .collect();
    let normal = p
        .iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .fold(glam::DVec3::ZERO, |n, (a, b)| n + a.cross(*b));
    let normal = normal
        .try_normalize()
        .ok_or("face sem área ou contorno cruzado.")?;
    if p.iter().any(|v| v.dot(normal).abs() > 1e-5) {
        return Err("face não plana. Desfaça a deformação ou triangule explicitamente antes de mover os pontos.".into());
    }
    let axis = normal.abs().max_position();
    let flat: Vec<_> = p
        .iter()
        .map(|v| match axis {
            0 => DVec2::new(v.y, v.z),
            1 => DVec2::new(v.z, v.x),
            _ => DVec2::new(v.x, v.y),
        })
        .collect();
    const EPS: f64 = 1e-10;
    let n = flat.len();
    for i in 0..n {
        if flat[i].distance_squared(flat[(i + 1) % n]) < EPS * EPS {
            return Err("dois cantos consecutivos coincidem.".into());
        }
        for j in i + 1..n {
            if j == (i + 1) % n || i == (j + 1) % n {
                continue;
            }
            let (a, b, c, d) = (flat[i], flat[(i + 1) % n], flat[j], flat[(j + 1) % n]);
            let overlaps = (a.min(b) - DVec2::splat(EPS)).cmple(c.max(d)).all()
                && (c.min(d) - DVec2::splat(EPS)).cmple(a.max(b)).all();
            if overlaps
                && cross(a, b, c) * cross(a, b, d) <= EPS * EPS
                && cross(c, d, a) * cross(c, d, b) <= EPS * EPS
            {
                return Err("o contorno cruza ou toca a si mesmo.".into());
            }
        }
    }
    let area = flat
        .iter()
        .zip(flat.iter().cycle().skip(1))
        .take(n)
        .map(|(a, b)| a.perp_dot(*b))
        .sum::<f64>();
    if area.abs() < EPS {
        return Err("face sem área.".into());
    }
    let sign = area.signum();
    // Preserve the established primitive diagonals for strictly convex polygons.
    if (0..n).all(|i| cross(flat[i], flat[(i + 1) % n], flat[(i + 2) % n]) * sign > EPS) {
        return Ok((
            normal.as_vec3(),
            (1..n - 1).map(|i| [0, i, i + 1]).collect(),
        ));
    }
    let mut remaining: Vec<_> = (0..n).collect();
    let mut triangles = Vec::with_capacity(n - 2);
    while remaining.len() > 3 {
        let mut ear = None;
        for i in 0..remaining.len() {
            let ids = [
                remaining[(i + remaining.len() - 1) % remaining.len()],
                remaining[i],
                remaining[(i + 1) % remaining.len()],
            ];
            let [a, b, c] = ids.map(|id| flat[id]);
            if cross(a, b, c) * sign <= EPS {
                continue;
            }
            if remaining.iter().any(|id| {
                !ids.contains(id)
                    && cross(a, b, flat[*id]) * sign >= -EPS
                    && cross(b, c, flat[*id]) * sign >= -EPS
                    && cross(c, a, flat[*id]) * sign >= -EPS
            }) {
                continue;
            }
            ear = Some((i, ids));
            break;
        }
        let Some((i, ids)) = ear else {
            return Err("não foi possível triangular o contorno sem degenerar faces.".into());
        };
        triangles.push(ids);
        remaining.remove(i);
    }
    if cross(flat[remaining[0]], flat[remaining[1]], flat[remaining[2]]).abs() < EPS {
        return Err("triângulo final degenerado.".into());
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    Ok((normal.as_vec3(), triangles))
}
