use pixel_painter::drawing::{Drawing, HEIGHT, PIXELS, Viewport, WIDTH};

#[test]
fn fast_strokes_have_no_gaps_and_undo_as_a_whole() {
    let mut drawing = Drawing::new();
    drawing.begin_stroke(0, 0, 1, 1);
    drawing.continue_stroke(20, 20, 1, 1);
    drawing.end_stroke();
    for coordinate in 0..=20 {
        assert_eq!(drawing.pixels()[coordinate * WIDTH + coordinate], 1);
    }
    assert!(drawing.is_dirty());
    assert!(drawing.undo());
    assert!(drawing.pixels().iter().all(|&pixel| pixel == 0));
    assert!(!drawing.is_dirty());
    assert!(!drawing.undo());
}

#[test]
fn brush_clips_at_edges_and_clear_is_undoable() {
    let mut drawing = Drawing::new();
    drawing.begin_stroke(WIDTH as i32 - 1, HEIGHT as i32 - 1, 2, 5);
    drawing.end_stroke();
    assert_eq!(drawing.pixels()[PIXELS - 1], 2);
    assert_eq!(drawing.pixels()[0], 0);
    let before = drawing.pixels().to_vec();
    drawing.clear();
    assert!(drawing.pixels().iter().all(|&pixel| pixel == 0));
    drawing.undo();
    assert_eq!(drawing.pixels(), before);
}

#[test]
fn validated_file_round_trip_and_failed_open_preserves_work() {
    let mut drawing = Drawing::new();
    drawing.begin_stroke(10, 20, 3, 3);
    drawing.end_stroke();
    let mut bytes = drawing.encode();
    let mut restored = Drawing::new();
    restored.replace_from_bytes(&bytes).unwrap();
    assert_eq!(restored.pixels(), drawing.pixels());
    assert!(!restored.is_dirty());
    bytes[8] = 255;
    assert!(restored.replace_from_bytes(&bytes).is_err());
    assert_eq!(restored.pixels(), drawing.pixels());
    assert!(restored.replace_from_bytes(&bytes[..7]).is_err());
    bytes[8] = 0;
    bytes.push(0);
    assert!(restored.replace_from_bytes(&bytes).is_err());
}

#[test]
fn undo_restores_dirty_state_across_save() {
    let mut drawing = Drawing::new();
    drawing.begin_stroke(1, 1, 1, 1);
    drawing.end_stroke();
    drawing.mark_saved();
    drawing.begin_stroke(2, 2, 2, 1);
    drawing.end_stroke();
    drawing.undo();
    assert!(!drawing.is_dirty());
    drawing.begin_stroke(3, 3, 2, 1);
    drawing.end_stroke();
    drawing.mark_saved();
    drawing.undo();
    assert!(drawing.is_dirty());
}

#[test]
fn viewport_preserves_aspect_and_maps_rotated_or_small_windows() {
    for (width, height) in [(480, 200), (200, 480), (80, 50)] {
        let viewport = Viewport::new(17, 23, width, height);
        assert_eq!(viewport.map(viewport.x, viewport.y), Some((0, 0)));
        assert!(viewport.map(viewport.x - 1, viewport.y).is_none());
        assert!(
            viewport
                .map(viewport.x + viewport.width, viewport.y)
                .is_none()
        );
        let (x, y) = viewport
            .map(
                viewport.x + viewport.width - 1,
                viewport.y + viewport.height - 1,
            )
            .unwrap();
        assert!(x < WIDTH as i32 && y < HEIGHT as i32);
    }
    assert!(Viewport::new(0, 0, 0, 0).map(0, 0).is_none());
}
