#[path = "../../../benchmarks/native_host.rs"]
mod harness;
struct FixedPlayer(super::Player);
impl eframe::App for FixedPlayer {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let rt = self.0.runtime.as_mut().unwrap();
        rt.set_paused(false);
        let before = rt.time;
        rt.advance(
            oxy_core::runtime::FIXED_DT,
            &oxy_core::runtime::InputFrame {
                held: ["mover_direita".into()].into(),
                ..Default::default()
            },
        );
        assert!((rt.time - before - f64::from(oxy_core::runtime::FIXED_DT)).abs() < 1e-7);
        rt.set_paused(true);
        self.0.update(ctx, frame);
    }
}
#[test]
#[ignore = "Opt-in native CPU benchmark; opens an inactive WGPU player"]
fn native_player_performance() {
    harness::run("OXY — medição do player", |cc, project| {
        let path = std::env::temp_dir().join("oxy-performance-project.oxy.json");
        std::fs::write(&path, serde_json::to_vec(&project).unwrap()).unwrap();
        let mut player = super::Player::new(cc, path);
        assert!(player.error.is_none(), "{:?}", player.error);
        player.runtime =
            Some(oxy_core::runtime::Runtime::new(&project, &project.start_scene).unwrap());
        Box::new(FixedPlayer(player))
    });
}
