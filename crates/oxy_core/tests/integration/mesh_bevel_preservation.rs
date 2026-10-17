use glam::Vec3;
use oxy_core::{
    document::{Entity, Primitive, Project, new_id},
    edit_history::CommandHistory,
    geometry::{
        self, Edge, EditableMesh, bevel,
        primitives::Parameters,
        selection::{Mode, Selection},
    },
    persistence,
    texture_cache::TextureCache,
};
use std::collections::HashMap;

fn cube() -> EditableMesh {
    geometry::primitives::generate(Primitive::Cube, 8, Parameters::default()).unwrap()
}
fn selected(id: u32) -> Selection {
    Selection {
        mode: Mode::Edge,
        ids: vec![id],
        through: false,
    }
}
fn surface_closed(mesh: &EditableMesh) {
    assert!(
        mesh.prepared()
            .incident_faces
            .iter()
            .all(|uses| uses.is_empty() || uses.len() == 2)
    );
}

#[test]
fn valid_cube_bevel_preserves_isolated_interior_vertex_and_roundtrips_history_and_disk() {
    let cube = cube();
    let edge = cube.data().edges[0].id;
    let mut data = cube.data().clone();
    let isolated = data.add_vertex(Vec3::ZERO).unwrap();
    let mesh = EditableMesh::new(data).unwrap();
    let original_revision = mesh.revision();
    let output = bevel::apply(&mesh, &selected(edge), 0.1, 4).unwrap();
    assert_eq!(output.mesh.position(isolated), Some(Vec3::ZERO));
    assert_eq!(mesh.revision(), original_revision);
    assert!(output.mesh.data().faces.len() > mesh.data().faces.len());
    surface_closed(&output.mesh);
    let mut project = Project::new("Preservação do arredondamento");
    let mut entity = Entity::new("Cubo e ponto", None);
    entity.mesh = Some(mesh.clone());
    let id = entity.id.clone();
    project.scenes[0].entities.push(entity);
    let before = project.clone();
    let mut history = CommandHistory::new();
    let mut images = TextureCache::default();
    history.begin("Arredondar", &project, &images);
    project.scenes[0].entity_mut(&id).unwrap().mesh = Some(output.mesh.clone());
    assert!(history.commit(&project, &mut images).unwrap());
    let after = project.clone();
    history.undo(&mut project, &mut images).unwrap();
    assert_eq!(project, before);
    history.redo(&mut project, &mut images).unwrap();
    assert_eq!(project, after);
    let directory = std::env::temp_dir().join(format!("oxy-bevel-preservation-{}", new_id()));
    let path = directory.join("project.oxy.json");
    persistence::save_bundle(&path, &project, &HashMap::new()).unwrap();
    assert_eq!(persistence::load_project(&path).unwrap(), project);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn loose_edges_inside_outside_and_attached_to_replaced_corner_keep_ids_and_positions() {
    let cube = cube();
    let edge = &cube.data().edges[0];
    let corner = edge.vertices[0];
    let mut data = cube.data().clone();
    let a = data.add_vertex(Vec3::new(0., 0., 0.01)).unwrap();
    let b = data.add_vertex(Vec3::new(100., 200., 300.)).unwrap();
    let loose = data.allocate_id().unwrap();
    data.edges.push(Edge {
        id: loose,
        vertices: [a, b],
    });
    let attached = data.allocate_id().unwrap();
    data.edges.push(Edge {
        id: attached,
        vertices: [corner, a],
    });
    let source = EditableMesh::new(data).unwrap();
    let output = bevel::apply(&source, &selected(edge.id), 0.1, 4).unwrap();
    surface_closed(&output.mesh);
    for id in [a, b, corner] {
        assert_eq!(output.mesh.position(id), source.position(id));
    }
    for id in [loose, attached] {
        assert_eq!(output.mesh.edge(id), source.edge(id));
        assert!(
            output.mesh.prepared().incident_faces[output.mesh.prepared().edges[&id]].is_empty()
        );
    }
    assert!(
        bevel::apply(&source, &selected(loose), 0.1, 4)
            .err()
            .unwrap()
            .contains("solta")
    );
}

#[test]
fn incompatible_open_surface_reports_region_without_removing_it() {
    let cube = cube();
    let mut data = cube.data().clone();
    data.faces.remove(0);
    let mesh = EditableMesh::new(data).unwrap();
    let before = mesh.data().clone();
    let error = bevel::apply(&mesh, &selected(mesh.data().edges[0].id), 0.1, 2)
        .err()
        .unwrap();
    assert!(error.contains("aresta") && error.contains("abertura"));
    assert_eq!(mesh.data(), &before);
}
