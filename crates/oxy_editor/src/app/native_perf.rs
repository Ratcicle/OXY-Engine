#[path = "../../../../benchmarks/native_host.rs"]
mod harness;
#[test]
#[ignore = "Opt-in native CPU benchmark; opens an inactive WGPU window"]
fn native_editor_performance() {
    harness::run("OXY — medição do editor", |cc, project| {
        let mut editor = super::Editor::new(cc);
        editor.scene_id = project.start_scene.clone();
        editor.state.project = project;
        editor.home.visible = false;
        editor.debug = true;
        editor.camera.orthographic_size = 50.;
        Box::new(editor)
    });
}
