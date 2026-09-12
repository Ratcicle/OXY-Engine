use super::*;

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}
fn button(pressed: bool) -> Event {
    Event::PointerButton {
        pos: Pos2::new(40., 40.),
        button: PointerButton::Secondary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}
fn wheel() -> Event {
    Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: Vec2::new(0., 1.),
        modifiers: Modifiers::NONE,
    }
}
fn route(nav: &mut Navigation, events: Vec<Event>, allowed: bool) -> egui::RawInput {
    let mut input = egui::RawInput {
        events,
        focused: true,
        ..Default::default()
    };
    nav.route(&mut input, 1., allowed, [None; 4], |p| {
        Rect::from_min_max(Pos2::ZERO, Pos2::new(100., 100.)).contains(p)
    });
    input
}
#[test]
fn navigation_owns_keys_wheel_and_releases_without_editor_repeats() {
    let mut nav = Navigation::default();
    for code in [Key::W, Key::E, Key::R] {
        assert_eq!(route(&mut nav, vec![key(code, true)], true).events.len(), 1);
    }
    let input = route(
        &mut nav,
        vec![button(true), key(Key::W, true), key(Key::E, true), wheel()],
        true,
    );
    assert_eq!(input.events.len(), 1);
    assert!(nav.active && nav.held.contains(&Key::W));
    let mut camera = CameraState::for_scene(&Scene::new("Vista", SceneKind::ThreeD));
    let mut prefs = preferences::Preferences::default();
    let distance = camera.distance;
    nav.apply(&mut camera, &mut prefs, 0.02);
    assert_eq!(distance, camera.distance);
    assert_eq!(prefs.navigation_speed, 9.6);
    route(&mut nav, vec![button(false)], true);
    let eye = camera.eye();
    nav.apply(&mut camera, &mut prefs, 0.02);
    assert_eq!(camera.eye(), eye);
    assert!(nav.held.is_empty() && !nav.active);
    let mut repeat = key(Key::W, true);
    if let Event::Key { repeat, .. } = &mut repeat {
        *repeat = true;
    }
    assert!(
        route(&mut nav, vec![repeat, key(Key::W, false)], true)
            .events
            .is_empty()
    );
    assert_eq!(
        route(&mut nav, vec![key(Key::W, true), wheel()], true)
            .events
            .len(),
        2
    );
}
#[test]
fn modifiers_return_to_base_and_scroll_is_bounded() {
    for (mods, factor) in [
        (Modifiers::NONE, 1.),
        (Modifiers::SHIFT, 4.),
        (Modifiers::CTRL, 0.25),
        (Modifiers::SHIFT | Modifiers::CTRL, 1.),
    ] {
        assert_eq!(multiplier(mods), factor);
    }
    let mut nav = Navigation::default();
    route(&mut nav, vec![button(true), key(Key::W, true)], true);
    let mut prefs = preferences::Preferences::default();
    let mut camera = CameraState::for_scene(&Scene::new("Vista", SceneKind::ThreeD));
    for (mods, expected) in [
        (Modifiers::SHIFT, 1.6),
        (Modifiers::CTRL, 0.1),
        (Modifiers::NONE, 0.4),
    ] {
        route(&mut nav, vec![], true);
        nav.modifiers = mods;
        let eye = camera.eye();
        nav.apply(&mut camera, &mut prefs, 0.05);
        assert!(((camera.eye() - eye).length() - expected).abs() < 0.0001);
        assert_eq!(prefs.navigation_speed, 8.);
    }
    assert_eq!(scroll_speed(MAX_SPEED, 1.), MAX_SPEED);
    assert_eq!(scroll_speed(MIN_SPEED, -1.), MIN_SPEED);
    assert!((scroll_speed(scroll_speed(8., 1.), -1.) - 8.).abs() < 0.00001);
}

#[test]
fn key_already_held_starts_without_waiting_for_repeat_and_idle_rmb_does_not_drift() {
    let mut nav = Navigation::default();
    let mut input = egui::RawInput {
        focused: true,
        events: vec![button(true)],
        ..Default::default()
    };
    nav.route(
        &mut input,
        1.,
        true,
        [Some(Key::W), None, None, None],
        |_| true,
    );
    assert!(nav.held.contains(&Key::W));
    let mut camera = CameraState::for_scene(&Scene::new("Vista", SceneKind::ThreeD));
    let mut prefs = preferences::Preferences::default();
    route(&mut nav, vec![], true);
    let start = camera.eye();
    nav.apply(&mut camera, &mut prefs, 0.05);
    assert!((camera.eye() - start).length() > 0.39);
    route(&mut nav, vec![key(Key::W, false)], true);
    let before = camera.view();
    for _ in 0..1000 {
        nav.apply(&mut camera, &mut prefs, 0.05);
    }
    assert_eq!(camera.view(), before);
}
#[test]
fn invalid_context_exit_and_focus_loss_clear_navigation_until_new_press() {
    for event in [
        Event::PointerGone,
        Event::WindowFocused(false),
        Event::PointerMoved(Pos2::new(110., 40.)),
    ] {
        let mut nav = Navigation::default();
        route(&mut nav, vec![button(true), key(Key::W, true)], true);
        route(&mut nav, vec![event], true);
        assert!(!nav.active && nav.held.is_empty());
        route(
            &mut nav,
            vec![Event::PointerMoved(Pos2::new(40., 40.))],
            true,
        );
        assert!(!nav.active);
        route(
            &mut nav,
            vec![button(false), button(true), key(Key::W, true)],
            true,
        );
        assert!(nav.active);
    }
    let mut nav = Navigation::default();
    route(&mut nav, vec![button(true), key(Key::W, true)], true);
    route(&mut nav, vec![], false);
    assert!(!nav.active && nav.held.is_empty());
    route(&mut nav, vec![], true);
    assert!(!nav.active);
    // A missing key-up outside the window does not eat the next genuine editor press.
    assert_eq!(
        route(&mut nav, vec![key(Key::W, true)], true).events.len(),
        1
    );
    let mut input = egui::RawInput {
        focused: false,
        events: vec![button(true), key(Key::W, true)],
        ..Default::default()
    };
    nav.route(&mut input, 1., true, [None; 4], |_| true);
    assert!(!nav.active);
}

#[test]
fn existing_primary_gesture_has_priority_and_navigation_does_not_add_qe_axes() {
    let mut nav = Navigation::default();
    let primary = Event::PointerButton {
        pos: Pos2::new(40., 40.),
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    };
    route(
        &mut nav,
        vec![primary, button(true), key(Key::W, true)],
        true,
    );
    assert!(!nav.active);
    route(
        &mut nav,
        vec![
            button(false),
            button(true),
            key(Key::Q, true),
            key(Key::E, true),
        ],
        true,
    );
    assert!(nav.active && nav.held.is_empty());
}
#[test]
fn mouse_look_is_independent_of_ui_scale_and_event_splitting() {
    let mut views = Vec::new();
    for scale in [0.8, 1., 1.6] {
        let mut nav = Navigation::default();
        route(&mut nav, vec![button(true)], true);
        let mut input = egui::RawInput {
            focused: true,
            events: vec![Event::PointerMoved(Pos2::new(40. + 16. / scale, 40.))],
            ..Default::default()
        };
        nav.route(&mut input, scale, true, [None; 4], |_| true);
        let mut camera = CameraState::for_scene(&Scene::new("Vista", SceneKind::ThreeD));
        nav.apply(&mut camera, &mut preferences::Preferences::default(), 0.02);
        views.push(camera);
    }
    for camera in &views {
        assert!(camera.view().abs_diff_eq(views[0].view(), 0.00001));
    }
    let mut a = views[0].clone();
    let mut b = a.clone();
    a.look_from_eye([30., 20.], 0.003);
    for _ in 0..10 {
        b.look_from_eye([3., 2.], 0.003);
    }
    assert!(a.view().abs_diff_eq(b.view(), 0.00005));
    let mut nav = Navigation::default();
    route(&mut nav, vec![button(true)], true);
    let mut input = egui::RawInput {
        focused: true,
        events: vec![
            Event::PointerMoved(Pos2::new(45., 40.)),
            Event::MouseMoved(Vec2::new(20., -5.)),
        ],
        ..Default::default()
    };
    nav.route(&mut input, 1.6, true, [None; 4], |_| true);
    assert_eq!(nav.look, Vec2::new(20., -5.)); // Do not apply both relative and absolute motion.
}
