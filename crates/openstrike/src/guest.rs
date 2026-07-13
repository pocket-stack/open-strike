//! The guest: one QuickJS realm running the OpenStrike product bundle —
//! gameplay rules (the base game is the first mod) plus the Solid JSX HUD.
//!
//! Two surfaces are mounted (RUNTIMES.md):
//!   - `ui` — the PocketJS 2D runtime (pocket-ui-wgpu), composited over
//!     the 3D frame as the HUD;
//!   - `strike` — this game's vocabulary. Facts flow guest-ward as per-tick
//!     event batches (`strike.__dispatch(state, events)`); intent flows
//!     host-ward as commands queued by ops and applied after the guest
//!     turn. No shared state, no re-entrancy.
//!
//! Turn order per fixed tick (Law 3 — one guest turn per tick):
//!   game.tick() → __dispatch(state, events) → frame(buttons) → ui.tick()
//!   → drain commands into the game.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow};
use pocket3d::gpu::{Gpu, OffscreenTarget};
use pocket_mod::Guest;
use pocket_mod::qjs::{Array, CatchResultExt, Function, Object};
use pocket_ui_wgpu::{Blit, UiRenderer, UiSurface};

use crate::bot::BotConfig;
use crate::game::{Command, GameEvent, OpenStrike, Phase};
use crate::weapon::WeaponConfig;

pub struct StrikeGuest {
    guest: Guest,
    ui: UiSurface,
    commands: Rc<RefCell<Vec<Command>>>,
    /// Logical UI size (the core's viewport).
    ui_size: (u32, u32),
    gfx: Option<OverlayGfx>,
}

struct OverlayGfx {
    renderer: UiRenderer,
    offscreen: OffscreenTarget,
    blit: Blit,
    target_format: wgpu::TextureFormat,
}

/// Locate the PSP-baseline product bundle (`dist/pocket/psp`).
pub fn find_bundle() -> Result<(PathBuf, PathBuf)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("OPENSTRIKE_UI_DIST") {
        roots.push(PathBuf::from(d));
    }
    roots.push(PathBuf::from("dist/pocket/psp"));
    roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/pocket/psp"));
    for root in &roots {
        let js = root.join("openstrike.js");
        let pak = root.join("openstrike.pak");
        if js.is_file() && pak.is_file() {
            return Ok((js, pak));
        }
    }
    Err(anyhow!(
        "HUD/rules bundle not found — build it first: `bun run build:ui` \
         (searched dist/pocket/psp next to the repo root; override with OPENSTRIKE_UI_DIST)"
    ))
}

impl StrikeGuest {
    /// Boot the realm: feed the pak, mount `ui` + `strike`, eval the bundle.
    /// `ui_size` is the logical HUD resolution (window logical size).
    pub fn boot(ui_size: (u32, u32)) -> Result<StrikeGuest> {
        let (js_path, pak_path) = find_bundle()?;
        let bundle = std::fs::read_to_string(&js_path)
            .with_context(|| format!("reading {}", js_path.display()))?;
        let pak =
            std::fs::read(&pak_path).with_context(|| format!("reading {}", pak_path.display()))?;

        let ui = UiSurface::new((ui_size.0 as f32, ui_size.1 as f32));
        ui.feed_pak(&pak);
        let guest = Guest::new()?;
        ui.mount(&guest)?;

        let commands: Rc<RefCell<Vec<Command>>> = Rc::new(RefCell::new(Vec::new()));
        mount_strike(&guest, &commands)?;

        guest.eval("openstrike", &bundle)?;
        if !guest.has_frame() {
            return Err(anyhow!("bundle evaluated but installed no frame() — HUD missing?"));
        }
        log::info!(
            "guest: booted {} ({} bytes js) at {}x{}",
            js_path.display(),
            bundle.len(),
            ui_size.0,
            ui_size.1
        );
        Ok(StrikeGuest { guest, ui, commands, ui_size, gfx: None })
    }

    /// One guest turn for one game tick.
    pub fn turn(&self, game: &mut OpenStrike) -> Result<()> {
        let events = std::mem::take(&mut game.events);
        self.guest.with(|ctx| -> Result<()> {
            let strike: Object = ctx.globals().get("strike").context("strike surface missing")?;
            let Ok(dispatch) = strike.get::<_, Function>("__dispatch") else {
                return Ok(()); // no SDK loaded — state simply doesn't flow
            };
            let state = build_state(&ctx, game)?;
            let batch = Array::new(ctx.clone())?;
            for (i, e) in events.iter().enumerate() {
                batch.set(i, build_event(&ctx, e)?)?;
            }
            dispatch
                .call::<_, ()>((state, batch))
                .catch(&ctx)
                .map_err(|e| anyhow!("strike.__dispatch threw: {e}"))?;
            Ok(())
        })?;
        self.guest.frame(0)?;
        self.ui.tick();
        for cmd in self.commands.borrow_mut().drain(..) {
            game.apply(cmd);
        }
        Ok(())
    }

    /// Render the HUD over `view` (`target_px` physical pixels): the UI draws
    /// 1:1 at its logical size offscreen, then composites with a linear blit
    /// (hidpi swapchains scale smoothly).
    pub fn render_overlay(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target_format: wgpu::TextureFormat,
    ) -> Result<()> {
        if self.gfx.as_ref().is_none_or(|g| g.target_format != target_format) {
            let offscreen = OffscreenTarget::new(gpu, self.ui_size.0, self.ui_size.1);
            let blit = Blit::new(
                gpu,
                &offscreen.view,
                target_format,
                wgpu::FilterMode::Linear,
                true,
            );
            self.gfx = Some(OverlayGfx {
                renderer: UiRenderer::new(gpu, pocket3d::gpu::OFFSCREEN_FORMAT),
                offscreen,
                blit,
                target_format,
            });
        }
        let gfx = self.gfx.as_mut().unwrap();
        let ui_size = self.ui_size;
        self.ui.with_ui(|ui| {
            gfx.renderer.render(
                gpu,
                ui,
                encoder,
                &gfx.offscreen.view,
                ui_size,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            )
        })?;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            gfx.blit.draw(&mut pass);
        }
        Ok(())
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Starting => "starting",
        Phase::Live => "live",
        Phase::Ended { won: true } => "won",
        Phase::Ended { won: false } => "lost",
    }
}

fn parse_phase(name: &str) -> Option<Phase> {
    Some(match name {
        "starting" => Phase::Starting,
        "live" => Phase::Live,
        "won" => Phase::Ended { won: true },
        "lost" => Phase::Ended { won: false },
        _ => return None,
    })
}

fn build_state<'js>(
    ctx: &pocket_mod::qjs::Ctx<'js>,
    game: &OpenStrike,
) -> pocket_mod::qjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    o.set("time", game.time as f64)?;
    o.set("phase", phase_name(game.phase))?;
    o.set("hp", game.player.health)?;
    o.set("alive", game.player.alive)?;
    o.set("ammo", game.weapon.ammo)?;
    o.set("reserve", game.weapon.reserve)?;
    o.set("reloading", game.weapon.reloading())?;
    let reload_frac = if game.weapon.reloading() {
        1.0 - (game.weapon.reload_left / game.weapon.cfg.reload_time).clamp(0.0, 1.0)
    } else {
        0.0
    };
    o.set("reloadFrac", reload_frac as f64)?;
    o.set("aliveBots", game.alive_bots() as u32)?;
    o.set("totalBots", game.bots.len() as u32)?;
    o.set("wins", game.score.wins)?;
    o.set("losses", game.score.losses)?;
    let v = game.player.state.vel;
    o.set("speed", ((v.x * v.x + v.z * v.z).sqrt()) as f64)?;
    Ok(o)
}

fn build_event<'js>(
    ctx: &pocket_mod::qjs::Ctx<'js>,
    e: &GameEvent,
) -> pocket_mod::qjs::Result<Object<'js>> {
    let o = Object::new(ctx.clone())?;
    match e {
        GameEvent::Hit { bot, headshot, damage, fatal } => {
            o.set("type", "hit")?;
            o.set("bot", *bot as u32)?;
            o.set("headshot", *headshot)?;
            o.set("damage", *damage)?;
            o.set("fatal", *fatal)?;
        }
        GameEvent::PlayerDamaged { amount, hp } => {
            o.set("type", "playerDamaged")?;
            o.set("amount", *amount)?;
            o.set("hp", *hp)?;
        }
        GameEvent::PlayerDied => o.set("type", "playerDied")?,
        GameEvent::RoundReset => o.set("type", "roundReset")?,
    }
    Ok(o)
}

/// Mount the `strike` namespace: intent ops that queue [`Command`]s.
fn mount_strike(guest: &Guest, commands: &Rc<RefCell<Vec<Command>>>) -> Result<()> {
    guest.mount("strike", |ctx, ns| {
        macro_rules! op {
            ($name:literal, $f:expr) => {
                ns.set($name, Function::new(ctx.clone(), $f)?)?;
            };
        }

        // Menu-host vocabulary (surface parity with the PSP EBOOT). The
        // desktop build pre-loads its map from the CLI and never enters the
        // menu, so these are honest no-ops and the catalogue is empty.
        ns.set("maps", Vec::<String>::new())?;
        op!("loadMap", move |_i: i32| {
            log::warn!("strike.loadMap: desktop pre-loads its map (--map)");
        });
        op!("toMenu", move || {
            log::warn!("strike.toMenu: no menu on the desktop host (exit and rerun)");
        });

        let q = commands.clone();
        op!("setPhase", move |name: String| {
            if let Some(p) = parse_phase(&name) {
                q.borrow_mut().push(Command::SetPhase(p));
            } else {
                log::warn!("strike.setPhase: unknown phase '{name}'");
            }
        });

        let q = commands.clone();
        op!("resetRound", move || q.borrow_mut().push(Command::ResetRound));

        let q = commands.clone();
        op!("addWin", move || q.borrow_mut().push(Command::AddWin));

        let q = commands.clone();
        op!("addLoss", move || q.borrow_mut().push(Command::AddLoss));

        let q = commands.clone();
        op!("setBotCount", move |n: i32| {
            q.borrow_mut().push(Command::SetBotCount(n.max(0) as usize))
        });

        let q = commands.clone();
        op!("configureWeapon", move |o: Object| {
            let d = WeaponConfig::default();
            let cfg = WeaponConfig {
                mag_size: get_u32(&o, "magSize", d.mag_size),
                reserve: get_u32(&o, "reserve", d.reserve),
                fire_interval: get_f32(&o, "fireInterval", d.fire_interval),
                reload_time: get_f32(&o, "reloadTime", d.reload_time),
                damage_body: get_i32(&o, "damageBody", d.damage_body),
                damage_head: get_i32(&o, "damageHead", d.damage_head),
            };
            q.borrow_mut().push(Command::ConfigureWeapon(cfg));
        });

        let q = commands.clone();
        op!("configureBots", move |o: Object| {
            let d = BotConfig::default();
            let cfg = BotConfig {
                count: get_u32(&o, "count", d.count as u32) as usize,
                speed: get_f32(&o, "speed", d.speed),
                attack_interval: get_f32(&o, "attackInterval", d.attack_interval),
                damage_min: get_i32(&o, "damageMin", d.damage_min),
                damage_max: get_i32(&o, "damageMax", d.damage_max),
            };
            q.borrow_mut().push(Command::ConfigureBots(cfg));
        });

        Ok(())
    })
}

fn get_f32(o: &Object, key: &str, default: f32) -> f32 {
    o.get::<_, f64>(key).map(|v| v as f32).unwrap_or(default)
}

fn get_i32(o: &Object, key: &str, default: i32) -> i32 {
    o.get::<_, i32>(key).unwrap_or(default)
}

fn get_u32(o: &Object, key: &str, default: u32) -> u32 {
    o.get::<_, i32>(key).map(|v| v.max(0) as u32).unwrap_or(default)
}
