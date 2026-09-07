//! Original officer's baked presentation, shared by handheld renderers.
//! Geometry and poses are authored/evaluated in Blender; the runtime only
//! interpolates indexed vertices. No allocation, skeleton solver or glTF parser.
#![no_std]

use glam::Vec3;
use openstrike_core::bot::ActorClip;

const DATA: &[u8] = include_bytes!("../../../assets/characters/police/officer.opch");
const HEADER: usize = 24;
const RECORD: usize = 16;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct Vertex {
    pub color: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Quantized GE input, including explicit padding so no uninitialized bytes
/// reach the device. Two consecutive entries form one hardware morph vertex.
#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct PackedVertex {
    pub color: u32,
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub padding: u16,
}

pub fn baked_frame_count() -> usize {
    u32_at(20) as usize
}

pub fn baked_vertex(frame: usize, i: usize) -> PackedVertex {
    assert!(frame < baked_frame_count() && i < vertex_count());
    let at = poses_start() + (frame * vertex_count() + i) * 6;
    PackedVertex {
        color: u32_at(colors_start() + i * 4),
        x: u16_at(at) as i16,
        y: u16_at(at + 2) as i16,
        z: u16_at(at + 4) as i16,
        padding: 0,
    }
}

fn u32_at(offset: usize) -> u32 {
    u32::from_le_bytes(DATA[offset..offset + 4].try_into().unwrap())
}
fn u16_at(offset: usize) -> u16 {
    u16::from_le_bytes([DATA[offset], DATA[offset + 1]])
}
pub fn vertex_count() -> usize {
    u32_at(8) as usize
}
pub fn index_count() -> usize {
    u32_at(12) as usize
}
pub fn triangle_count() -> usize {
    index_count() / 3
}
pub fn asset_bytes() -> usize {
    DATA.len()
}
fn colors_start() -> usize {
    HEADER + u32_at(16) as usize * RECORD
}
fn indices_start() -> usize {
    colors_start() + vertex_count() * 4
}
fn poses_start() -> usize {
    indices_start() + index_count() * 2
}
pub fn index(i: usize) -> usize {
    assert!(i < index_count());
    u16_at(indices_start() + i * 2) as usize
}
pub fn copy_indices(out: &mut [u16]) {
    assert_eq!(out.len(), index_count());
    for (i, out) in out.iter_mut().enumerate() {
        *out = index(i) as u16;
    }
}
pub fn duration(clip: ActorClip) -> f32 {
    f32::from_bits(u32_at(HEADER + clip as usize * RECORD + 8))
}

pub struct Pose {
    a: usize,
    b: usize,
    mix: f32,
}
impl Pose {
    /// Global baked-frame indices and their interpolation weight. Renderers
    /// with vertex morphing can consume the same samples without CPU skinning.
    pub fn frame_pair(&self) -> (usize, usize, f32) {
        let stride = vertex_count() * 6;
        (
            (self.a - poses_start()) / stride,
            (self.b - poses_start()) / stride,
            self.mix,
        )
    }
    pub fn new(clip: ActorClip, time: f32) -> Self {
        let at = HEADER + clip as usize * RECORD;
        let start = u32_at(at) as usize;
        let count = u32_at(at + 4) as usize;
        let duration = duration(clip);
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        let time = if u32_at(at + 12) == 1 {
            time % duration
        } else {
            time.min(duration)
        };
        let frame = time / duration * (count - 1) as f32;
        let a = frame as usize;
        let b = (a + 1).min(count - 1);
        let stride = vertex_count() * 6;
        Self {
            a: poses_start() + (start + a) * stride,
            b: poses_start() + (start + b) * stride,
            mix: frame - a as f32,
        }
    }
    /// Sample one unique vertex; callers reuse it through u16 indices.
    pub fn vertex(&self, i: usize) -> Vertex {
        assert!(i < vertex_count());
        let component = |c| {
            let a = u16_at(self.a + i * 6 + c) as i16 as f32;
            let b = u16_at(self.b + i * 6 + c) as i16 as f32;
            (a + (b - a) * self.mix) * (1.0 / 256.0)
        };
        Vertex {
            color: u32_at(colors_start() + i * 4),
            x: component(0),
            y: component(2),
            z: component(4),
        }
    }
    pub fn fill(&self, out: &mut [Vertex]) {
        assert_eq!(out.len(), vertex_count());
        // Validate the two complete frames once. Calling vertex() here left
        // six byte-range checks and a function call inside every vertex on
        // Allegrex; the checked fixed-size chunks keep the hot loop bounded.
        let bytes = out.len() * 6;
        let (a, _) = DATA[self.a..self.a + bytes].as_chunks::<6>();
        let (b, _) = DATA[self.b..self.b + bytes].as_chunks::<6>();
        let colors = colors_start();
        let (colors, _) = DATA[colors..colors + out.len() * 4].as_chunks::<4>();
        for (((v, a), b), color) in out.iter_mut().zip(a).zip(b).zip(colors) {
            let component = |offset: usize| {
                let a = i16::from_le_bytes([a[offset], a[offset + 1]]) as f32;
                let b = i16::from_le_bytes([b[offset], b[offset + 1]]) as f32;
                (a + (b - a) * self.mix) * (1.0 / 256.0)
            };
            *v = Vertex {
                color: u32::from_le_bytes(*color),
                x: component(0),
                y: component(2),
                z: component(4),
            };
        }
    }
    pub fn position(&self, i: usize) -> Vec3 {
        let v = self.vertex(i);
        Vec3::new(v.x, v.y, v.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_asset_stays_within_psp_budget_and_indices_are_valid() {
        assert_eq!(&DATA[..4], b"OPCH");
        assert_eq!(u32_at(4), 1);
        assert_eq!(u32_at(16), ActorClip::ALL.len() as u32);
        assert!(asset_bytes() <= 512 * 1024);
        assert!(triangle_count() <= 1400);
        assert!(vertex_count() * core::mem::size_of::<Vertex>() <= 65536);
        assert!(index_count() * 2 <= 65536);
        assert_eq!(core::mem::size_of::<PackedVertex>(), 12);
        assert!(baked_frame_count() * vertex_count() * 24 <= 2 * 1024 * 1024);
        assert_eq!(
            DATA.len(),
            poses_start() + u32_at(20) as usize * vertex_count() * 6
        );
        for i in 0..index_count() {
            assert!(index(i) < vertex_count());
        }
    }
    #[test]
    fn every_authored_clip_moves_and_has_finite_bounded_poses() {
        for clip in ActorClip::ALL {
            let a = Pose::new(clip, 0.0);
            let b = Pose::new(clip, duration(clip) * 0.37);
            let mut moved = 0;
            for i in 0..vertex_count() {
                let p = b.position(i);
                assert!(p.is_finite() && p.abs().max_element() < 120.0);
                if a.position(i).distance_squared(p) > 0.01 {
                    moved += 1;
                }
            }
            assert!(moved > 10, "{clip:?}: only {moved} vertices moved");
        }
    }
    #[test]
    fn loops_wrap_and_one_shots_hold_final_pose() {
        for clip in ActorClip::ALL {
            let looping = matches!(clip, ActorClip::Idle | ActorClip::Walk | ActorClip::Run);
            let a = Pose::new(clip, if looping { 0.0 } else { duration(clip) });
            let b = Pose::new(clip, duration(clip) * 2.0);
            for i in 0..vertex_count() {
                assert!(a.position(i).distance(b.position(i)) < 0.01);
            }
        }
    }
    #[test]
    fn batch_sampling_preserves_every_clip_and_vertex() {
        let mut vertices = [Vertex::default(); 847];
        for clip in ActorClip::ALL {
            for fraction in [0.0, 0.13, 0.37, 0.81, 1.0, 2.0] {
                let pose = Pose::new(clip, duration(clip) * fraction);
                pose.fill(&mut vertices);
                let (a, b, weight) = pose.frame_pair();
                assert!(b == a || b == a + 1);
                assert!((0.0..=1.0).contains(&weight));
                if a == b {
                    assert_eq!(weight, 0.0);
                }
                for (i, batch) in vertices.iter().enumerate() {
                    let single = pose.vertex(i);
                    assert_eq!(batch.color, single.color);
                    assert_eq!([batch.x, batch.y, batch.z], [single.x, single.y, single.z]);
                    let a = baked_vertex(a, i);
                    let b = baked_vertex(b, i);
                    assert_eq!(a.color, single.color);
                    assert_eq!(a.color, b.color);
                    for ((a, b), expected) in [a.x, a.y, a.z]
                        .into_iter()
                        .zip([b.x, b.y, b.z])
                        .zip([single.x, single.y, single.z])
                    {
                        // GE decodes i16 / 32768; the model scale is 128.
                        let actual = (a as f32 * (1.0 - weight) + b as f32 * weight) / 256.0;
                        assert!((actual - expected).abs() < 0.001);
                    }
                }
            }
        }
    }
    #[test]
    fn death_is_a_grounded_authored_fall_and_gaits_close_the_loop() {
        let death = Pose::new(ActorClip::Death, duration(ActorClip::Death));
        let mut low = f32::MAX;
        let mut high = f32::MIN;
        for i in 0..vertex_count() {
            let y = death.position(i).y;
            low = low.min(y);
            high = high.max(y);
        }
        assert!(low >= 0.0 && low < 1.0);
        assert!(
            high < 30.0,
            "corpse must lie down in its own action: {high}"
        );
        for clip in [ActorClip::Idle, ActorClip::Walk, ActorClip::Run] {
            let (start, _, _) = Pose::new(clip, 0.0).frame_pair();
            let count = u32_at(HEADER + clip as usize * RECORD + 4) as usize;
            for i in 0..vertex_count() {
                let a = baked_vertex(start, i);
                let b = baked_vertex(start + count - 1, i);
                for (a, b) in [a.x, a.y, a.z].into_iter().zip([b.x, b.y, b.z]) {
                    assert!((a as i32 - b as i32).abs() < 128, "{clip:?} loop seam");
                }
            }
        }
    }
}
