//! SPEC-022 in the shell: the map view drawn through the session's
//! renderer is the same on both paths, frame for frame (§3.1 rule 3,
//! §4 T10/T11 over the shell's own scenes), and drawing never changes the
//! world (§3.13, §4 T13).
use super::*;
use crate::map_preview::test_support::{
    cpu_renderer, gpu_lock, gpu_renderer, interactive, synthetic_corpus,
};
use vault_render::IndexedImage;
use winit::keyboard::KeyCode;

/// A short play session on `preview`, each frame composed as the shell
/// composes it, with how its refreshes were drawn: the first frame, the
/// in-world cursor over the map (its red translucent outline), a walk
/// ordered by a click and ticked frame by frame (the player reinserted on
/// each animation step), the roofs toggled off and on, and the elevation
/// changed and back.
fn session_frames(preview: &mut MapPreview) -> Vec<(IndexedImage, FrameRender)> {
    let mut frames = Vec::new();
    let mut shot = |preview: &mut MapPreview| {
        let image = preview.image().unwrap();
        frames.push((image, preview.last_frame_render()));
    };
    let mut now = 16;
    preview.tick(now).unwrap();
    shot(preview);
    for point in [[300, 200], [250, 160], [420, 240]] {
        preview.pointer_moved(point);
        shot(preview);
    }
    preview.click([250, 160], false);
    for _ in 0..40 {
        now += 16;
        preview.tick(now).unwrap();
        shot(preview);
    }
    for key in [
        KeyCode::KeyR,
        KeyCode::KeyR,
        KeyCode::PageUp,
        KeyCode::PageDown,
    ] {
        preview.step(key);
        shot(preview);
    }
    frames
}

/// Both sessions' frames are equal, frame for frame. Every frame of the
/// instanced session drew one refresh, instanced or (refused) by the
/// reference path; the counts are returned.
fn assert_same_frames(
    reference: &[(IndexedImage, FrameRender)],
    instanced: &[(IndexedImage, FrameRender)],
) -> FrameRender {
    assert_eq!(reference.len(), instanced.len());
    let mut totals = FrameRender::default();
    for (index, ((expected, drawn), (image, counts))) in reference.iter().zip(instanced).enumerate()
    {
        assert_eq!(
            *drawn,
            FrameRender {
                reference: 1,
                ..FrameRender::default()
            },
            "frame {index}: the reference session draws on the reference path"
        );
        assert_eq!(counts.refreshes(), 1, "frame {index}: {counts:?}");
        assert_eq!(counts.reference, 0, "frame {index}: {counts:?}");
        if counts.instanced == 1 {
            assert!(counts.instances > 0 && counts.draws > 0, "{counts:?}");
            assert!(counts.draws <= counts.instances, "{counts:?}");
        }
        assert!(
            expected.pixels() == image.pixels(),
            "frame {index} differs between the paths ({counts:?})"
        );
        totals.instanced += counts.instanced;
        totals.refused += counts.refused;
        totals.instances += counts.instances;
        totals.draws += counts.draws;
        totals.snapshots += counts.snapshots;
    }
    totals
}

#[test]
fn the_reference_path_is_the_default() {
    let dir = synthetic_corpus();
    let mut preview = interactive(dir.path());
    assert_eq!(preview.scene_renderer().path(), RenderPath::Reference);
    preview.tick(16).unwrap();
    preview.image().unwrap();
    assert_eq!(
        preview.last_frame_render(),
        FrameRender {
            reference: 1,
            ..FrameRender::default()
        }
    );
    let metrics = preview.render_metrics();
    assert_eq!(
        (metrics.path.as_str(), metrics.executor),
        ("reference", None)
    );
    assert!(metrics.refusals.is_empty());
    assert_eq!(preview.scene_renderer().session_totals().reference, 1);
}

/// The CPU executor draws the instanced path's frames on any host: every
/// frame of the session equals the reference path's, and the supported
/// frames (here all of them: the Vanilla view has no shroud or roof peek)
/// were drawn by the instanced path, with fewer draw calls than instances.
#[test]
fn the_map_view_is_the_same_on_both_paths() {
    let dir = synthetic_corpus();
    let mut reference = interactive(dir.path());
    let expected = session_frames(&mut reference);
    let mut instanced = interactive(dir.path());
    let renderer = cpu_renderer();
    instanced.set_scene_renderer(renderer.clone());
    let frames = session_frames(&mut instanced);
    let totals = assert_same_frames(&expected, &frames);
    assert_eq!(totals.instanced, frames.len(), "{totals:?}");
    assert_eq!(totals.refused, 0);
    assert!(totals.draws < totals.instances, "{totals:?}");
    // The translucent cursor outline reads the canvas: snapshot copies.
    assert!(totals.snapshots > 0, "{totals:?}");
    assert_eq!(renderer.session_totals().instanced, frames.len());
    assert!(renderer.logged_causes().is_empty());
    let metrics = instanced.render_metrics();
    assert_eq!(metrics.path, "instanced");
    assert_eq!(metrics.executor.as_deref(), Some("CPU executor"));
    // The same world state at the end of both sessions.
    assert_eq!(
        reference.host.world.state_hash(),
        instanced.host.world.state_hash()
    );
}

/// SPEC-022 §4 T13: the world's `state_hash` is the same before and after
/// 100 refreshes on each path.
#[test]
fn drawing_never_changes_the_world_state() {
    let dir = synthetic_corpus();
    for renderer in [SharedRenderer::default(), cpu_renderer()] {
        let mut preview = interactive(dir.path());
        preview.set_scene_renderer(renderer.clone());
        preview.tick(16).unwrap();
        preview.pointer_moved([300, 200]);
        let before = preview.host.world.state_hash();
        for _ in 0..100 {
            preview.image().unwrap();
        }
        assert_eq!(preview.host.world.state_hash(), before, "{renderer:?}");
        let totals = renderer.session_totals();
        assert!(totals.refreshes() >= 100, "{totals:?}");
    }
}

/// SPEC-022 §4 T11 in the shell: the GPU executor draws the same frames
/// (a GPU test: it skips without an adapter unless `VAULT_REQUIRE_GPU=1`).
#[test]
fn the_gpu_draws_the_map_view_as_the_reference_does() {
    let _gpu = gpu_lock();
    let Some(gpu) = gpu_renderer() else {
        return;
    };
    let dir = synthetic_corpus();
    let mut reference = interactive(dir.path());
    let expected = session_frames(&mut reference);
    let mut instanced = interactive(dir.path());
    let renderer = SharedRenderer::from_renderer(gpu);
    instanced.set_scene_renderer(renderer.clone());
    let frames = session_frames(&mut instanced);
    let totals = assert_same_frames(&expected, &frames);
    // A lost or refusing device falls back frame by frame, still equal;
    // a working one draws every frame.
    let causes = renderer.logged_causes();
    if causes.is_empty() {
        assert_eq!(totals.instanced, frames.len(), "{totals:?}");
    }
    println!("GPU map view: {totals:?}, fallbacks {causes:?}");
    let metrics = instanced.render_metrics();
    assert!(
        metrics.executor.as_deref().is_some_and(
            |executor| executor.starts_with("GPU ") || executor == "no GPU device: reference"
        ),
        "{metrics:?}"
    );
    drop(instanced);
    drop(renderer);
}
