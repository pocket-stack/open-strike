//! Desktop game shell: wraps the platform-free simulation
//! (`openstrike_core::StrikeSim`) with wgpu presentation — scene/camera/HUD
//! composition, GPU asset upload — and keyboard/mouse input mapping.
//! `Deref`s to the sim so gameplay state reads the same as before the split.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use pocket3d::bsp::MapData;
use pocket3d::input::Input;
use pocket3d::prelude::*;
use pocket3d::winit::event::MouseButton;
use pocket3d::winit::keyboard::KeyCode;

pub use openstrike_core::sim::{Command, GameEvent, Phase, SimInput};
use openstrike_core::StrikeSim;

use crate::weapon::build_rifle;

pub struct OpenStrike {
    pub sim: StrikeSim,
    pub map: MapData,
    pub scene: Scene,
    pub camera: Camera,
    pub hud: Hud,
    pub debug_overlay: bool,

    pub bot_asset: Option<Arc<ModelAsset>>,
    /// Clip index of the bot model's walk cycle (see load in `upload`).
    pub(crate) bot_walk_clip: usize,
    pub rifle_asset: Option<Arc<ModelAsset>>,
    /// Exit the app after this many seconds (smoke tests).
    pub auto_quit: Option<f32>,
}

impl Deref for OpenStrike {
    type Target = StrikeSim;
    fn deref(&self) -> &StrikeSim {
        &self.sim
    }
}

impl DerefMut for OpenStrike {
    fn deref_mut(&mut self) -> &mut StrikeSim {
        &mut self.sim
    }
}

impl OpenStrike {
    pub fn new(map: MapData, spawn_pos: Vec3, spawn_yaw: f32, bot_count: usize) -> Self {
        let mut scene = Scene::default();
        if let Some(sun) = map.sun {
            scene.sky.sun_dir = sun.dir;
            scene.lighting.sun_dir = sun.dir;
            scene.lighting.sun_color = sun.color * 0.9;
        }
        let camera = Camera {
            fov_y: 74f32.to_radians(),
            ..Default::default()
        };
        let bot_spawns = if map.t_spawns.is_empty() {
            map.ct_spawns.clone()
        } else {
            map.t_spawns.clone()
        };
        let sim = StrikeSim::new(spawn_pos, spawn_yaw, bot_spawns, bot_count);
        Self {
            sim,
            map,
            scene,
            camera,
            hud: Hud::default(),
            debug_overlay: false,
            bot_asset: None,
            bot_walk_clip: 0,
            rifle_asset: None,
            auto_quit: None,
        }
    }

    /// Upload GPU resources (called from `Game::init` or headless setup).
    pub fn upload_world(&mut self, gpu: &Gpu, renderer: &Renderer) {
        let world = Arc::new(WorldModel::from_bsp(
            gpu,
            &renderer.world_material_layout,
            &renderer.samplers,
            &self.map,
        ));
        self.scene.world = Some(world);
        self.rifle_asset = Some(build_rifle(gpu, renderer));

        match crate::args::find_asset("models/Soldier.glb") {
            Some(path) => {
                match ModelAsset::load_glb(
                    gpu,
                    &renderer.model_material_layout,
                    &renderer.samplers,
                    &path,
                ) {
                    Ok(asset) => {
                        // Bots move on their Walk cycle; fall back to clip 0
                        // for single-clip models.
                        self.bot_walk_clip = asset
                            .clips
                            .iter()
                            .position(|c| c.name.eq_ignore_ascii_case("walk"))
                            .unwrap_or(0);
                        self.bot_asset = Some(asset);
                        let clip = self.bot_walk_clip;
                        for bot in &mut self.sim.bots {
                            bot.anim.clip = clip;
                        }
                    }
                    Err(e) => log::warn!("bot model failed to load: {e:#}"),
                }
            }
            None => {
                log::warn!("bot model not found (models/Soldier.glb); bots render as nothing")
            }
        }
    }

    pub fn reset_round(&mut self) {
        let clip = self.bot_walk_clip;
        self.sim.reset_round(clip);
    }

    /// Apply one guest command (drained after each guest turn).
    pub fn apply(&mut self, cmd: Command) {
        let clip = self.bot_walk_clip;
        self.sim.apply(cmd, clip);
    }

    /// Full fixed-step game tick, from raw keyboard/mouse input.
    pub fn tick(&mut self, dt: f32, input: &Input) {
        if input.key_pressed(KeyCode::KeyV) {
            self.sim.toggle_fly();
        }
        let mut sim_input = SimInput {
            walk: input.key_down(KeyCode::ShiftLeft),
            jump: input.key_down(KeyCode::Space),
            fire: input.mouse_button_down(MouseButton::Left),
            reload: input.key_pressed(KeyCode::KeyR),
            ..Default::default()
        };
        if input.key_down(KeyCode::KeyW) {
            sim_input.move_y += 1.0;
        }
        if input.key_down(KeyCode::KeyS) {
            sim_input.move_y -= 1.0;
        }
        if input.key_down(KeyCode::KeyD) {
            sim_input.move_x += 1.0;
        }
        if input.key_down(KeyCode::KeyA) {
            sim_input.move_x -= 1.0;
        }
        let col = &self.map.collision;
        self.sim.tick(col, dt, &sim_input);
    }

    /// Build camera, scene models/effects, and HUD for the current frame.
    pub fn compose_base(&mut self, alpha: f32, time: f32, screen: (f32, f32)) {
        self.scene.time = time;
        self.camera.pos = self.sim.player.eye_interpolated(alpha);
        self.camera.yaw = self.sim.player.yaw;
        self.camera.pitch = self.sim.player.pitch;

        // Bots.
        self.scene.models.clear();
        if let Some(asset) = &self.bot_asset {
            let scale = 70.0 / asset.height();
            for bot in &self.sim.bots {
                let mut inst = ModelInstance::new(asset.clone());
                inst.transform = bot.transform_scaled(scale);
                inst.anim = AnimState {
                    clip: bot.anim.clip,
                    time: bot.anim.time,
                    speed: bot.anim.speed,
                    looping: bot.anim.looping,
                };
                inst.tint = bot.tint();
                self.scene.models.push(inst);
            }
        }

        // Viewmodel (interpolated with the camera — see viewmodel_transform_at).
        self.scene.viewmodel = match (&self.rifle_asset, self.sim.player.alive) {
            (Some(rifle), true) => {
                let mut vm = ModelInstance::new(rifle.clone());
                vm.transform = self.sim.viewmodel_transform_at(alpha);
                vm.lit = 1.0;
                Some(vm)
            }
            _ => None,
        };

        // Effects.
        self.scene.sprites.clear();
        self.scene.beams.clear();
        let mut sprites = Vec::new();
        let mut beams = Vec::new();
        self.sim.effects.emit(&mut sprites, &mut beams);
        self.scene.sprites.extend(sprites.into_iter().map(|s| Sprite {
            pos: s.pos,
            size: s.size,
            color: s.color,
        }));
        self.scene.beams.extend(beams.into_iter().map(|b| Beam {
            a: b.a,
            b: b.b,
            width: b.width,
            color: b.color,
        }));

        self.compose_hud(screen);
    }

    fn compose_hud(&mut self, screen: (f32, f32)) {
        let _ = screen;
        let hud = &mut self.hud;
        hud.clear();
        // The player-facing HUD is a PocketJS app composited by the host
        // (game/hud.tsx via pocket-ui-wgpu); the bitmap HUD only carries the
        // F3 debug overlay now.
        if self.debug_overlay {
            let p = &self.sim.player.state;
            let phase = format!("{:?}", self.sim.phase);
            hud.text(
                8.0,
                8.0,
                2.0,
                [1.0, 1.0, 1.0, 0.8],
                &format!(
                    "POS {:6.0} {:6.0} {:6.0}  VEL {:5.0}  {}  {}",
                    p.pos.x,
                    p.pos.y,
                    p.pos.z,
                    (p.vel.x * p.vel.x + p.vel.z * p.vel.z).sqrt(),
                    if p.on_ground { "GND" } else { "AIR" },
                    phase,
                ),
            );
        }
    }
}

