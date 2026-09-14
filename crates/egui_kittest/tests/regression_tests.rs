use egui::accesskit::Role;
use egui::{Button, ComboBox, Image, Pos2, Rect, Scene, Vec2, Widget};
use egui_kittest::{kittest::Queryable, Harness, SnapshotResults};

#[test]
pub fn focus_should_skip_over_disabled_buttons() {
    let mut harness = Harness::new_ui(|ui| {
        ui.add(Button::new("Button 1"));
        ui.add_enabled(false, Button::new("Button Disabled"));
        ui.add(Button::new("Button 3"));
    });

    harness.press_key(egui::Key::Tab);
    harness.run();

    let button_1 = harness.get_by_label("Button 1");
    assert!(button_1.is_focused());

    harness.press_key(egui::Key::Tab);
    harness.run();

    let button_3 = harness.get_by_label("Button 3");
    assert!(button_3.is_focused());

    harness.press_key(egui::Key::Tab);
    harness.run();

    let button_1 = harness.get_by_label("Button 1");
    assert!(button_1.is_focused());
}

#[test]
fn image_failed() {
    let mut harness = Harness::new_ui(|ui| {
        Image::new("file://invalid/path")
            .alt_text("I have an alt text")
            .max_size(Vec2::new(100.0, 100.0))
            .ui(ui);
    });

    harness.run();
    harness.fit_contents();

    #[cfg(all(feature = "wgpu", feature = "snapshot"))]
    harness.snapshot("image_snapshots");
}

#[test]
fn test_combobox() {
    let items = ["Item 1", "Item 2", "Item 3"];
    let mut harness = Harness::builder()
        .with_size(Vec2::new(300.0, 200.0))
        .build_ui_state(
            |ui, selected| {
                ComboBox::new("combobox", "Select Something").show_index(
                    ui,
                    selected,
                    items.len(),
                    |idx| *items.get(idx).expect("Invalid index"),
                );
            },
            0,
        );

    harness.run();

    let mut results = SnapshotResults::new();

    #[cfg(all(feature = "wgpu", feature = "snapshot"))]
    results.add(harness.try_snapshot("combobox_closed"));

    let combobox = harness.get_by_role_and_label(Role::ComboBox, "Select Something");
    combobox.click();

    harness.run();

    #[cfg(all(feature = "wgpu", feature = "snapshot"))]
    results.add(harness.try_snapshot("combobox_opened"));

    let item_2 = harness.get_by_role_and_label(Role::Button, "Item 2");
    // Node::click doesn't close the popup, so we use simulate_click
    item_2.simulate_click();

    harness.run();

    assert_eq!(harness.state(), &1);

    // Popup should be closed now
    assert!(harness.query_by_label("Item 2").is_none());
}

/// Regression test: when a `Window` is collapsed, its content `Ui`'s clip rect is animated
/// down to nothing (see `CollapsingState::show_body_unindented`). Plain content respects this,
/// but a `Scene` nested inside the window has its own clip rect, computed from its own
/// allocated size rather than intersected with the ambient (animating) clip rect - so its
/// content stays fully visible throughout the collapse animation instead of disappearing with
/// the rest of the window.
#[test]
fn window_collapse_should_clip_scene_content() {
    use std::cell::RefCell;

    let scene_rect = RefCell::new(Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 100.0)));
    let label_clip_height = RefCell::new(f32::INFINITY);
    let scene_clip_height = RefCell::new(f32::INFINITY);

    let mut harness = Harness::builder()
        .with_size(Vec2::new(400.0, 400.0))
        // The collapse animation (`Style::animation_time`, ~0.083s) needs to span several
        // simulated frames for us to observe it mid-flight; the default step_dt (0.25s) would
        // jump straight from fully open to fully closed in a single step.
        .with_step_dt(1.0 / 60.0)
        .build_ui(|ui| {
            egui::Window::new("collapse_test").show(ui.ctx(), |ui| {
                ui.label("content");
                let mut h = label_clip_height.borrow_mut();
                *h = h.min(ui.clip_rect().height());
                drop(h);

                let mut rect = *scene_rect.borrow();
                Scene::new().show(ui, &mut rect, |ui| {
                    ui.label("scene content");
                    let mut h = scene_clip_height.borrow_mut();
                    *h = h.min(ui.clip_rect().height());
                });
                *scene_rect.borrow_mut() = rect;
            });
        });

    harness.run();
    let initial_label_height = *label_clip_height.borrow();
    let initial_scene_height = *scene_clip_height.borrow();

    // Click the window's collapse button (accessible label "Hide" while expanded), then step
    // through the resulting animation, recording the *smallest* clip height seen along the way.
    harness.get_by_label("Hide").click();
    harness.run_steps(20);

    let min_label_height = *label_clip_height.borrow();
    let min_scene_height = *scene_clip_height.borrow();

    assert!(
        min_label_height < initial_label_height * 0.1,
        "regular content's clip rect should shrink to near-nothing while collapsing: {initial_label_height} -> {min_label_height}"
    );
    assert!(
        min_scene_height < initial_scene_height * 0.1,
        "Scene content's clip rect should shrink to near-nothing while collapsing, same as regular content: {initial_scene_height} -> {min_scene_height}"
    );
}

/// Investigation for a second symptom reported alongside the `Scene` clip bug: content appears
/// to jump upward by a few pixels on the very first frame of a `Window` collapse. Toggles the
/// persisted `CollapsingState` directly (bypassing the multi-event pointer simulation of an
/// actual click, which by itself can span several simulated frames) so we can isolate exactly
/// one frame's worth of change.
#[test]
fn window_collapse_content_position_on_first_frame() {
    use std::cell::RefCell;

    let content_top = RefCell::new(0.0_f32);

    let mut harness = Harness::builder()
        .with_size(Vec2::new(400.0, 400.0))
        .with_step_dt(1.0 / 60.0)
        .build_ui(|ui| {
            egui::Window::new("collapse_test_jump").show(ui.ctx(), |ui| {
                *content_top.borrow_mut() = ui.cursor().top();
                ui.label("content");
            });
        });

    harness.run();
    let before = *content_top.borrow();

    // Toggle the same persisted state a real click on the collapse button would, without going
    // through the (multi-frame) pointer-event simulation.
    let collapsing_id = egui::Id::new("collapse_test_jump").with("collapsing");
    let mut state =
        egui::collapsing_header::CollapsingState::load(&harness.ctx, collapsing_id).unwrap();
    state.set_open(false);
    state.store(&harness.ctx);

    harness.step();
    let after_one_frame = *content_top.borrow();
    harness.run_steps(20); // let the animation fully settle
    let after_full_collapse = *content_top.borrow();

    let total_delta = (before - after_full_collapse).abs();
    let first_frame_delta = (before - after_one_frame).abs();

    assert!(
        first_frame_delta < total_delta * 0.7,
        "content's top position moved {first_frame_delta} of {total_delta} total pixels in a \
         single frame - it should animate smoothly across several frames rather than jump most \
         of the way there in one (before: {before}, after one frame: {after_one_frame}, fully \
         collapsed: {after_full_collapse})"
    );
}
