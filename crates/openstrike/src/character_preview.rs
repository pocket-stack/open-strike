//! Map-independent proof that the shipped GLB clips load and animate in Pocket3D.
use crate::{args::Args, scripts::Headless};
use anyhow::{Context, Result, ensure};
use openstrike_core::bot::ActorClip;
use pocket3d::prelude::*;

pub fn run(args: &Args) -> Result<()> {
    let out = args.screenshot.as_deref().unwrap_or("out/character");
    std::fs::create_dir_all(out)?;
    let mut hl = Headless::new(args.size)?;
    let path =
        crate::args::find_asset("characters/police/officer.glb").context("officer.glb missing")?;
    let asset = ModelAsset::load_glb(
        &hl.gpu,
        &hl.renderer.model_material_layout,
        &hl.renderer.samplers,
        &path,
    )?;
    ensure!(
        asset.clips.len() == 7,
        "expected seven officer actions, found {}",
        asset.clips.len()
    );
    let mut scene = Scene::default();
    scene.sky.zenith = Vec3::splat(0.08);
    scene.sky.horizon = Vec3::splat(0.12);
    scene.lighting.sun_dir = Vec3::new(-0.4, 0.7, -0.8).normalize();
    scene.lighting.ambient = Vec3::splat(0.6);
    let mut camera = Camera {
        pos: Vec3::new(92.0, 63.0, -135.0),
        fov_y: 40f32.to_radians(),
        ..Default::default()
    };
    camera.look_at(Vec3::new(0.0, 36.0, 0.0));
    let hud = Hud::default();
    for clip in ActorClip::ALL {
        let index = asset
            .clips
            .iter()
            .position(|c| c.name == clip.name())
            .with_context(|| format!("missing {}", clip.name()))?;
        let mut pixels = Vec::new();
        let fractions: Vec<f32> =
            if matches!(clip, ActorClip::Walk | ActorClip::Run | ActorClip::Death) {
                let frames = (openstrike_character::duration(clip) * 24.0).round() as usize;
                (0..=frames).map(|i| i as f32 / frames as f32).collect()
            } else {
                vec![0.0, 0.37, 0.73]
            };
        for (frame, fraction) in fractions.into_iter().enumerate() {
            scene.models.clear();
            let mut instance = ModelInstance::new(asset.clone());
            instance.transform = Mat4::from_scale(Vec3::splat(70.0 / asset.height()));
            instance.anim = AnimState {
                clip: index,
                time: openstrike_character::duration(clip) * fraction,
                speed: 1.0,
                looping: false,
            };
            scene.models.push(instance);
            hl.renderer
                .render(&hl.gpu, &hl.target.view, args.size, &scene, &camera, &hud);
            hl.target.save_png(
                &hl.gpu,
                &std::path::Path::new(out).join(format!("{}-{frame:02}.png", clip.name())),
            )?;
            pixels.push(hl.target.read_rgba(&hl.gpu)?);
        }
        let changed = pixels[0]
            .chunks_exact(4)
            .zip(pixels[pixels.len() / 2].chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        ensure!(
            changed > 20,
            "{} does not visibly animate ({changed} pixels)",
            clip.name()
        );
        println!(
            "PASS {}: {changed} changed pixels in shipped Pocket3D skinning",
            clip.name()
        );
    }
    println!("CHARACTER SCRIPT PASSED: {out}");
    Ok(())
}
