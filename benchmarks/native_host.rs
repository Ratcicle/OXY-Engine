//! Shared opt-in harness for real native hosts. App::update CPU time includes
//! driver/submit work, but excludes eframe tessellation, GPU completion and VSync.
use oxy_core::document::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
pub fn fixture(n: usize) -> Project {
    let mut p = Project::new("Carga nativa determinística");
    p.id = "00000000-0000-4000-8000-000000000001".into();
    p.scenes[0].id = "00000000-0000-4000-8000-000000000002".into();
    p.start_scene = p.scenes[0].id.clone();
    for i in 0..n {
        let mut e = Entity::new(format!("Peça {i:04}"), Some(Primitive::Rectangle));
        e.id = format!("00000000-0000-4000-8000-{:012x}", 100 + i);
        e.transform.position = [(i % 40) as f32 - 20., (i / 40) as f32 - 20., 0.];
        e.dimensions = [0.7, 0.7, 1.];
        e.material.color = [0.2 + (i % 4) as f32 * 0.12, 0.6, 0.8, 1.];
        if i < n / 2 {
            e.collider = Some(Collider {
                size: e.dimensions,
                ..Default::default()
            });
        }
        if i < 10 {
            e.controller = Some(Controller {
                gravity: 0.,
                ..Default::default()
            });
        }
        p.scenes[0].entities.push(e);
    }
    p
}
struct Measured {
    inner: Box<dyn eframe::App>,
    times: Vec<u64>,
    frames: usize,
    start: Instant,
    output: PathBuf,
    screenshot: bool,
}
impl eframe::App for Measured {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let pixels = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(pixels) = pixels {
            let rgba: Vec<u8> = pixels.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::save_buffer(
                self.output.with_extension("png"),
                &rgba,
                pixels.width() as u32,
                pixels.height() as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
            self.screenshot = true;
        }
        if self.frames >= 121 && self.screenshot {
            self.times.sort_unstable();
            let json = serde_json::json!({"samples":self.times.len(),"median_ns":self.times[50],"p95_ns":self.times[95],"p99_ns":self.times[99],"cpu_update_ns":self.times,"method":"20 warmups + 101 App::update CPU samples, real WGPU host at 1280x720 logical points, forced redraw solely for benchmark; no GPU/VSync timing; player includes exactly one manual fixed step, host runtime paused to prevent accumulated steps","entities":std::env::var("OXY_PERF_ENTITIES").unwrap_or("1600".into())});
            std::fs::write(&self.output, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        assert!(
            self.start.elapsed() < Duration::from_secs(120),
            "Native measurement exceeded 120 seconds"
        );
        let start = Instant::now();
        self.inner.update(ctx, frame);
        if (20..121).contains(&self.frames) {
            self.times.push(start.elapsed().as_nanos() as u64);
        }
        if self.frames == 115 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        self.frames += 1;
        ctx.request_repaint();
    }
}
pub fn run(
    label: &str,
    make: impl FnOnce(&eframe::CreationContext<'_>, Project) -> Box<dyn eframe::App> + 'static,
) {
    use winit::platform::windows::EventLoopBuilderExtWindows;
    let output = PathBuf::from(
        std::env::var("OXY_PERF_OUTPUT").expect("Set OXY_PERF_OUTPUT to a new JSON file"),
    );
    assert!(!output.exists(), "Preserve previous measurement");
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let n = std::env::var("OXY_PERF_ENTITIES")
        .unwrap_or("1600".into())
        .parse()
        .unwrap();
    eframe::run_native(
        label,
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280., 720.])
                .with_active(false),
            event_loop_builder: Some(Box::new(|b| {
                b.with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(Measured {
                inner: make(cc, fixture(n)),
                times: Vec::new(),
                frames: 0,
                start: Instant::now(),
                output,
                screenshot: false,
            }))
        }),
    )
    .unwrap();
}
