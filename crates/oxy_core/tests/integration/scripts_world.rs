//! The areas of the scripts' API that reach the world beyond objects (design §5.4, stage 4): the
//! 3D character through `obj.character`, the physics (switches and queries), animations, the
//! camera, the interface, the sound loops and volumes, the game, the saved progress, input,
//! time and draws, and the names kept for 0.8. Each function does what its node does, through the
//! same operation, and refuses what it cannot do with a message that names the object; a project
//! that uses every area replays alike and survives a VM made anew.
use oxy_core::{
    animation::Clip,
    character::{CameraMode, CameraRig, CharacterConfig, MovementProfile},
    document::*,
    headless::{self, SimulationRequest},
    input_actions,
    physics3d::{Collider3d, CollisionShape},
    runtime::{FIXED_DT, InputFrame, Runtime, RuntimeOptions},
    script::{self, ScriptCall, ScriptError, ScriptRef, ScriptSources},
    surface::{SurfaceMaterial, SurfacePreset, TranslationPlatform},
    variables,
};
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;

const PATH: &str = "scripts/teste.luau";

fn run(project: &Project, source: &str) -> Runtime {
    let options = RuntimeOptions {
        scripts: Arc::new(ScriptSources::from_texts([(PATH, source)])),
        ..Default::default()
    };
    Runtime::with_options(project, &project.start_scene, options).unwrap()
}

fn step(rt: &mut Runtime, steps: usize) {
    for _ in 0..steps {
        rt.advance(FIXED_DT, &InputFrame::default());
    }
}

/// An object named `name` with a text attribute `Resultado` that runs `scripts/teste.luau`.
fn object(name: &str, primitive: Option<Primitive>) -> Entity {
    let mut e = Entity::new(name, primitive);
    e.attributes
        .insert("Resultado".into(), Value::Text(String::new()));
    e.scripts = vec![ScriptRef::new(PATH)];
    e
}

/// A box with a 3D collider at `position`, `size` metres.
fn block(name: &str, position: [f32; 3], size: [f32; 3]) -> Entity {
    let mut e = Entity::new(name, Some(Primitive::Cube));
    e.transform.position = position;
    e.dimensions = size;
    e.physics3d = Some(Collider3d {
        shape: CollisionShape::Box { size },
        ..Default::default()
    });
    e
}

/// A 3D project with a floor 40 m wide whose top is at y = 0, and `entities`.
fn world(entities: Vec<Entity>) -> Project {
    let mut project = Project::new("Mundo");
    project.scenes[0].kind = SceneKind::ThreeD;
    project.scenes[0]
        .entities
        .push(block("Chão", [0., -0.5, 0.], [40., 1., 40.]));
    project.scenes[0].entities.extend(entities);
    project
}

fn result(rt: &Runtime, id: &str) -> String {
    match &rt.scene().entity(id).unwrap().attributes["Resultado"] {
        Value::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    }
}

fn call(rt: &mut Runtime, id: &str, function: &str) -> ScriptCall {
    rt.call_script(id, PATH, function, &[])
}

/// Calls `function` and gives what it wrote in `Resultado`.
fn ask(rt: &mut Runtime, id: &str, function: &str) -> String {
    assert_eq!(call(rt, id, function), ScriptCall::Done, "{function}");
    result(rt, id)
}

fn failed(call: ScriptCall) -> ScriptError {
    match call {
        ScriptCall::Failed(error) => error,
        other => panic!("esperava um erro, veio {other:?}"),
    }
}

/// The message of the error `function` stops with.
fn refused(rt: &mut Runtime, id: &str, function: &str) -> String {
    failed(call(rt, id, function)).message
}

fn errors(rt: &Runtime) -> Vec<String> {
    rt.script_errors()
        .map(|(_, text, _)| text.to_owned())
        .collect()
}

const CHARACTER: &str = r#"local M = {}
function M.estado(self)
    local c = self.character
    local apoio = c.support
    self.attr.Resultado = string.format("%s %s %s %s %.1f %s", tostring(c.grounded),
        tostring(c.rising), c.posture, if apoio then apoio.name else "nil", c.speed,
        tostring(c.falling))
end
function M.olhar(self)
    local c = self.character
    self.attr.Resultado = string.format("%.2f %.2f %.2f | %.1f %.1f", c.aim.x, c.aim.y, c.aim.z,
        c.look.x, c.look.y)
end
function M.pular(self) self.character:jump() end
function M.agachar(self) self.character:set_posture("crouch", true) end
function M.levantar(self) self.character:set_posture("crouch", false) end
function M.empurrar(self)
    self.character:set_velocity(vector.create(3, 0, 0))
    self.character:add_velocity(vector.create(1, 0, 0))
    self.attr.Resultado = tostring(self.character.own_velocity.x)
end
function M.bloquear(self)
    self.character:block("movement", "diálogo", true)
    self.character:block("movement", "menu", true)
    self.character:block("movement", "menu", false)
    self.attr.Resultado = table.concat(self.character:block_reasons("movement"), ",") .. " "
        .. tostring(self.character.movement_blocked) .. " "
        .. tostring(#self.character:block_reasons("look"))
end
function M.perfil(self)
    self.character:apply_profile("agile")
    local agil = self.character:get_param("ground_acceleration")
    self.character:set_param("speed", 3)
    self.character:set_option("automatic_input", false)
    self.attr.Resultado = agil .. " " .. self.character:get_param("speed")
end
function M.girar(self) self.character:set_yaw(90) end
function M.teleportar(self)
    local ok, motivo, destino = self.character:teleport(vector.create(5, 1, 0), { keep_velocity = true })
    self.attr.Resultado = tostring(ok) .. " " .. tostring(motivo) .. " " .. tostring(destino)
end
function M.parede(self)
    local ok, motivo, destino = self.character:teleport(vector.create(10, 0, 0))
    self.attr.Resultado = tostring(ok) .. " " .. tostring(destino) .. " " .. motivo
end
function M.on_step(self)
    if self.attr.Andar then
        self.character:move(vector.create(0, 1, 0))
    end
end
function M.param_errado(self) self.character:set_param("sped", 3) end
function M.postura_errada(self) self.character:set_posture("deitar", true) end
function M.opcao_errada(self) self.character:teleport(vector.zero, { busca = 1 }) end
function M.membro_errado(self) return self.character.positon end
function M.escrever(self) self.character.position = vector.zero end
function M.metodo_errado(self) self.character:jmup() end
function M.de_outro(self) return scene.get("Caixa").character end
function M.deslizar(self) self.character:set_posture("slide", true) end
return M
"#;

#[test]
fn characters_read_their_state_aim_and_look_and_obey_their_commands() {
    let mut player = object("Jogador", None);
    player.character3d = Some(CharacterConfig::default());
    player.transform.position = [0., 1., 0.];
    player.attributes.insert("Andar".into(), Value::Bool(false));
    let id = player.id.clone();
    let wall = block("Parede", [10., 2., 0.], [2., 4., 2.]);
    let crate_ = Entity::new("Caixa", Some(Primitive::Cube));
    let project = world(vec![player, wall, crate_]);
    let mut rt = run(&project, CHARACTER);
    step(&mut rt, 60);
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    // On the floor, standing, still: the state of Ler personagem 3D.
    assert_eq!(
        ask(&mut rt, &id, "estado"),
        "true false standing Chão 0.0 false"
    );
    // The aim comes from the look of the simulated input: forward is -Z.
    assert_eq!(ask(&mut rt, &id, "olhar"), "0.00 0.00 -1.00 | 0.0 0.0");
    // Walking forward for a second moves the feet toward -Z.
    let z = rt.character_state(&id).unwrap().position.z;
    rt.set_attribute(&id, "Andar", Value::Bool(true)).unwrap();
    step(&mut rt, 60);
    rt.set_attribute(&id, "Andar", Value::Bool(false)).unwrap();
    assert!(rt.character_state(&id).unwrap().position.z < z - 1.);
    step(&mut rt, 60);
    // A jump leaves the floor in the next movement step.
    assert_eq!(call(&mut rt, &id, "pular"), ScriptCall::Done);
    step(&mut rt, 2);
    assert!(ask(&mut rt, &id, "estado").starts_with("false true standing nil"));
    step(&mut rt, 90);
    assert_eq!(call(&mut rt, &id, "agachar"), ScriptCall::Done);
    step(&mut rt, 2);
    assert!(ask(&mut rt, &id, "estado").contains(" crouched "));
    assert_eq!(call(&mut rt, &id, "levantar"), ScriptCall::Done);
    step(&mut rt, 2);
    // Velocities set and added at once, as Adicionar / definir velocidade.
    assert_eq!(ask(&mut rt, &id, "empurrar"), "4");
    step(&mut rt, 60);
    assert_eq!(ask(&mut rt, &id, "bloquear"), "diálogo true 0");
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    // Sliding asks for the slide the character allows.
    assert!(refused(&mut rt, &id, "deslizar").ends_with(
        "Jogador não pode deslizar: o deslize está desligado no personagem (ligue com \
         set_option(\"slide_enabled\", true))"
    ));
    // The profile writes what the editor's button writes; set_param and set_option last the run.
    let mut agile = CharacterConfig::default();
    agile.apply_profile(MovementProfile::Parkour);
    assert_eq!(
        ask(&mut rt, &id, "perfil"),
        format!("{} 3", agile.ground_acceleration)
    );
    let config = rt.scene().entity(&id).unwrap().character3d.clone().unwrap();
    assert!(!config.automatic_input && config.speed == 3.);
    assert_eq!(call(&mut rt, &id, "girar"), ScriptCall::Done);
    let yaw = rt.character_state(&id).unwrap().yaw;
    assert!((yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-4, "{yaw}");
    // A teleport with room gives ok and where the feet went; one into a wall, why not.
    assert_eq!(ask(&mut rt, &id, "teleportar"), "true nil 5, 1, 0");
    let refusal = ask(&mut rt, &id, "parede");
    assert!(refusal.starts_with("false nil "), "{refusal}");
    assert!(refusal.len() > "false nil ".len(), "{refusal}");
    // Wrong names are refused with the names there are.
    let line = CHARACTER
        .lines()
        .position(|l| l.contains("M.param_errado"))
        .unwrap()
        + 1;
    let error = failed(call(&mut rt, &id, "param_errado"));
    assert_eq!((error.path.as_str(), error.line as usize), (PATH, line));
    assert_eq!(
        error.message,
        format!(
            "set_param não conhece \"sped\"; quis dizer speed? Use {}",
            [
                "speed",
                "sprint_speed",
                "crouch_speed",
                "gravity",
                "jump_speed",
                "ground_acceleration",
                "ground_braking",
                "ground_friction",
                "air_acceleration",
                "air_projected_limit",
                "air_resistance",
                "horizontal_limit",
                "jump_retention",
                "landing_retention",
                "coyote_ms",
                "jump_buffer_ms",
                "slide_duration",
                "slide_friction",
                "slide_control",
                "angular_speed",
            ]
            .join(", ")
        )
    );
    assert!(
        refused(&mut rt, &id, "postura_errada")
            .ends_with("set_posture não conhece \"deitar\". Use sprint, crouch, slide")
    );
    assert!(
        refused(&mut rt, &id, "opcao_errada").ends_with(
            "teleport não conhece a opção busca; use search, keep_velocity, yaw ou look"
        )
    );
    assert!(
        refused(&mut rt, &id, "membro_errado")
            .ends_with("Character não tem 'positon'; quis dizer 'position'?")
    );
    assert!(refused(&mut rt, &id, "escrever").ends_with("obj.character.position é só leitura"));
    assert!(
        refused(&mut rt, &id, "metodo_errado")
            .ends_with("Character não tem 'jmup'; quis dizer 'jump'?"),
        "{}",
        refused(&mut rt, &id, "metodo_errado")
    );
    assert!(refused(&mut rt, &id, "de_outro").ends_with(
        "Caixa não tem Personagem 3D (confira antes com obj:has_component(\"character\"))"
    ));
}

const QUERIES: &str = r#"local M = {}
local function hit_text(hit, motivo)
    if hit == nil then
        return "nil " .. tostring(motivo)
    end
    local piso = if hit.surface then hit.surface.name else "padrão"
    return string.format("%s %.2f %.2f %.2f %.2f %s", hit.object.name, hit.distance, hit.point.y,
        hit.normal.y, hit.fraction, piso)
end
local DOWN = vector.create(0, -1, 0)
function M.raio(self)
    local partes = {
        hit_text(physics.raycast(self.world_position, DOWN, 10, { exclude = self })),
        hit_text(physics.raycast(self.world_position, vector.create(0, 1, 0), 10, { exclude = self })),
        hit_text(physics.raycast(self.world_position, DOWN, 10)),
        hit_text(physics.raycast(self.world_position, DOWN, 10, { exclude = self, areas = true })),
        hit_text(physics.raycast(self.world_position, DOWN, 10, { exclude = self, mask = "Personagens" })),
        hit_text(physics.raycast(self.world_position, DOWN, 10, { exclude = self, mask = { "Cenário padrão" }, category = 1 })),
        hit_text(physics.raycast(self.world_position, vector.zero, 10)),
    }
    self.attr.Resultado = table.concat(partes, " | ")
end
function M.formas(self)
    local esfera = physics.sphere_cast(self.world_position, 0.5, DOWN, 10, { exclude = self })
    local capsula = physics.capsule_cast(self.world_position, 0.3, 1.8, DOWN, 10, { exclude = self })
    local cabe = physics.capsule_fits(vector.create(-5, 3, -5), 0.3, 1.8)
    local nao, contato = physics.capsule_fits(vector.create(-5, 0, -5), 0.3, 1.8)
    self.attr.Resultado = string.format("%s %.2f | %s %.2f | %s | %s %s", esfera.object.name,
        esfera.distance, capsula.object.name, capsula.distance, tostring(cabe), tostring(nao),
        contato.object.name)
end
function M.grupo_errado(self) physics.raycast(self.world_position, DOWN, 10, { mask = "Inimigos" }) end
function M.distancia_errada(self) physics.raycast(self.world_position, DOWN, -1) end
function M.opcao_errada(self) physics.raycast(self.world_position, DOWN, 1, { ignorar = self }) end
function M.origem_errada(self) physics.raycast("aqui", DOWN, 1) end
return M
"#;

#[test]
fn physics_queries_hit_miss_refuse_and_name_collision_groups() {
    let mut eye = object("Olho", Some(Primitive::Cube));
    eye.transform.position = [0., 5., 0.];
    eye.physics3d = Some(Collider3d::default());
    let id = eye.id.clone();
    let mut sensor = block("Sensor", [0., 2., 0.], [1., 1., 1.]);
    sensor.physics3d.as_mut().unwrap().sensor = true;
    let mut project = world(vec![eye, sensor]);
    project
        .surfaces
        .push(SurfaceMaterial::preset(SurfacePreset::Ice));
    let ice = project.surfaces[0].id.clone();
    project.scenes[0].entities[0]
        .physics3d
        .as_mut()
        .unwrap()
        .surface = Some(ice);
    let mut rt = run(&project, QUERIES);
    step(&mut rt, 1);
    let found = ask(&mut rt, &id, "raio");
    let parts: Vec<&str> = found.split(" | ").collect();
    assert_eq!(
        parts,
        [
            // Down to the floor, its surface named; up, nothing.
            "Chão 5.00 0.00 1.00 0.50 Gelo",
            "nil nil",
            // Without exclude the ray starts inside the eye and hits it at once.
            "Olho 0.00 5.00 0.00 0.00 padrão",
            // Areas only with areas = true; groups by name or as a mask.
            "Sensor 2.50 2.50 1.00 0.25 padrão",
            "nil nil",
            "Chão 5.00 0.00 1.00 0.50 Gelo",
            // A zero direction is a refused query: nil and why.
            "nil Direção da consulta não pode ser zero.",
        ],
        "{found}"
    );
    assert_eq!(
        ask(&mut rt, &id, "formas"),
        "Chão 4.50 | Chão 4.10 | true | false Chão"
    );
    assert!(
        refused(&mut rt, &id, "grupo_errado")
            .contains("physics.raycast: o grupo de colisão \"Inimigos\" não existe"),
        "{}",
        refused(&mut rt, &id, "grupo_errado")
    );
    assert!(
        refused(&mut rt, &id, "distancia_errada")
            .ends_with("physics.raycast: distance é um número de metros, 0 ou mais; recebeu -1")
    );
    assert!(refused(&mut rt, &id, "opcao_errada").ends_with(
        "physics.raycast não conhece a opção ignorar; use areas, exclude, category ou mask"
    ));
    assert!(refused(&mut rt, &id, "origem_errada").ends_with(
        "physics.raycast: origin é um ponto do mundo, um vetor como vector.create(0, 1, 0), não \
         texto"
    ));
    // In a 2D scene a query is refused with why, not an error.
    let mut flat = project.clone();
    flat.scenes[0].kind = SceneKind::TwoD;
    for e in &mut flat.scenes[0].entities {
        e.physics3d = None;
    }
    let mut rt = run(&flat, QUERIES);
    step(&mut rt, 1);
    assert!(
        ask(&mut rt, &id, "raio").starts_with("nil Esta consulta exige cena 3D. | "),
        "{}",
        result(&rt, &id)
    );
}

const VIEWS: &str = r#"local M = {}
function M.colisor(self)
    local antes = self.collider.surface
    self.collider.enabled = false
    local desligado = self.collider.enabled
    self.collider.surface = "gelo"
    local gelo = self.collider.surface.name
    self.collider.surface = nil
    local nada = self.collider.surface
    self.collider.surface = scene.get("Pedra").collider.surface
    self.collider.enabled = true
    self.attr.Resultado = string.format("%s %s %s %s %s %s", tostring(antes), tostring(desligado),
        gelo, tostring(nada), self.collider.surface.name, tostring(self.collider.is_area))
end
function M.area(self)
    self.attr.Resultado = tostring(scene.get("Sensor").collider.is_area)
end
function M.superficie_errada(self) self.collider.surface = "Granito" end
function M.superficie_2d(self) return scene.get("Caixa 2D").collider.surface end
function M.corpo(self)
    local heroi = scene.get("Herói").character
    local antes = heroi.body_surface
    heroi.body_surface = "gelo"
    local gelo = heroi.body_surface.name
    heroi.body_surface = scene.get("Pedra").collider.surface
    self.attr.Resultado = string.format("%s %s %s", tostring(antes), gelo,
        heroi.body_surface.name)
end
function M.corpo_errado(self) scene.get("Herói").character.body_surface = "Granito" end
function M.plataforma(self)
    local p = scene.get("Elevador").platform
    p.enabled = false
    self.attr.Resultado = tostring(p.enabled)
end
function M.controle(self)
    local c = scene.get("Caixa 2D").controller
    c.enabled = false
    self.attr.Resultado = tostring(c.enabled)
end
function M.animar(self)
    local a = scene.get("Portão").animation
    local antes = tostring(a.clip) .. " " .. tostring(a.playing)
    a:play("abrir")
    local tocando = a.clip .. " " .. tostring(a.playing)
    a.playing = false
    self.attr.Resultado = antes .. " | " .. tocando .. " | " .. tostring(a.playing)
end
function M.clipe_errado(self) scene.get("Portão").animation:play("Fechar") end
function M.texto(self)
    local ui = scene.get("Placar").ui
    ui.text = "Tempo: " .. time.format(65, "mm:ss")
    ui.color = vector.create(1, 0.5, 0)
    self.attr.Resultado = ui.text .. " " .. tostring(ui.color)
end
function M.cor_errada(self) scene.get("Placar").ui.color = vector.create(2, 0, 0) end
function M.texto_errado(self) scene.get("Placar").ui.text = 3 end
function M.sem_interface(self) return self.ui end
function M.sem_animacao(self) return self.animation end
function M.sem_plataforma(self) return self.platform end
function M.sem_controle(self) return self.controller end
function M.sem_colisor(self) return scene.get("Placar").collider end
return M
"#;

#[test]
fn colliders_platforms_controllers_animations_and_interface_switch_through_their_views() {
    let mut door = object("Porta", Some(Primitive::Cube));
    door.transform.position = [0., 1., 0.];
    door.physics3d = Some(Collider3d::default());
    let id = door.id.clone();
    let mut sensor = block("Sensor", [3., 1., 0.], [1., 1., 1.]);
    sensor.physics3d.as_mut().unwrap().sensor = true;
    let mut stone = block("Pedra", [6., 1., 0.], [1., 1., 1.]);
    let mut lift = block("Elevador", [9., 1., 0.], [1., 1., 1.]);
    lift.platform = Some(TranslationPlatform::default());
    let mut flat = Entity::new("Caixa 2D", Some(Primitive::Cube));
    flat.collider = Some(Collider::default());
    flat.controller = Some(Controller::default());
    let mut gate = Entity::new("Portão", Some(Primitive::Cube));
    gate.clips.push(Clip::new("Abrir"));
    let gate_id = gate.id.clone();
    let mut board = Entity::new("Placar", None);
    board.ui = Some(UiElement::default());
    let board_id = board.id.clone();
    let mut hero = Entity::new("Herói", None);
    hero.character3d = Some(CharacterConfig::default());
    hero.transform.position = [12., 0.02, 0.];
    let hero_id = hero.id.clone();
    let mut project = world(vec![]);
    for preset in [SurfacePreset::Ice, SurfacePreset::Mud] {
        project.surfaces.push(SurfaceMaterial::preset(preset));
    }
    stone.physics3d.as_mut().unwrap().surface = Some(project.surfaces[1].id.clone());
    let rubber = project.surfaces[1].name.clone();
    project.scenes[0]
        .entities
        .extend([door, sensor, stone, lift, flat, gate, board, hero]);
    let mut rt = run(&project, VIEWS);
    step(&mut rt, 1);
    assert_eq!(
        ask(&mut rt, &id, "colisor"),
        format!("nil false {} nil {rubber} false", project.surfaces[0].name)
    );
    assert!(
        rt.scene()
            .entity(&id)
            .unwrap()
            .physics3d
            .as_ref()
            .unwrap()
            .enabled
    );
    assert_eq!(ask(&mut rt, &id, "area"), "true");
    assert!(
        refused(&mut rt, &id, "superficie_errada")
            .contains("collider.surface = \"Granito\" nomeia uma superfície que o projeto não tem")
    );
    assert!(refused(&mut rt, &id, "superficie_2d").ends_with(
        "o colisor de Caixa 2D é 2D e não tem superfície: só um colisor 3D tem (a do corpo de \
             um personagem é obj.character.body_surface)"
    ));
    // The surface of a character's body, which Ler / Aplicar superfície ao colisor read and
    // write on a character, and not the one under its feet.
    assert_eq!(
        ask(&mut rt, &id, "corpo"),
        format!("nil {} {rubber}", project.surfaces[0].name)
    );
    let body = &rt.scene().entity(&hero_id).unwrap().character3d;
    assert_eq!(
        body.as_ref().unwrap().body.surface.as_ref(),
        Some(&project.surfaces[1].id)
    );
    assert_eq!(
        rt.object_surface(&hero_id).unwrap().as_ref(),
        Some(&project.surfaces[1].id)
    );
    assert!(refused(&mut rt, &id, "corpo_errado").contains(
        "character.body_surface = \"Granito\" nomeia uma superfície que o projeto não tem"
    ));
    assert_eq!(ask(&mut rt, &id, "plataforma"), "false");
    assert_eq!(ask(&mut rt, &id, "controle"), "false");
    // A clip by its name in any case, as Reproduzir animação plays it; false pauses it.
    assert_eq!(
        ask(&mut rt, &id, "animar"),
        "nil false | Abrir true | false"
    );
    assert_eq!(
        rt.animation_playing(&gate_id).map(|(_, on)| on),
        Some(false)
    );
    assert!(
        refused(&mut rt, &id, "clipe_errado")
            .ends_with("Portão não tem o clipe \"Fechar\". Clipes: Abrir"),
        "{}",
        refused(&mut rt, &id, "clipe_errado")
    );
    // The text and the color of an interface element, new in 0.7.
    assert_eq!(ask(&mut rt, &id, "texto"), "Tempo: 01:05 1, 0.5, 0");
    let ui = rt.scene().entity(&board_id).unwrap().ui.clone().unwrap();
    assert_eq!(ui.text, "Tempo: 01:05");
    assert_eq!(ui.color[..3], [1., 0.5, 0.]);
    assert!(
        refused(&mut rt, &id, "cor_errada")
            .ends_with("A cor precisa de vermelho, verde e azul de 0 a 1; recebeu (2, 0, 0)")
    );
    assert!(
        refused(&mut rt, &id, "texto_errado").ends_with("obj.ui.text recebe um texto, não número")
    );
    for (function, message) in [
        (
            "sem_interface",
            "Porta não é um elemento de interface (texto, imagem, botão ou barra) (confira antes \
             com obj:has_component(\"ui\"))",
        ),
        (
            "sem_animacao",
            "Porta não tem animações (confira antes com obj:has_component(\"animation\"))",
        ),
        (
            "sem_plataforma",
            "Porta não tem plataforma móvel (confira antes com obj:has_component(\"platform\"))",
        ),
        (
            "sem_controle",
            "Porta não tem Movimento 2D (confira antes com obj:has_component(\"controller\"))",
        ),
        (
            "sem_colisor",
            "Placar não tem colisor nem área (confira antes com obj:has_component(\"collider\"))",
        ),
    ] {
        let said = refused(&mut rt, &id, function);
        assert!(said.ends_with(message), "{function}: {said}");
    }
}

const CAMERAS: &str = r#"local M = {}
function M.ler(self)
    local ativa = camera.active()
    self.attr.Resultado = string.format("%s %s %.2f %.1f", ativa.name, camera.mode(),
        camera.forward().z, camera.look().y)
end
function M.modos(self)
    camera.set_mode(self, "first_person", 0)
    local primeira = camera.mode()
    camera.set(self, "fov", 75, 0)
    camera.set(self, "distance", 6)
    camera.follow(self, scene.get("Outro"))
    camera.look_at(self, vector.create(0, 0, -10))
    camera.look_at(self, scene.get("Outro"))
    camera.release_look(self)
    camera.activate(scene.get("Vista"), 0)
    self.attr.Resultado = primeira .. " " .. camera.active().name .. " " .. camera.mode()
end
function M.sem_camera(self) camera.activate(scene.get("Outro")) end
function M.sem_controle(self) camera.set_mode(scene.get("Vista"), "third_person") end
function M.inativa(self) camera.look_at(self, vector.zero) end
function M.modo_errado(self) camera.set_mode(self, "terceira") end
function M.ajuste_errado(self) camera.set(self, "zoom", 2) end
function M.a_si_mesma(self) camera.follow(self, self) end
function M.segundos(self) camera.activate(self, -1) end
return M
"#;

#[test]
fn the_camera_reads_and_commands_through_the_nodes_operations() {
    let mut player = Entity::new("Jogador", None);
    player.character3d = Some(CharacterConfig::default());
    player.transform.position[1] = 0.02;
    let mut other = Entity::new("Outro", None);
    other.character3d = Some(CharacterConfig::default());
    other.transform.position = [4., 0.02, 0.];
    let mut cam = object("Câmera", None);
    cam.camera = Some(Camera::default());
    cam.camera_rig = Some(CameraRig {
        mode: CameraMode::ThirdPerson,
        target: Some(player.id.clone()),
        ..Default::default()
    });
    let id = cam.id.clone();
    let mut view = Entity::new("Vista", None);
    view.camera = Some(Camera {
        active: false,
        ..Default::default()
    });
    let view_id = view.id.clone();
    let project = world(vec![player, other, cam, view]);
    let mut rt = run(&project, CAMERAS);
    step(&mut rt, 2);
    let read = ask(&mut rt, &id, "ler");
    assert!(read.starts_with("Câmera third_person -"), "{read}");
    assert_eq!(ask(&mut rt, &id, "modos"), "first_person Vista fixed");
    let e = rt.scene().entity(&id).unwrap();
    assert_eq!(e.camera.as_ref().unwrap().fov, 75.);
    assert_eq!(e.camera_rig.as_ref().unwrap().distance, 6.);
    assert_eq!(rt.active_camera(), Some(view_id.as_str()));
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    for (function, message) in [
        ("sem_camera", "camera.activate: Outro não tem Câmera"),
        (
            "sem_controle",
            "camera.set_mode: Vista não tem Controle de câmera (só uma câmera de personagem troca \
             de modo, acompanha e olha para um alvo)",
        ),
        (
            "inativa",
            "camera.look_at: Câmera não é a câmera ativa; ative-a antes (camera.activate)",
        ),
        (
            "modo_errado",
            "camera.set_mode não conhece o modo \"terceira\". Modos: first_person, third_person, \
             fixed, fixed_object",
        ),
        (
            "ajuste_errado",
            "camera.set não conhece o ajuste \"zoom\". Ajustes: fov, distance, shoulder",
        ),
        (
            "a_si_mesma",
            "camera.follow: uma câmera não acompanha a si mesma",
        ),
        (
            "segundos",
            "camera.activate: seconds não pode ser negativo; recebeu -1",
        ),
    ] {
        let said = refused(&mut rt, &id, function);
        assert!(said.ends_with(message), "{function}: {said}");
    }
}

const LOOK: &str = r#"local M = {}
M.state = { x = 0, y = 0 }
function M.on_step(self)
    self.state.x += input.look().x
    self.state.y += input.look().y
end
function M.ler(self)
    local c = self.character
    local f = camera.forward()
    local l = camera.look()
    self.attr.Resultado = string.format("%.4f %.4f %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f %.6f",
        self.state.x, self.state.y, c.aim.x, c.aim.y, c.aim.z, c.look.x, c.look.y, f.x, f.y, f.z,
        l.x, l.y)
end
function M.voltar(self)
    local ok = self.character:teleport(self.character.position, { look = vector.create(90, 10, 0) })
    self.attr.Resultado = string.format("%s %.4f %.4f", tostring(ok), self.character.look.x,
        self.character.look.y)
end
return M
"#;

/// What `ler` of [`LOOK`] wrote: the look input summed, the aim, the character's look, the
/// camera's forward and its look.
fn looks(rt: &mut Runtime, id: &str) -> Vec<f32> {
    ask(rt, id, "ler")
        .split(' ')
        .map(|n| n.parse().unwrap())
        .collect()
}

#[test]
fn the_look_comes_from_the_input_through_the_camera_and_teleport_takes_it_back() {
    use glam::{Quat, Vec3};
    let mut player = object("Jogador", None);
    player.character3d = Some(CharacterConfig::default());
    player.transform.position = [0., 0.02, 0.];
    let id = player.id.clone();
    let mut cam = Entity::new("Câmera", None);
    cam.camera = Some(Camera::default());
    cam.camera_rig = Some(CameraRig {
        mode: CameraMode::ThirdPerson,
        target: Some(id.clone()),
        ..Default::default()
    });
    let project = world(vec![player, cam]);
    let mut rt = run(&project, LOOK);
    step(&mut rt, 2);
    // The mouse moves with the cursor hidden, as in a game.
    assert!(!rt.cursor_shown());
    let frame = InputFrame {
        look: [40., -20.],
        ..Default::default()
    };
    for _ in 0..5 {
        rt.advance(FIXED_DT, &frame);
    }
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    let read = looks(&mut rt, &id);
    // input.look() is what the device moved in each step.
    assert_eq!(read[..2], [200., -100.]);
    // The aim and the look come from the yaw and the pitch the input gave the character.
    let state = rt.character_state(&id).unwrap().clone();
    assert!(state.look_yaw != 0. && state.pitch != 0., "{state:?}");
    let aim = Quat::from_rotation_y(state.look_yaw) * Quat::from_rotation_x(state.pitch) * -Vec3::Z;
    let near = |a: &[f32], b: &[f32], by: f32| a.iter().zip(b).all(|(a, b)| (a - b).abs() < by);
    assert!(near(&read[2..5], &aim.to_array(), 1e-5), "{read:?} {aim}");
    let turn = (state.look_yaw.to_degrees() + 180.).rem_euclid(360.) - 180.;
    assert!(
        near(&read[5..7], &[turn, state.pitch.to_degrees()], 1e-3),
        "{read:?} {state:?}"
    );
    // The camera that follows the character looks the same way, and its look is its forward's.
    let forward = Vec3::from_slice(&read[7..10]);
    assert!(near(&read[7..10], &aim.to_array(), 1e-5), "{read:?}");
    let from_forward = [
        (-forward.x).atan2(-forward.z).to_degrees(),
        forward.y.asin().to_degrees(),
    ];
    assert!(near(&read[10..12], &from_forward, 1e-3), "{read:?}");
    assert!(near(&read[10..12], &read[5..7], 1e-3), "{read:?}");
    // A look read from the character and given to teleport turns it back to it, camera included.
    assert_eq!(ask(&mut rt, &id, "voltar"), "true 90.0000 10.0000");
    step(&mut rt, 1);
    let read = looks(&mut rt, &id);
    assert!(near(&read[5..7], &[90., 10.], 1e-3), "{read:?}");
    assert!(near(&read[10..12], &[90., 10.], 1e-3), "{read:?}");
}

const SOUNDS: &str = r#"local M = {}
function M.tocar(self)
    audio.loop("Trilha", { volume = 0.5, fade = 1 })
    audio.loop("Vento", { channel = "effects" })
    audio.set_volume("music", 0.25)
    audio.set_volume("master", 3)
    self.attr.Resultado = audio.volume("music") .. " " .. audio.volume("master") .. " "
        .. audio.volume("effects")
end
function M.parar(self)
    audio.stop({ sound = "Vento", channel = "effects" })
    audio.stop({ channel = "music", fade = 2 })
end
function M.canal_errado(self) audio.loop("Trilha", { channel = "ambiente" }) end
function M.opcao_errada(self) audio.stop({ som = "Vento" }) end
function M.fade_errado(self) audio.stop({ fade = 61 }) end
function M.som_errado(self) audio.loop("Trilhaa") end
function M.volume_errado(self) audio.set_volume("geral", 1) end
return M
"#;

#[test]
fn sound_loops_stops_and_volumes_go_through_the_nodes_operations() {
    let mut player = object("Jogador", None);
    player.scripts = vec![ScriptRef::new(PATH)];
    let id = player.id.clone();
    let mut project = Project::new("Sons");
    let mut sound = |name: &str| {
        let asset = Asset {
            id: new_id(),
            name: name.into(),
            path: format!("assets/{name}.wav"),
            kind: AssetKind::Audio,
            model: None,
        };
        let id = asset.id.clone();
        project.assets.push(asset);
        id
    };
    let (track, wind) = (sound("Trilha"), sound("Vento"));
    project.scenes[0].entities.push(player);
    let mut rt = run(&project, SOUNDS);
    step(&mut rt, 1);
    // Volumes are kept within 0 and 1, as Volume do canal keeps them.
    assert_eq!(ask(&mut rt, &id, "tocar"), "0.25 1 1");
    let audio = rt.audio();
    let music = audio.music.as_ref().unwrap();
    assert_eq!(
        (music.asset.as_str(), music.volume, music.fade),
        (track.as_str(), 0.5, 1.)
    );
    assert!(audio.loops.contains_key(&wind));
    assert_eq!(call(&mut rt, &id, "parar"), ScriptCall::Done);
    assert!(rt.audio().music.is_none() && rt.audio().loops.is_empty());
    for (function, message) in [
        (
            "canal_errado",
            "audio.loop: channel é \"music\" ou \"effects\", não \"ambiente\"",
        ),
        (
            "opcao_errada",
            "audio.stop não conhece a opção som; use sound, channel ou fade",
        ),
        (
            "fade_errado",
            "audio.stop: fade vai de 0 a 60 segundos; recebeu 61",
        ),
        (
            "som_errado",
            "o som \"Trilhaa\" não existe no projeto; sons: Trilha, Vento",
        ),
        (
            "volume_errado",
            "audio.set_volume: o canal é \"master\", \"music\" ou \"effects\", não \"geral\"",
        ),
    ] {
        let said = refused(&mut rt, &id, function);
        assert!(said.ends_with(message), "{function}: {said}");
    }
}

const GAME: &str = r#"local M = {}
M.state = { lido = "" }
function M.on_step(self)
    local partes = {}
    for _, acao in { "Pular", "atacar" } do
        table.insert(partes, string.format("%s:%s%s%s", acao, tostring(input.pressed(acao)),
            tostring(input.held(acao)), tostring(input.released(acao))))
    end
    local eixo = input.movement()
    table.insert(partes, string.format("%.0f,%.0f %.0f,%.0f %.0f", eixo.x, eixo.y,
        input.look().x, input.look().y, input.wheel()))
    self.state.lido = table.concat(partes, " ")
end
function M.lido(self) self.attr.Resultado = self.state.lido end
function M.pausar(self)
    game.pause()
    local pausado = game.paused()
    game.show_cursor()
    self.attr.Resultado = tostring(pausado)
end
function M.retomar(self)
    game.resume()
    game.hide_cursor()
end
function M.salvar(self)
    local antes = save.exists()
    vars.set("Moedas", 7)
    save.write()
    local depois = save.exists()
    save.clear()
    self.attr.Resultado = tostring(antes) .. " " .. tostring(depois) .. " "
        .. tostring(vars.get("Moedas")) .. " " .. tostring(save.exists())
end
function M.sair(self) game.quit() end
function M.acao_errada(self) input.pressed("Pulsar") end
function M.acao_dupla(self) input.pressed(" esquivar ") end
function M.jogador(self) input.held("Pular", self) end
function M.jogador_eixo(self) input.movement(self) end
return M
"#;

#[test]
fn the_game_pause_cursor_quitting_saved_progress_and_input_read_and_act() {
    let player = object("Jogador", None);
    let id = player.id.clone();
    let mut project = Project::new("Jogo");
    variables::create(&mut project, "Moedas", Value::Number(0.), true).unwrap();
    for key in ["Q", "E"] {
        input_actions::create(&mut project, "Esquivar", key).unwrap();
    }
    project.scenes[0].entities.push(player);
    let mut rt = run(&project, GAME);
    let frame = |pressed: &[&str], held: &[&str], released: &[&str]| InputFrame {
        pressed: pressed.iter().map(|s| (*s).to_owned()).collect(),
        held: held.iter().map(|s| (*s).to_owned()).collect(),
        released: released.iter().map(|s| (*s).to_owned()).collect(),
        look: [3., -2.],
        wheel: 1.,
        ..Default::default()
    };
    rt.advance(
        FIXED_DT,
        &frame(&["pular"], &["pular", "mover_frente"], &[]),
    );
    assert_eq!(
        ask(&mut rt, &id, "lido"),
        // Without a camera that turns with the mouse the look stays 0, as for the node.
        "Pular:truetruefalse atacar:falsefalsefalse 0,1 0,0 1"
    );
    rt.advance(
        FIXED_DT,
        &frame(&[], &["atacar", "mover_direita"], &["pular"]),
    );
    assert_eq!(
        ask(&mut rt, &id, "lido"),
        "Pular:falsefalsetrue atacar:falsetruefalse 1,0 0,0 1"
    );
    // The game pause and the cursor, as Pausar / retomar jogo and Mostrar / esconder cursor.
    assert_eq!(ask(&mut rt, &id, "pausar"), "true");
    assert!(rt.game_paused() && rt.cursor_shown());
    assert_eq!(call(&mut rt, &id, "retomar"), ScriptCall::Done);
    assert!(!rt.game_paused() && !rt.cursor_shown());
    // Saved progress: written, then cleared back to the declared value.
    assert_eq!(ask(&mut rt, &id, "salvar"), "false true 0 false");
    assert!(!rt.quit_requested);
    assert_eq!(call(&mut rt, &id, "sair"), ScriptCall::Done);
    assert!(rt.quit_requested);
    let said = refused(&mut rt, &id, "acao_errada");
    assert!(
        said.contains("input.pressed: a ação de entrada \"Pulsar\" não existe no projeto; quis dizer Pular? Ações: "),
        "{said}"
    );
    assert!(!said.contains("mover_esquerda"), "{said}");
    // Two actions sharing a name are told apart by renaming one, never by their IDs.
    assert_eq!(
        refused(&mut rt, &id, "acao_dupla"),
        "input.pressed: há 2 ações de entrada chamadas \" esquivar \"; renomeie uma delas em \
         Lógica > Ações de entrada"
    );
    for function in ["jogador", "jogador_eixo"] {
        let said = refused(&mut rt, &id, function);
        assert!(
            said.contains("o argumento player chega na 0.8, com a rede e vários jogadores"),
            "{said}"
        );
    }
}

const TIME: &str = r#"local M = {}
local ONDAS = random.stream("ondas")
local TEXTO = time.format(3725, "h:mm:ss")
function M.tempo(self)
    self.attr.Resultado = string.format("%.4f %.4f %s %s %s", time.now(), time.dt, TEXTO,
        time.format(75.5, "mm:ss.cc"), time.format(-1.25, "ss.cc"))
end
function M.sortear(self)
    local a = random.number(0, 1)
    local b = random.integer(1, 6)
    local c = ONDAS:integer(1, 1000)
    self.attr.Resultado = string.format("%.6f %d %d", a, b, c)
end
function M.so_ondas(self)
    ONDAS:number(0, 1)
    ONDAS:number(0, 1)
end
function M.semente(self) random.seed(7) end
function M.formato_errado(self) time.format(1, "mm:s") end
function M.faixa_errada(self) random.number(2, 1) end
function M.semente_errada(self) random.seed(1.5) end
function M.nome_errado(self) random.stream("") end
return M
"#;

#[test]
fn time_and_draws_follow_the_engine_and_streams_keep_apart() {
    let a = object("A", None);
    let b = object("B", None);
    let (a_id, b_id) = (a.id.clone(), b.id.clone());
    let mut project = Project::new("Tempo");
    project.scenes[0].entities.extend([a, b]);
    let mut rt = run(&project, TIME);
    step(&mut rt, 30);
    assert_eq!(
        ask(&mut rt, &a_id, "tempo"),
        "0.5000 0.0167 1:02:05 01:15.50 -01.25"
    );
    // The same draws for the same seed; a stream drawn more does not move the script's own.
    let draws = |rt: &mut Runtime, extra: bool| {
        assert_eq!(call(rt, &a_id, "semente"), ScriptCall::Done);
        if extra {
            assert_eq!(call(rt, &a_id, "so_ondas"), ScriptCall::Done);
        }
        let first = ask(rt, &a_id, "sortear");
        (first, ask(rt, &b_id, "sortear"))
    };
    let plain = draws(&mut rt, false);
    let again = draws(&mut rt, false);
    assert_eq!(plain, again);
    let streamed = draws(&mut rt, true);
    let own = |text: &str| text.rsplit_once(' ').unwrap().0.to_owned();
    assert_eq!(own(&streamed.0), own(&plain.0));
    assert_ne!(streamed.0, plain.0);
    // Each object draws its own numbers.
    assert_ne!(plain.0, plain.1);
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    for (function, message) in [
        (
            "formato_errado",
            "time.format não conhece o formato \"mm:s\"; quis dizer mm:ss? Formatos: mm:ss, \
             mm:ss.cc, m:ss.cc, ss.cc, mm:ss.mmm, h:mm:ss",
        ),
        (
            "faixa_errada",
            "random.number: Mínimo 2 maior que o máximo 1",
        ),
        (
            "semente_errada",
            "random.seed: A semente precisa ser um inteiro de 0 a 9007199254740991; recebeu 1.5",
        ),
        (
            "nome_errado",
            "random.stream: o nome da sequência tem de 1 a 256 bytes",
        ),
    ] {
        let said = refused(&mut rt, &a_id, function);
        assert!(said.ends_with(message), "{function}: {said}");
    }
}

const LATER: &str = r#"local M = {}
function M.jogadores(self) return players end
function M.jogador(self) return Player end
function M.sessao(self) return session end
function M.online(self) return game.online end
function M.hospeda(self) return game.is_host end
function M.servidor(self) return game.is_server end
function M.on_player_join(self, player) end
return M
"#;

#[test]
fn the_names_of_0_8_say_they_come_in_0_8() {
    let player = object("Jogador", None);
    let id = player.id.clone();
    let mut project = Project::new("0.8");
    project.scenes[0].entities.push(player);
    let mut rt = run(&project, LATER);
    step(&mut rt, 1);
    // A callback of 0.8 is told once: the engine does not call it yet.
    assert_eq!(
        errors(&rt),
        [
            "scripts/teste.luau:8: on_player_join chega na 0.8, com a rede e vários jogadores: na \
          0.7 a engine nunca chama essa função."
        ]
    );
    for (function, name) in [
        ("jogadores", "players"),
        ("jogador", "Player"),
        ("sessao", "session"),
        ("online", "game.online"),
        ("hospeda", "game.is_host"),
        ("servidor", "game.is_server"),
    ] {
        let said = refused(&mut rt, &id, function);
        assert!(
            said.ends_with(&format!(
                "{name} chega na 0.8, com a rede e vários jogadores; na 0.7 ainda não existe"
            )),
            "{function}: {said}"
        );
    }
    let sources = ScriptSources::from_texts([(PATH, LATER)]);
    let report = &script::check_loading(&sources, [PATH], |_| true)[0];
    assert_eq!(report.warnings.len(), 1, "{report:?}");
    // A near name still points at the callback of 0.8.
    let near = "local M = {}\nfunction M.on_player_jion(self) end\nreturn M\n";
    let sources = ScriptSources::from_texts([(PATH, near)]);
    let report = &script::check_loading(&sources, [PATH], |_| true)[0];
    assert!(
        report.warnings[0]
            .message
            .ends_with("quis dizer on_player_join?"),
        "{report:?}"
    );
}

const VALUES: &str = r#"local M = {}
M.state = { guardado = param.object() }
M.attributes = { Alvo = param.object() }
function M.texto(self)
    self.attr.Resultado = tostring(self.character) .. " " .. tostring(random.stream("cartas"))
        .. " " .. typeof(self.character)
end
function M.na_memoria(self) self.state.guardado = self.character end
function M.no_atributo(self) self.attr.Alvo = self.character end
function M.medir(self) return #self.character end
function M.sequencia_guardada(self) self:after(1, M.texto, random.stream("cartas")) end
function M.depois_de_sair(self)
    local c = scene.get("Outro").character
    scene.get("Outro"):destroy()
    return c.position
end
return M
"#;

#[test]
fn views_are_values_that_memory_attributes_and_timers_refuse() {
    let mut player = object("Jogador", None);
    player.character3d = Some(CharacterConfig::default());
    player.attributes.insert("Alvo".into(), Value::Object(None));
    let id = player.id.clone();
    let mut other = Entity::new("Outro", None);
    other.character3d = Some(CharacterConfig::default());
    other.transform.position = [3., 0.02, 0.];
    let project = world(vec![player, other]);
    let mut rt = run(&project, VALUES);
    step(&mut rt, 1);
    assert_eq!(
        ask(&mut rt, &id, "texto"),
        "Jogador.character random.stream(\"cartas\") Object"
    );
    for (function, message) in [
        (
            "na_memoria",
            "self.state.guardado: guarda objeto, não o personagem 3D de um objeto",
        ),
        (
            "no_atributo",
            "o atributo Alvo guarda objeto, não o personagem 3D de um objeto",
        ),
        ("medir", "o personagem 3D de um objeto não tem tamanho (#)"),
        ("depois_de_sair", "o objeto não existe mais"),
    ] {
        let said = refused(&mut rt, &id, function);
        assert!(said.ends_with(message), "{function}: {said}");
    }
    let said = refused(&mut rt, &id, "sequencia_guardada");
    assert!(said.contains("uma sequência de sorteios"), "{said}");
}

/// A 3D project whose scripts use every area: a player walking and jumping by input, a camera
/// following it, a gate animated by a marker of time, a scoreboard with the time, sounds,
/// physics queries, draws, the pause and saved progress.
fn every_area() -> (Project, Arc<ScriptSources>) {
    let mut player = object("Jogador", None);
    player.character3d = Some(CharacterConfig::default());
    player.transform.position = [0., 0.02, 0.];
    player.attributes.insert("Pontos".into(), Value::Number(0.));
    player.scripts = vec![ScriptRef::new("scripts/jogador.luau")];
    let mut cam = Entity::new("Câmera", None);
    cam.camera = Some(Camera::default());
    cam.camera_rig = Some(CameraRig {
        target: Some(player.id.clone()),
        ..Default::default()
    });
    cam.scripts = vec![ScriptRef::new("scripts/camera.luau")];
    let mut gate = block("Portão", [0., 1., -8.], [3., 2., 0.5]);
    gate.clips.push(Clip::new("Abrir"));
    gate.platform = Some(TranslationPlatform::default());
    gate.scripts = vec![ScriptRef::new("scripts/portao.luau")];
    let mut board = Entity::new("Placar", None);
    board.ui = Some(UiElement::default());
    board.scripts = vec![ScriptRef::new("scripts/placar.luau")];
    let mut project = world(vec![player, cam, gate, board]);
    variables::create(&mut project, "Recorde", Value::Number(0.), true).unwrap();
    project.assets.push(Asset {
        id: new_id(),
        name: "Trilha".into(),
        path: "assets/trilha.wav".into(),
        kind: AssetKind::Audio,
        model: None,
    });
    let player = r#"local M = {}
M.state = { pulos = 0, proximo = 0 }
local SORTE = random.stream("sorte")
function M.on_step(self)
    self.character:move(input.movement())
    if input.pressed("Pular") and self.character.grounded then
        self.character:jump()
        self.state.pulos += 1
    end
    local hit = physics.raycast(self.character.position + vector.create(0, 1, 0),
        self.character.aim, 20, { exclude = self })
    if hit and time.now() >= self.state.proximo then
        self.state.proximo = time.now() + 0.5
        self.attr.Pontos += SORTE:integer(1, 3) + random.integer(0, 1)
        self:send("ponto", hit.object.name)
    end
    if self.character.position.y < -5 then
        self.character:teleport(vector.create(0, 1, 0), { search = 1 })
    end
end
function M.on_message(self, name, value, from)
    vars.set("Recorde", math.max(vars.get("Recorde"), self.attr.Pontos))
    save.write()
end
return M
"#;
    let camera = r#"local M = {}
function M.on_start(self)
    camera.set(self, "fov", 70, 0)
    self:after(1, M.primeira)
end
function M.primeira(self)
    camera.set_mode(self, "first_person", 0.2)
end
return M
"#;
    let gate = r#"local M = {}
function M.on_start(self)
    audio.loop("Trilha", { fade = 0.5 })
    self:every(0.75, M.alternar)
end
function M.alternar(self)
    self.animation:play("Abrir")
    self.platform.enabled = not self.platform.enabled
    self.collider.enabled = physics.capsule_fits(vector.create(0, 2, -4), 0.3, 1.8)
    if random.number(0, 1) < 0.3 then
        game.pause()
        game.resume()
    end
end
return M
"#;
    let board = r#"local M = {}
function M.on_step(self)
    local jogador = scene.get("Jogador")
    self.ui.text = time.format(time.now(), "mm:ss.cc") .. " · " .. jogador.attr.Pontos
    self.ui.color = vector.create(1, if game.paused() then 0 else 1, 0.5)
end
return M
"#;
    let sources = ScriptSources::from_texts([
        ("scripts/jogador.luau", player),
        ("scripts/camera.luau", camera),
        ("scripts/portao.luau", gate),
        ("scripts/placar.luau", board),
    ]);
    (project, Arc::new(sources))
}

#[test]
fn a_project_using_every_area_replays_and_survives_a_vm_made_anew() {
    let (project, scripts) = every_area();
    let inputs = json!([
        {"type": "press", "at": 0.2, "action": "mover_frente"},
        {"type": "press", "at": 0.6, "action": "pular"},
        {"type": "release", "at": 0.7, "action": "pular"},
        {"type": "release", "at": 1.5, "action": "mover_frente"},
        {"type": "press", "at": 2.0, "action": "pular"},
        {"type": "release", "at": 2.1, "action": "pular"},
    ]);
    let request: SimulationRequest = serde_json::from_value(json!({
        "seconds": 3,
        "inputs": inputs,
        "check_replay": true,
        "check_vm_reset": true,
        "actions": "none",
        "ui": false
    }))
    .unwrap();
    let (report, rt) = headless::run_with_scripts(&project, scripts.clone(), &request).unwrap();
    for check in [report.replay.as_ref(), report.vm_reset.as_ref()] {
        let check = check.unwrap();
        assert!(check.identical, "{check:?}");
    }
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    assert_eq!(
        report.state_hash,
        headless::run_with_scripts(&project, scripts, &request)
            .unwrap()
            .0
            .state_hash
    );
    // The run did what the scripts say: the player walked and scored, the board shows it.
    let scene = rt.scene();
    let player = scene.entities.iter().find(|e| e.name == "Jogador").unwrap();
    assert!(matches!(player.attributes["Pontos"], Value::Number(n) if n > 0.));
    let board = scene.entities.iter().find(|e| e.name == "Placar").unwrap();
    assert!(
        board.ui.as_ref().unwrap().text.starts_with("00:0"),
        "{:?}",
        board.ui
    );
    let names: BTreeSet<&str> = rt
        .animation_playing(
            &scene
                .entities
                .iter()
                .find(|e| e.name == "Portão")
                .unwrap()
                .id,
        )
        .map(|(clip, _)| clip)
        .into_iter()
        .collect();
    assert_eq!(names.len(), 1);
    assert!(rt.audio().music.is_some());
    assert!(rt.save_slot().exists);
}
