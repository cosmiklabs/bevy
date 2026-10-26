//! A test that a [`StandardMaterial`] compiles for every combination of UV
//! channels a mesh can have: none, only `UV_0`, only `UV_1`, or both, with
//! textures read from either channel, in the forward, prepass and deferred
//! pipelines.
//!
//! It renders a few frames without a window and exits with an error if any
//! render pipeline fails to compile. This is run in CI to ensure that the
//! shaders don't assume `UV_0` is present whenever `UV_1` is.
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use bevy::{
    app::ScheduleRunnerPlugin,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    core_pipeline::prepass::{DeferredPrepass, DepthPrepass, NormalPrepass},
    image::Image,
    mesh::UvChannel,
    prelude::*,
    render::{
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{
            CachedPipelineState, Extent3d, PipelineCache, TextureDimension, TextureFormat,
        },
        Render, RenderApp, RenderSystems,
    },
    shader::ShaderCacheError,
    window::ExitCondition,
    winit::WinitPlugin,
};

/// Frames in a row with no pipeline compiling before the test passes.
const SETTLED_FRAMES: u32 = 10;
/// Frames to wait for every pipeline to compile before giving up.
const MAX_FRAMES: u32 = 3000;

#[derive(Resource, Clone, Default)]
struct PipelineStatus {
    failed: Arc<AtomicBool>,
    settled_frames: Arc<AtomicU32>,
}

fn main() {
    let status = PipelineStatus::default();

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>(),
        ScheduleRunnerPlugin::run_loop(std::time::Duration::ZERO),
    ))
    .insert_resource(status.clone())
    .add_systems(Startup, setup)
    .add_systems(Update, exit_when_settled);

    app.sub_app_mut(RenderApp)
        .insert_resource(status)
        .add_systems(Render, check_pipelines.after(RenderSystems::Render));

    let exit = app.run();
    if !exit.is_success() {
        std::process::exit(1);
    }
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = images.add(Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ));

    // Tangents are generated from `UV_0`, before it's moved.
    let sphere = Mesh::from(Sphere::new(0.5))
        .with_generated_tangents()
        .unwrap();
    let uvs = sphere.attribute(Mesh::ATTRIBUTE_UV_0).unwrap().clone();
    let only_uv_0 = sphere.clone();
    let only_uv_1 = sphere
        .clone()
        .with_removed_attribute(Mesh::ATTRIBUTE_UV_0)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvs.clone());
    let both = sphere
        .clone()
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvs);
    let neither = sphere.with_removed_attribute(Mesh::ATTRIBUTE_UV_0);
    let meshes = [only_uv_0, only_uv_1, both, neither].map(|mesh| meshes.add(mesh));

    let materials = [UvChannel::Uv0, UvChannel::Uv1].map(|channel| {
        materials.add(StandardMaterial {
            base_color_texture: Some(texture.clone()),
            base_color_channel: channel.clone(),
            normal_map_texture: Some(texture.clone()),
            normal_map_channel: channel.clone(),
            depth_map: Some(texture.clone()),
            parallax_depth_scale: 0.05,
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        })
    });

    for (i, mesh) in meshes.iter().enumerate() {
        for (j, material) in materials.iter().enumerate() {
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(i as f32 - 1.5, j as f32 - 0.5, 0.0),
            ));
        }
    }

    commands.spawn((
        DirectionalLight::default(),
        Transform::from_xyz(1.0, 2.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let transform = Transform::from_xyz(0.0, 0.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y);
    let target = |images: &mut Assets<Image>| {
        RenderTarget::Image(
            images
                .add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                ))
                .into(),
        )
    };
    // Forward, with a prepass.
    commands.spawn((
        Camera3d::default(),
        target(&mut images),
        transform,
        DepthPrepass,
        NormalPrepass,
    ));
    // Deferred.
    commands.spawn((
        Camera3d::default(),
        target(&mut images),
        transform,
        Msaa::Off,
        DepthPrepass,
        DeferredPrepass,
    ));
}

fn check_pipelines(pipeline_cache: Res<PipelineCache>, status: Res<PipelineStatus>) {
    let mut compiling = false;
    for pipeline in pipeline_cache.pipelines() {
        match &pipeline.state {
            // Retried once the shader has loaded.
            CachedPipelineState::Err(
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable,
            ) => compiling = true,
            CachedPipelineState::Err(error) => {
                error!("a pipeline failed to compile: {error}");
                status.failed.store(true, Ordering::Relaxed);
            }
            CachedPipelineState::Ok(_) => {}
            _ => compiling = true,
        }
    }
    if compiling {
        status.settled_frames.store(0, Ordering::Relaxed);
    } else {
        status.settled_frames.fetch_add(1, Ordering::Relaxed);
    }
}

fn exit_when_settled(
    status: Res<PipelineStatus>,
    mut frames: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *frames += 1;
    if status.failed.load(Ordering::Relaxed) {
        exit.write(AppExit::error());
    } else if status.settled_frames.load(Ordering::Relaxed) >= SETTLED_FRAMES {
        info!("every pipeline compiled");
        exit.write(AppExit::Success);
    } else if *frames >= MAX_FRAMES {
        error!("pipelines were still compiling after {MAX_FRAMES} frames");
        exit.write(AppExit::error());
    }
}
