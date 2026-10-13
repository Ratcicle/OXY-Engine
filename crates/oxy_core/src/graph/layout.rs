//! Node sizes as the editor draws them, and a layout that keeps logic built without positions,
//! such as an agent's, readable.
use super::{Graph, OperationDef, ParamDef, PortType, definition};
use crate::document::Id;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

pub const NODE_WIDTH: f32 = 200.;
pub const HEADER_HEIGHT: f32 = 30.;
pub const ROW_HEIGHT: f32 = 22.;
pub const BODY_TOP: f32 = 6.;
pub const BODY_BOTTOM: f32 = 8.;
/// From one column of [`arrange`] to the next: a node and room for its wires.
pub const COLUMN_STEP: f32 = 260.;
/// Between nodes stacked in a column.
pub const ROW_GAP: f32 = 24.;
/// Between the bands of [`arrange`], and between a graph and what goes below it.
pub const BAND_GAP: f32 = 80.;

/// Parameters without an input port of the same name: rows of the node body.
pub fn summary_params(op: &OperationDef) -> impl Iterator<Item = &ParamDef> {
    op.params
        .iter()
        .filter(|param| !op.inputs.iter().any(|port| port.id == param.id))
}

pub fn port_rows(op: &OperationDef) -> usize {
    op.inputs.len().max(op.outputs.len())
}

/// Size of a node in graph units, as the editor draws it. An unknown operation keeps one row
/// for its identifier.
pub fn node_extent(op: Option<&OperationDef>) -> [f32; 2] {
    let rows = op
        .map_or(1, |op| port_rows(op) + summary_params(op).count())
        .max(1);
    [
        NODE_WIDTH,
        HEADER_HEIGHT + BODY_TOP + rows as f32 * ROW_HEIGHT + BODY_BOTTOM,
    ]
}

/// Lays out the nodes of `graph` listed in `movable`; the others stay where they are. Events
/// take column 0 and execution goes left to right by its longest path; a node that only gives
/// or takes data sits one column left of its first consumer; a column is stacked in the order of
/// what feeds it, and each event's chain takes a band of its own. Movable nodes connected to
/// nodes that stay start to their right (or end to their left when they only feed them); the
/// others go below the graph, event chains before what no event runs. Node rectangles
/// ([`node_extent`]) never overlap. A cycle, which validation refuses, only orders its nodes as
/// they come.
pub fn arrange(graph: &mut Graph, movable: &HashSet<Id>) {
    for (index, position) in positions(graph, movable) {
        graph.nodes[index].position = position;
    }
}

/// Where [`arrange`] puts each movable node, by its index in the graph.
fn positions(graph: &Graph, movable: &HashSet<Id>) -> Vec<(usize, [f32; 2])> {
    let moving: Vec<bool> = graph
        .nodes
        .iter()
        .map(|node| movable.contains(&node.id))
        .collect();
    if !moving.contains(&true) {
        return Vec::new();
    }
    let index: HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.id.as_str(), i))
        .collect();
    let defs: Vec<Option<&OperationDef>> = graph
        .nodes
        .iter()
        .map(|node| definition(&node.operation))
        .collect();
    let sizes: Vec<[f32; 2]> = defs.iter().map(|def| node_extent(*def)).collect();
    // Connections between movable nodes, and those with a node that stays at one end; two
    // connections between the same nodes count once.
    let mut inner = BTreeSet::new();
    let mut outer = BTreeSet::new();
    for edge in &graph.edges {
        let (Some(&from), Some(&to)) = (
            index.get(edge.from_node.as_str()),
            index.get(edge.to_node.as_str()),
        ) else {
            continue;
        };
        match (moving[from], moving[to]) {
            _ if from == to => {}
            (true, true) => {
                inner.insert((from, to));
            }
            (false, false) => {}
            _ => {
                outer.insert((from, to));
            }
        }
    }
    let mut occupied: Vec<[f32; 4]> = (0..graph.nodes.len())
        .filter(|i| !moving[*i])
        .map(|i| rect(graph.nodes[i].position, sizes[i]))
        .collect();
    let mut placed = Vec::new();
    // Event chains first, then what no event runs.
    let mut groups = components(&moving, &inner);
    groups.sort_by_key(|members| (!members.iter().any(|i| is_event(defs[*i])), members[0]));
    for members in groups {
        let local = block(&members, &inner, &defs, &sizes);
        let at = |i: usize| graph.nodes[i].position;
        let member: HashSet<usize> = members.iter().copied().collect();
        let sources: Vec<usize> = outer
            .iter()
            .filter(|(_, to)| member.contains(to))
            .map(|(from, _)| *from)
            .collect();
        let sinks: Vec<usize> = outer
            .iter()
            .filter(|(from, _)| member.contains(from))
            .map(|(_, to)| *to)
            .collect();
        let top = |nodes: &[usize]| {
            nodes
                .iter()
                .map(|i| at(*i)[1])
                .fold(f32::INFINITY, f32::min)
        };
        let mut origin = if !sources.is_empty() {
            let right = sources
                .iter()
                .map(|i| at(*i)[0])
                .fold(f32::NEG_INFINITY, f32::max);
            [right + COLUMN_STEP, top(&sources)]
        } else if !sinks.is_empty() {
            let left = sinks
                .iter()
                .map(|i| at(*i)[0])
                .fold(f32::INFINITY, f32::min);
            let last = local.iter().map(|(_, p)| p[0]).fold(0., f32::max);
            [left - COLUMN_STEP - last, top(&sinks)]
        } else if occupied.is_empty() {
            [0., 0.]
        } else {
            let left = occupied.iter().map(|r| r[0]).fold(f32::INFINITY, f32::min);
            let bottom = occupied
                .iter()
                .map(|r| r[3])
                .fold(f32::NEG_INFINITY, f32::max);
            [left, bottom + BAND_GAP]
        };
        // Down past whatever is in the way: each move leaves one rectangle above for good.
        for _ in 0..=occupied.len() * local.len() {
            let down = local.iter().find_map(|(i, p)| {
                let r = rect([origin[0] + p[0], origin[1] + p[1]], sizes[*i]);
                occupied
                    .iter()
                    .find(|o| overlaps(&r, o))
                    .map(|o| o[3] + ROW_GAP - r[1])
            });
            match down {
                Some(down) => origin[1] += down.max(1.),
                None => break,
            }
        }
        for (i, p) in local {
            let position = [origin[0] + p[0], origin[1] + p[1]];
            occupied.push(rect(position, sizes[i]));
            placed.push((i, position));
        }
    }
    placed
}

/// Whether a node of `def` has an execution port among its inputs (or its outputs).
fn has_exec(def: Option<&OperationDef>, inputs: bool) -> bool {
    def.is_some_and(|def| {
        let ports = if inputs { &def.inputs } else { &def.outputs };
        ports.iter().any(|port| port.kind == PortType::Exec)
    })
}

/// Whether a node of `def` starts execution, as events do.
fn is_event(def: Option<&OperationDef>) -> bool {
    has_exec(def, false) && !has_exec(def, true)
}

/// [min x, min y, max x, max y] of a node at `position`.
fn rect(position: [f32; 2], size: [f32; 2]) -> [f32; 4] {
    [
        position[0],
        position[1],
        position[0] + size[0],
        position[1] + size[1],
    ]
}

/// Whether two node rectangles touch, counting the room kept between stacked nodes.
fn overlaps(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] + ROW_GAP && b[1] < a[3] + ROW_GAP
}

/// The movable nodes connected among themselves, each group in graph order, the groups in the
/// order of their first node.
fn components(moving: &[bool], inner: &BTreeSet<(usize, usize)>) -> Vec<Vec<usize>> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut parent: Vec<usize> = (0..moving.len()).collect();
    for &(a, b) in inner {
        let (a, b) = (root(&mut parent, a), root(&mut parent, b));
        parent[a.max(b)] = a.min(b);
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut slot = HashMap::new();
    for i in (0..moving.len()).filter(|i| moving[*i]) {
        let r = root(&mut parent, i);
        let g = *slot.entry(r).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[g].push(i);
    }
    groups
}

/// Positions of a group of connected nodes relative to its top left corner.
fn block(
    members: &[usize],
    inner: &BTreeSet<(usize, usize)>,
    defs: &[Option<&OperationDef>],
    sizes: &[[f32; 2]],
) -> Vec<(usize, [f32; 2])> {
    let member: HashSet<usize> = members.iter().copied().collect();
    let mut preds: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut succs: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(from, to) in inner.iter().filter(|(from, _)| member.contains(from)) {
        preds.entry(to).or_default().push(from);
        succs.entry(from).or_default().push(to);
    }
    let none = Vec::new();
    let preds_of = |i: usize| preds.get(&i).unwrap_or(&none);
    let succs_of = |i: usize| succs.get(&i).unwrap_or(&none);

    // Topological order, graph order among equals; a node left in a cycle comes in graph order.
    let mut waiting: HashMap<usize, usize> =
        members.iter().map(|i| (*i, preds_of(*i).len())).collect();
    let mut ready: BTreeSet<usize> = members
        .iter()
        .copied()
        .filter(|i| waiting[i] == 0)
        .collect();
    let mut order = Vec::with_capacity(members.len());
    let mut seen = HashSet::new();
    while order.len() < members.len() {
        let next = match ready.pop_first() {
            Some(next) => next,
            None => *members.iter().find(|i| !seen.contains(*i)).unwrap(),
        };
        if !seen.insert(next) {
            continue;
        }
        order.push(next);
        for s in succs_of(next) {
            let left = waiting.get_mut(s).unwrap();
            *left = left.saturating_sub(1);
            if *left == 0 && !seen.contains(s) {
                ready.insert(*s);
            }
        }
    }
    let place: HashMap<usize, usize> = order.iter().enumerate().map(|(k, i)| (*i, k)).collect();

    // Columns by the longest path; data-only nodes then move up to their first consumer.
    let mut col: HashMap<usize, usize> = HashMap::new();
    for &i in &order {
        let rank = preds_of(i)
            .iter()
            .filter(|p| place[*p] < place[&i])
            .map(|p| col[p] + 1)
            .max()
            .unwrap_or(0);
        col.insert(i, rank);
    }
    for &i in order.iter().rev() {
        let data_only = defs[i].is_some() && !has_exec(defs[i], true) && !has_exec(defs[i], false);
        if let Some(first) = succs_of(i).iter().map(|s| col[s]).min()
            && data_only
        {
            col.insert(i, first.saturating_sub(1));
        }
    }

    // Bands: what each event reaches, in graph order; then whatever feeds a band joins it; the
    // rest shares one band.
    let mut band: HashMap<usize, usize> = HashMap::new();
    let mut bands = 0;
    for &event in members.iter().filter(|i| is_event(defs[**i])) {
        let mut queue = VecDeque::from([event]);
        while let Some(i) = queue.pop_front() {
            if band.contains_key(&i) {
                continue;
            }
            band.insert(i, bands);
            queue.extend(succs_of(i).iter().copied());
        }
        bands += 1;
    }
    for &i in order.iter().rev() {
        if band.contains_key(&i) {
            continue;
        }
        if let Some(b) = succs_of(i)
            .iter()
            .filter_map(|s| Some(((col[s], *s), *band.get(s)?)))
            .min()
            .map(|(_, b)| b)
        {
            band.insert(i, b);
        }
    }
    for &i in members {
        band.entry(i).or_insert(bands);
    }

    // Each band's columns, ordered by where their neighbours sit (barycenters), then stacked.
    let mut result = Vec::with_capacity(members.len());
    let mut top = 0.;
    let mut band_ids: Vec<usize> = band.values().copied().collect();
    band_ids.sort_unstable();
    band_ids.dedup();
    for b in band_ids {
        let mut columns: Vec<(usize, Vec<usize>)> = Vec::new();
        for &i in members.iter().filter(|i| band[*i] == b) {
            match columns.iter_mut().find(|(c, _)| *c == col[&i]) {
                Some((_, list)) => list.push(i),
                None => columns.push((col[&i], vec![i])),
            }
        }
        columns.sort_by_key(|(c, _)| *c);
        let mut slot: HashMap<usize, f32> = HashMap::new();
        for (_, list) in &columns {
            for (k, i) in list.iter().enumerate() {
                slot.insert(*i, k as f32);
            }
        }
        for pass in 0..3 {
            let forward = pass % 2 == 0;
            let sequence: Vec<usize> = if forward {
                (0..columns.len()).collect()
            } else {
                (0..columns.len()).rev().collect()
            };
            for k in sequence {
                let c = columns[k].0;
                let key = |i: usize| {
                    let near: Vec<f32> = if forward { preds_of(i) } else { succs_of(i) }
                        .iter()
                        .filter(|n| band[*n] == b && (col[*n] < c) == forward && col[*n] != c)
                        .map(|n| slot[n])
                        .collect();
                    if near.is_empty() {
                        slot[&i]
                    } else {
                        near.iter().sum::<f32>() / near.len() as f32
                    }
                };
                let mut keyed: Vec<(f32, f32, usize)> = columns[k]
                    .1
                    .iter()
                    .map(|i| (key(*i), slot[i], *i))
                    .collect();
                keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
                columns[k].1 = keyed.into_iter().map(|(_, _, i)| i).collect();
                for (row, i) in columns[k].1.iter().enumerate() {
                    slot.insert(*i, row as f32);
                }
            }
        }
        let mut height: f32 = 0.;
        for (c, list) in &columns {
            let mut y = 0.;
            for &i in list {
                result.push((i, [*c as f32 * COLUMN_STEP, top + y]));
                y += sizes[i][1] + ROW_GAP;
            }
            height = height.max(y - ROW_GAP);
        }
        top += height + BAND_GAP;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Node};

    fn node(graph: &mut Graph, operation: &str) -> Id {
        let node = Node::new(operation, [0.; 2]);
        let id = node.id.clone();
        graph.nodes.push(node);
        id
    }
    fn link(graph: &mut Graph, from: &str, from_port: &str, to: &str, to_port: &str) {
        graph.edges.push(Edge {
            from_node: from.into(),
            from_port: from_port.into(),
            to_node: to.into(),
            to_port: to_port.into(),
        });
    }
    fn all(graph: &Graph) -> HashSet<Id> {
        graph.nodes.iter().map(|n| n.id.clone()).collect()
    }
    fn at(graph: &Graph, id: &str) -> [f32; 2] {
        graph.node(id).unwrap().position
    }
    fn assert_apart(graph: &Graph) {
        let rects: Vec<[f32; 4]> = graph
            .nodes
            .iter()
            .map(|n| rect(n.position, node_extent(definition(&n.operation))))
            .collect();
        for (a, ra) in rects.iter().enumerate() {
            assert!(ra.iter().all(|v| v.is_finite()), "{ra:?}");
            for rb in &rects[a + 1..] {
                let apart = ra[2] <= rb[0] || rb[2] <= ra[0] || ra[3] <= rb[1] || rb[3] <= ra[1];
                assert!(apart, "{ra:?} overlaps {rb:?}");
            }
        }
    }

    /// Two events, one with a branch and a data chain, one with a spawn whose output feeds
    /// another action, sharing the last message; and a loose value.
    fn sample() -> (Graph, Vec<Id>) {
        let mut g = Graph::default();
        let start = node(&mut g, "event.scene_start");
        let branch = node(&mut g, "condition.branch");
        let compare = node(&mut g, "condition.compare");
        let read = node(&mut g, "attribute.get");
        let number = node(&mut g, "value.number");
        let yes = node(&mut g, "debug.message");
        let no = node(&mut g, "attribute.set");
        let after = node(&mut g, "debug.message");
        let click = node(&mut g, "event.click");
        let spawn = node(&mut g, "action.spawn");
        let remove = node(&mut g, "action.remove");
        let loose = node(&mut g, "value.text");
        link(&mut g, &start, "exec", &branch, "exec");
        link(&mut g, &read, "value", &compare, "a");
        link(&mut g, &number, "value", &compare, "b");
        link(&mut g, &compare, "result", &branch, "condition");
        link(&mut g, &branch, "then", &yes, "exec");
        link(&mut g, &branch, "else", &no, "exec");
        link(&mut g, &yes, "exec", &after, "exec");
        link(&mut g, &number, "value", &no, "value");
        link(&mut g, &click, "exec", &spawn, "exec");
        link(&mut g, &spawn, "exec", &remove, "exec");
        link(&mut g, &spawn, "created", &remove, "target");
        link(&mut g, &spawn, "exec", &after, "exec");
        let ids = vec![
            start, branch, compare, read, number, yes, no, after, click, spawn, remove, loose,
        ];
        (g, ids)
    }

    #[test]
    fn sizes_follow_ports_and_body_rows() {
        let message = node_extent(definition("debug.message"));
        // Executar/Continuar, plus the message parameter, which has no port.
        assert_eq!(message, [200., 30. + 6. + 2. * 22. + 8.]);
        assert_eq!(node_extent(None), [200., 30. + 6. + 22. + 8.]);
    }

    #[test]
    fn events_lead_columns_and_data_feeds_from_the_left_without_overlaps() {
        let (mut g, ids) = sample();
        let [
            start,
            branch,
            compare,
            read,
            number,
            yes,
            no,
            after,
            click,
            spawn,
            remove,
            loose,
        ] = ids.try_into().unwrap();
        g.validate(&HashSet::new()).unwrap();
        let every = all(&g);
        arrange(&mut g, &every);
        assert_apart(&g);
        for event in [&start, &click] {
            assert_eq!(at(&g, event)[0], 0., "events in column 0");
        }
        // Execution goes left to right.
        for (a, b) in [
            (&start, &branch),
            (&branch, &yes),
            (&branch, &no),
            (&yes, &after),
            (&click, &spawn),
            (&spawn, &remove),
        ] {
            assert!(at(&g, a)[0] < at(&g, b)[0], "{a} before {b}");
        }
        // Data sits one column left of its first consumer.
        assert_eq!(at(&g, &compare)[0], at(&g, &branch)[0] - COLUMN_STEP);
        assert_eq!(at(&g, &read)[0], at(&g, &compare)[0] - COLUMN_STEP);
        assert!(at(&g, &number)[0] < at(&g, &no)[0]);
        assert_eq!(at(&g, &number)[0], at(&g, &compare)[0] - COLUMN_STEP);
        // Each event's chain has its own band, the loose value below them; the message both
        // chains run stays in the first.
        let bottom = |ids: &[&Id]| {
            ids.iter()
                .map(|id| {
                    at(&g, id)[1] + node_extent(definition(&g.node(id).unwrap().operation))[1]
                })
                .fold(f32::NEG_INFINITY, f32::max)
        };
        let first = bottom(&[&start, &branch, &compare, &read, &number, &yes, &no, &after]);
        assert!(
            at(&g, &click)[1] >= first + BAND_GAP,
            "second band below the first"
        );
        assert!(at(&g, &loose)[1] > bottom(&[&click, &spawn, &remove]));
        // The same graph gives the same layout.
        let again = g.clone();
        let every = all(&g);
        arrange(&mut g, &every);
        assert_eq!(g, again);
    }

    #[test]
    fn nodes_that_stay_are_kept_and_new_ones_go_beside_or_below_them() {
        let (mut g, ids) = sample();
        // Everything but the click's actions and the loose value was placed by hand.
        for (k, node) in g.nodes.iter_mut().enumerate() {
            node.position = [-2400. + k as f32 * 300., 150. * k as f32];
        }
        let fixed: Vec<(Id, [f32; 2])> =
            g.nodes.iter().map(|n| (n.id.clone(), n.position)).collect();
        let movable: HashSet<Id> = [&ids[9], &ids[10], &ids[11]].into_iter().cloned().collect();
        // The event that feeds the new nodes stays: they start right of it.
        arrange(&mut g, &movable);
        for (id, position) in &fixed {
            if !movable.contains(id) {
                assert_eq!(at(&g, id), *position, "explicit positions untouched");
            }
        }
        assert_eq!(at(&g, &ids[9])[0], at(&g, &ids[8])[0] + COLUMN_STEP);
        assert!(at(&g, &ids[10])[0] > at(&g, &ids[9])[0]);
        // The loose value goes below the whole graph.
        let bottom = fixed
            .iter()
            .filter(|(id, _)| !movable.contains(id))
            .map(|(id, p)| p[1] + node_extent(definition(&g.node(id).unwrap().operation))[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(at(&g, &ids[11])[1] >= bottom + BAND_GAP);
        assert_apart(&g);

        // A new value that only feeds a node that stays ends left of it.
        let mut g = Graph::default();
        let set = node(&mut g, "attribute.set");
        g.nodes[0].position = [600., 300.];
        let value = node(&mut g, "value.number");
        link(&mut g, &value, "value", &set, "value");
        arrange(&mut g, &HashSet::from([value.clone()]));
        assert_eq!(at(&g, &set), [600., 300.]);
        assert_eq!(at(&g, &value), [600. - COLUMN_STEP, 300.]);
        // Nothing movable, nothing moves.
        let before = g.clone();
        arrange(&mut g, &HashSet::from(["fantasma".to_owned()]));
        assert_eq!(g, before);
    }

    #[test]
    fn cycles_and_crowded_spots_still_end() {
        // Validation refuses cycles; the layout still ends on one.
        let mut g = Graph::default();
        let a = node(&mut g, "debug.message");
        let b = node(&mut g, "debug.message");
        let c = node(&mut g, "debug.message");
        link(&mut g, &a, "exec", &b, "exec");
        link(&mut g, &b, "exec", &c, "exec");
        link(&mut g, &c, "exec", &a, "exec");
        link(&mut g, &a, "exec", &a, "exec");
        assert!(g.validate(&HashSet::new()).is_err());
        let every = all(&g);
        arrange(&mut g, &every);
        assert_apart(&g);
        assert!(at(&g, &a)[0] < at(&g, &b)[0] && at(&g, &b)[0] < at(&g, &c)[0]);

        // A new chain whose spot right of its event is taken moves down past what is there.
        let mut g = Graph::default();
        let event = node(&mut g, "event.scene_start");
        let blocker = node(&mut g, "debug.message");
        g.nodes[1].position = [COLUMN_STEP, 10.];
        let first = node(&mut g, "debug.message");
        let second = node(&mut g, "debug.message");
        link(&mut g, &event, "exec", &first, "exec");
        link(&mut g, &first, "exec", &second, "exec");
        arrange(&mut g, &HashSet::from([first.clone(), second.clone()]));
        assert_eq!(at(&g, &blocker), [COLUMN_STEP, 10.]);
        assert_eq!(at(&g, &first)[0], COLUMN_STEP);
        assert_apart(&g);
    }
}
