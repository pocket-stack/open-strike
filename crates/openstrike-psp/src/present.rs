//! GE presentation of the sim: baked officer animation, the rifle viewmodel,
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
        (
            0.85,
            [
                corner(1.0, -1.0, 1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, 1.0, -1.0),
                corner(1.0, 1.0, 1.0),
            ],
        ),
        (
            0.7,
            [
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, -1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
                corner(-1.0, 1.0, -1.0),
            ],
        ),
        (
            1.0,
            [
                corner(-1.0, 1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(1.0, 1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
            ],
        ),
        (
            0.5,
            [
                corner(-1.0, -1.0, -1.0),
                corner(1.0, -1.0, -1.0),
                corner(1.0, -1.0, 1.0),
                corner(-1.0, -1.0, 1.0),
            ],
        ),
        (
            0.9,
            [
                corner(-1.0, -1.0, 1.0),
                corner(1.0, -1.0, 1.0),
                corner(1.0, 1.0, 1.0),
                corner(-1.0, 1.0, 1.0),
            ],
        ),
        (
            0.65,
            [
                corner(1.0, -1.0, -1.0),
                corner(-1.0, -1.0, -1.0),
                corner(-1.0, 1.0, -1.0),
                corner(1.0, 1.0, -1.0),
            ],
        ),
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

/// Immutable adjacent pose pairs, shared by all officers. The PSP GE blends
/// the two quantized frames; CPU cost is independent of the vertex count.
/// The cache is built/flushed once and remains alive for every in-flight list.
pub struct OfficerRenderer {
    pairs: Vec<[openstrike_character::PackedVertex; 2]>,
    indices: Vec<u16>,
    // A held pose reads only the first member of each cached pair. Doubling
    // indices selects that member without another vertex cache or any upload.
    held_indices: Vec<u16>,
    pub visible: u32,
}
impl OfficerRenderer {
    pub fn new() -> Self {
        let frames = openstrike_character::baked_frame_count();
        let vertices = openstrike_character::vertex_count();
        let mut pairs = Vec::with_capacity(frames * vertices);
        for frame in 0..frames {
            for i in 0..vertices {
                pairs.push([
                    openstrike_character::baked_vertex(frame, i),
                    openstrike_character::baked_vertex((frame + 1).min(frames - 1), i),
                ]);
            }
        }
        assert!(pairs.len() * core::mem::size_of_val(&pairs[0]) <= 2 * 1024 * 1024);
        let mut indices = alloc::vec![0; openstrike_character::index_count()];
        openstrike_character::copy_indices(&mut indices);
        assert!(vertices * 2 <= u16::MAX as usize);
        let held_indices: Vec<u16> = indices.iter().map(|i| i * 2).collect();
        unsafe {
            sys::sceKernelDcacheWritebackRange(
                pairs.as_ptr() as *const _,
                (pairs.len() * core::mem::size_of_val(&pairs[0])) as u32,
            );
            sys::sceKernelDcacheWritebackRange(
                indices.as_ptr() as *const _,
                (indices.len() * 2) as u32,
            );
            sys::sceKernelDcacheWritebackRange(
                held_indices.as_ptr() as *const _,
                (held_indices.len() * 2) as u32,
            );
        }
        Self {
            pairs,
            indices,
            held_indices,
            visible: 0,
        }
    }
    pub unsafe fn draw(&mut self, _pool: &mut FramePool, bots: &[Bot], cam: &Camera3d) {
        use core::ffi::c_void;
        use psp::sys::{GuPrimitive, MatrixMode, VertexType};
        let frustum = cam.frustum();
        self.visible = 0;
        for bot in bots {
            // Includes the carried rifle, stride and fallen body at every yaw.
            if !frustum.intersects_aabb(
                bot.state.pos - Vec3::new(78.0, 42.0, 78.0),
                bot.state.pos + Vec3::new(78.0, 45.0, 78.0),
            ) {
                continue;
            }
            let (clip, time) = bot.animation_sample();
            let (a, b, mix) = openstrike_character::Pose::new(clip, time).frame_pair();
            // Terminal one-shot frames hold their last pose. At a clip
            // boundary the next cached frame belongs to another action.
            let mix = if a == b { 0.0 } else { mix };
            let held = mix == 0.0;
            sys::sceGuMorphWeight(0, 1.0 - mix);
            sys::sceGuMorphWeight(1, mix);
            sys::sceGuSetMatrix(
                MatrixMode::Model,
                // GE normalizes signed 16-bit positions by 32768; OPCH
                // uses 256 quantization steps per game unit.
                &pocket3d_gu::to_psp_matrix(bot.transform_scaled(32768.0 / 256.0)),
            );
            sys::sceGuDisable(GuState::Texture2D);
            sys::sceGuDrawArray(
                GuPrimitive::Triangles,
                VertexType::COLOR_8888
                    | VertexType::VERTEX_16BIT
                    | if held {
                        VertexType::empty()
                    } else {
                        VertexType::VERTICES2
                    }
                    | VertexType::INDEX_16BIT
                    | VertexType::TRANSFORM_3D,
                self.indices.len() as i32,
                if held {
                    self.held_indices.as_ptr()
                } else {
                    self.indices.as_ptr()
                } as *const c_void,
                self.pairs
                    .as_ptr()
                    .add(a * openstrike_character::vertex_count()) as *const c_void,
            );
            self.visible += 1;
        }
        sys::sceGuMorphWeight(0, 1.0);
        sys::sceGuMorphWeight(1, 0.0);
        sys::sceGuEnable(GuState::Texture2D);
        sys::sceGuSetMatrix(
            MatrixMode::Model,
            &pocket3d_gu::to_psp_matrix(Mat4::IDENTITY),
        );
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
        quad(
            s.pos - r - u,
            s.pos + r - u,
            s.pos + r + u,
            s.pos - r + u,
            color,
        );
    }
    for b in &beams {
        let color = abgr_f(b.color, b.color[3]);
        let axis = b.b - b.a;
        let side = axis.cross(fwd).normalize_or_zero() * (b.width * 0.5);
        quad(b.a - side, b.b - side, b.b + side, b.a + side, color);
    }

    // Additive blend, depth-test but never depth-write (transparents).
    sys::sceGuEnable(GuState::Blend);
    sys::sceGuBlendFunc(
        BlendOp::Add,
        BlendFactor::SrcAlpha,
        BlendFactor::Fix,
        0,
        0xffffff,
    );
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
