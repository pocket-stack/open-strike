//! The rifle: config, firing state, timed effects, the procedural viewmodel
//! geometry (as plain data), and the shared deterministic RNG.

use alloc::vec::Vec;

use glam::Vec3;

pub const RANGE: f32 = 8192.0;

/// Weapon tuning — owned by the `strike` surface (mods set it through
/// `strike.configureWeapon`). Defaults are the base game's rifle.
#[derive(Clone, Debug)]
pub struct WeaponConfig {
    pub mag_size: u32,
    pub reserve: u32,
    /// Seconds between shots (0.105 ≈ 570 rpm).
    pub fire_interval: f32,
    pub reload_time: f32,
    pub damage_body: i32,
    pub damage_head: i32,
}

impl Default for WeaponConfig {
    fn default() -> Self {
        Self {
            mag_size: 30,
            reserve: 90,
            fire_interval: 0.105,
            reload_time: 2.4,
            damage_body: 34,
            damage_head: 100,
        }
    }
}

pub struct Weapon {
    pub cfg: WeaponConfig,
    pub ammo: u32,
    pub reserve: u32,
    pub cooldown: f32,
    pub reload_left: f32,
    /// 0..1 visual recoil, decays.
    pub recoil: f32,
    /// `recoil` at the previous tick (the viewmodel interpolates between the
    /// two so recoil doesn't step at the tick rate on high-Hz displays).
    pub prev_recoil: f32,
}

impl Default for Weapon {
    fn default() -> Self {
        Self::with_config(WeaponConfig::default())
    }
}

impl Weapon {
    pub fn with_config(cfg: WeaponConfig) -> Self {
        Self {
            ammo: cfg.mag_size,
            reserve: cfg.reserve,
            cfg,
            cooldown: 0.0,
            reload_left: 0.0,
            recoil: 0.0,
            prev_recoil: 0.0,
        }
    }

    pub fn reloading(&self) -> bool {
        self.reload_left > 0.0
    }

    pub fn can_fire(&self) -> bool {
        self.cooldown <= 0.0 && self.ammo > 0 && !self.reloading()
    }

    pub fn tick(&mut self, dt: f32) {
        self.cooldown -= dt;
        self.prev_recoil = self.recoil;
        self.recoil = (self.recoil - dt * 3.0).max(0.0);
        if self.reload_left > 0.0 {
            self.reload_left -= dt;
            if self.reload_left <= 0.0 {
                let want = self.cfg.mag_size - self.ammo;
                let take = want.min(self.reserve);
                self.ammo += take;
                self.reserve -= take;
            }
        }
    }

    pub fn trigger_reload(&mut self) {
        if !self.reloading() && self.ammo < self.cfg.mag_size && self.reserve > 0 {
            self.reload_left = self.cfg.reload_time;
        }
    }

    /// Consume one round; returns false if empty.
    pub fn fire(&mut self) -> bool {
        if !self.can_fire() {
            return false;
        }
        self.ammo -= 1;
        self.cooldown = self.cfg.fire_interval;
        self.recoil = (self.recoil + 0.35).min(1.0);
        true
    }

    /// Fresh magazine under the current config (round reset).
    pub fn reset(&mut self) {
        *self = Self::with_config(self.cfg.clone());
    }
}

// ---------------------------------------------------------------------------
// Timed world-space effects (flashes, tracers, impacts)
// ---------------------------------------------------------------------------

pub enum EffectKind {
    MuzzleFlash { pos: Vec3 },
    Tracer { a: Vec3, b: Vec3 },
    Impact { pos: Vec3 },
    BloodPuff { pos: Vec3 },
}

pub struct Effect {
    pub kind: EffectKind,
    pub age: f32,
    pub ttl: f32,
}

/// Renderer-agnostic effect output (the desktop maps these to scene
/// sprites/beams; the PSP draws billboards).
#[derive(Clone, Copy, Debug)]
pub struct FxSprite {
    pub pos: Vec3,
    pub size: f32,
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct FxBeam {
    pub a: Vec3,
    pub b: Vec3,
    pub width: f32,
    pub color: [f32; 4],
}

#[derive(Default)]
pub struct Effects {
    pub list: Vec<Effect>,
}

impl Effects {
    pub fn spawn(&mut self, kind: EffectKind, ttl: f32) {
        self.list.push(Effect { kind, age: 0.0, ttl });
    }

    pub fn tick(&mut self, dt: f32) {
        for e in &mut self.list {
            e.age += dt;
        }
        self.list.retain(|e| e.age < e.ttl);
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Emit sprites/beams for this frame.
    pub fn emit(&self, sprites: &mut Vec<FxSprite>, beams: &mut Vec<FxBeam>) {
        for e in &self.list {
            let f = 1.0 - (e.age / e.ttl).clamp(0.0, 1.0);
            match e.kind {
                EffectKind::MuzzleFlash { pos } => sprites.push(FxSprite {
                    pos,
                    size: 14.0 + 6.0 * f,
                    color: [1.0, 0.85, 0.4, 0.9 * f],
                }),
                EffectKind::Tracer { a, b } => beams.push(FxBeam {
                    a,
                    b,
                    width: 1.6,
                    color: [1.0, 0.9, 0.55, 0.7 * f],
                }),
                EffectKind::Impact { pos } => sprites.push(FxSprite {
                    pos,
                    size: 6.0 + 6.0 * (1.0 - f),
                    color: [0.9, 0.8, 0.6, 0.8 * f],
                }),
                EffectKind::BloodPuff { pos } => sprites.push(FxSprite {
                    pos,
                    size: 10.0 + 8.0 * (1.0 - f),
                    color: [0.75, 0.1, 0.05, 0.8 * f],
                }),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Procedural rifle viewmodel (geometry only; platforms upload/draw it)
// ---------------------------------------------------------------------------

/// Material palette (one texel per entry on desktop; vertex colors on PSP).
pub const GUN_COLORS: [[u8; 4]; 6] = [
    [38, 38, 42, 255],   // 0 receiver: gunmetal
    [22, 22, 24, 255],   // 1 barrel: near-black
    [82, 58, 38, 255],   // 2 wood furniture
    [55, 55, 60, 255],   // 3 magazine
    [30, 30, 33, 255],   // 4 grip/sights
    [140, 120, 90, 255], // 5 accent
];

/// Where the muzzle sits in viewmodel-local space (gun points -Z).
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(0.0, 0.6, -31.0);

/// One box of the rifle, in viewmodel-local space.
#[derive(Clone, Copy, Debug)]
pub struct RifleBox {
    pub min: Vec3,
    pub max: Vec3,
    /// Index into [`GUN_COLORS`].
    pub color: usize,
}

/// The rifle as boxes (receiver, barrel, furniture, ...). Kept as data so
/// each backend builds its own vertex format from one source of truth.
pub fn rifle_boxes() -> [RifleBox; 10] {
    let b = |min: Vec3, max: Vec3, color: usize| RifleBox { min, max, color };
    [
        // Receiver.
        b(Vec3::new(-1.3, -2.0, -16.0), Vec3::new(1.3, 1.6, 4.0), 0),
        // Barrel + muzzle.
        b(Vec3::new(-0.45, 0.1, -30.0), Vec3::new(0.45, 1.0, -16.0), 1),
        b(Vec3::new(-0.65, -0.05, -32.0), Vec3::new(0.65, 1.15, -30.0), 4),
        // Wood handguard under the barrel.
        b(Vec3::new(-0.95, -1.3, -26.0), Vec3::new(0.95, 0.1, -16.0), 2),
        // Magazine (slightly raked).
        b(Vec3::new(-0.95, -6.4, -10.5), Vec3::new(0.95, -2.0, -6.0), 3),
        // Pistol grip.
        b(Vec3::new(-0.85, -5.2, -1.2), Vec3::new(0.85, -2.0, 1.6), 4),
        // Stock.
        b(Vec3::new(-1.05, -2.4, 4.0), Vec3::new(1.05, 1.0, 12.5), 2),
        // Front sight + rear sight.
        b(Vec3::new(-0.18, 1.0, -29.4), Vec3::new(0.18, 2.2, -28.6), 4),
        b(Vec3::new(-0.5, 1.6, -6.0), Vec3::new(0.5, 2.3, -4.8), 4),
        // Carry-handle hint above receiver.
        b(Vec3::new(-0.4, 1.6, -4.0), Vec3::new(0.4, 2.0, 2.0), 0),
    ]
}

/// A tiny deterministic PRNG (xorshift) — reproducible headless runs.
#[derive(Clone)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1 << 24) as f32
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.f32() * (hi - lo)
    }
}
