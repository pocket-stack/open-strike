//! GE presentation of the sim: procedural bot bodies, the rifle viewmodel,
//! and additive effect billboards. The desktop equivalent is scene
//! composition in crates/openstrike/src/game.rs — here the "scene" is GE
//! commands recorded straight into the open display list.

use alloc::vec::Vec;

use glam::{Mat4, Vec3};
use openstrike_core::weapon::{FxBeam, FxSprite, GUN_COLORS, rifle_boxes};
use openstrike_core::{Bot, StrikeSim};
use pocket3d_gu::mesh::{ColorVert, clear_depth_for_viewmodel, draw_color_tris};
use pocket3d_gu::{Camera3d, FramePool};
use psp::sys::{self, BlendFactor, BlendOp, GuState};

fn abgr(rgba: [u8; 4], brightness: f32) -> u32 {
    let c = |v: u8| ((v as f32 * brightness).clamp(0.0, 255.0)) as u32;
    0xff00_0000 | (c(rgba[2]) << 16) | (c(rgba[1]) << 8) | c(rgba[0])
}

fn abgr_f(color: [f32; 4], scale: f32) -> u32 {
    let c = |v: f32| ((v * scale).clamp(0.0, 1.0) * 255.0) as u32;
    (c(color[3]) << 24) | (c(color[2]) << 16) | (c(color[1]) << 8) | c(color[0])
}

/// Emit a box as 12 vertex-colored triangles with cheap per-face shading
/// (top bright, bottom dark) so unlit geometry still reads as 3D.
fn add_box(out: &mut Vec<ColorVert>, min: Vec3, max: Vec3, rgba: [u8; 4]) {
    let corner = |x: f32, y: f32, z: f32| Vec3 {
        x: if x > 0.0 { max.x } else { min.x },
        y: if y > 0.0 { max.y } else { min.y },
        z: if z > 0.0 { max.z } else { min.z },
    };
    // (brightness, four corners CCW seen from outside)
    let faces: [(f32, [Vec3; 4]); 6] = [
        (0.85, [corner(1.0, -1.0, 1.0), corner(1.0, -1.0, -1.0), corner(1.0, 1.0, -1.0), corner(1.0, 1.0, 1.0)]),
        (0.7, [corner(-1.0, -1.0, -1.0), corner(-1.0, -1.0, 1.0), corner(-1.0, 1.0, 1.0), corner(-1.0, 1.0, -1.0)]),
        (1.0, [corner(-1.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), corner(1.0, 1.0, -1.0), corner(-1.0, 1.0, -1.0)]),
        (0.5, [corner(-1.0, -1.0, -1.0), corner(1.0, -1.0, -1.0), corner(1.0, -1.0, 1.0), corner(-1.0, -1.0, 1.0)]),
        (0.9, [corner(-1.0, -1.0, 1.0), corner(1.0, -1.0, 1.0), corner(1.0, 1.0, 1.0), corner(-1.0, 1.0, 1.0)]),
        (0.65, [corner(1.0, -1.0, -1.0), corner(-1.0, -1.0, -1.0), corner(-1.0, 1.0, -1.0), corner(1.0, 1.0, -1.0)]),
    ];
    for (brightness, q) in faces {
        let color = abgr(rgba, brightness);
        let v = |p: Vec3| ColorVert {
            color,
            x: p.x,
            y: p.y,
            z: p.z,
        };
        out.extend_from_slice(&[v(q[0]), v(q[1]), v(q[2]), v(q[0]), v(q[2]), v(q[3])]);
    }
}

/// The rifle viewmodel as vertex-colored triangles (built once at boot).
pub fn build_rifle() -> Vec<ColorVert> {
    let mut out = Vec::new();
    for b in rifle_boxes() {
        add_box(&mut out, b.min, b.max, GUN_COLORS[b.color]);
    }
    out
}

/// Bot body geometry in "feet space" (origin at the feet, +Y up, facing -Z),
/// so `Bot::transform_scaled(1.0)` places and death-falls it correctly.
pub fn build_bot_body() -> Vec<ColorVert> {
    const UNIFORM: [u8; 4] = [168, 142, 92, 255]; // desert fatigues
    const VEST: [u8; 4] = [70, 74, 62, 255];
    const SKIN: [u8; 4] = [206, 168, 130, 255];
    const GUNMETAL: [u8; 4] = [40, 40, 44, 255];
    let mut out = Vec::new();
    // Legs.
    add_box(&mut out, Vec3::new(-11.0, 0.0, -6.0), Vec3::new(-2.0, 32.0, 6.0), UNIFORM);
    add_box(&mut out, Vec3::new(2.0, 0.0, -6.0), Vec3::new(11.0, 32.0, 6.0), UNIFORM);
    // Torso + vest.
    add_box(&mut out, Vec3::new(-13.0, 32.0, -7.0), Vec3::new(13.0, 54.0, 7.0), VEST);
    // Arms.
    add_box(&mut out, Vec3::new(-17.0, 34.0, -5.0), Vec3::new(-13.0, 52.0, 5.0), UNIFORM);
    add_box(&mut out, Vec3::new(13.0, 34.0, -5.0), Vec3::new(17.0, 52.0, 5.0), UNIFORM);
    // Head.
    add_box(&mut out, Vec3::new(-6.0, 54.0, -6.0), Vec3::new(6.0, 68.0, 6.0), SKIN);
    // Rifle held across, pointing forward (-Z).
    add_box(&mut out, Vec3::new(-2.0, 40.0, -26.0), Vec3::new(2.0, 44.0, -4.0), GUNMETAL);
    out
}

/// Draw all bots (alive and falling corpses).
pub unsafe fn draw_bots(pool: &mut FramePool, body: &[ColorVert], bots: &[Bot]) {
    for bot in bots {
        // Fade corpses is desktop tint; here the shading bake is enough.
        draw_color_tris(pool, body, bot.transform_scaled(1.0));
    }
}

/// Additive billboards for effects (muzzle flashes, tracers, impacts).
pub unsafe fn draw_effects(pool: &mut FramePool, sim: &StrikeSim, cam: &Camera3d) {
    let mut sprites: Vec<FxSprite> = Vec::new();
    let mut beams: Vec<FxBeam> = Vec::new();
    sim.effects.emit(&mut sprites, &mut beams);
    if sprites.is_empty() && beams.is_empty() {
        return;
    }

    let fwd = cam.forward();
    let right = fwd.cross(Vec3::Y).normalize_or_zero();
    let up = right.cross(fwd);

    let mut verts: Vec<ColorVert> = Vec::new();
    let mut quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: u32| {
        let v = |p: Vec3| ColorVert {
            color,
            x: p.x,
            y: p.y,
            z: p.z,
        };
        verts.extend_from_slice(&[v(a), v(b), v(c), v(a), v(c), v(d)]);
    };
    for s in &sprites {
        // Additive: bake alpha into the color (dst weight is fixed 1).
        let color = abgr_f(s.color, s.color[3]);
        let r = right * (s.size * 0.5);
        let u = up * (s.size * 0.5);
        quad(s.pos - r - u, s.pos + r - u, s.pos + r + u, s.pos - r + u, color);
    }
    for b in &beams {
        let color = abgr_f(b.color, b.color[3]);
        let axis = b.b - b.a;
        let side = axis.cross(fwd).normalize_or_zero() * (b.width * 0.5);
        quad(b.a - side, b.b - side, b.b + side, b.a + side, color);
    }

    // Additive blend, depth-test but never depth-write (transparents).
    sys::sceGuEnable(GuState::Blend);
    sys::sceGuBlendFunc(BlendOp::Add, BlendFactor::SrcAlpha, BlendFactor::Fix, 0, 0xffffff);
    sys::sceGuDepthMask(1);
    draw_color_tris(pool, &verts, Mat4::IDENTITY);
    sys::sceGuDepthMask(0);
    sys::sceGuDisable(GuState::Blend);
}

/// The first-person rifle, drawn over a cleared depth buffer so it never
/// pokes into walls (the desktop renderer's dedicated viewmodel pass).
pub unsafe fn draw_viewmodel(pool: &mut FramePool, rifle: &[ColorVert], sim: &StrikeSim) {
    if !sim.player.alive {
        return;
    }
    clear_depth_for_viewmodel();
    draw_color_tris(pool, rifle, sim.viewmodel_transform_at(1.0));
}
