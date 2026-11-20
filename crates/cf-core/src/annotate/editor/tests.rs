use super::*;

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn drag(state: &mut EditorState, from: Vec2, to: Vec2) {
    state.pointer_down(from, 1.0);
    state.pointer_move(v(f32::midpoint(from.x, to.x), f32::midpoint(from.y, to.y)));
    state.pointer_up(to);
}

#[test]
fn drawing_a_rect_is_one_undoable_edit() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Rect);
    drag(&mut s, v(10.0, 10.0), v(50.0, 40.0));
    assert_eq!(s.document().items().len(), 1);
    assert!(s.preview().is_none());
    assert!(s.undo());
    assert!(s.document().items().is_empty());
}

#[test]
fn a_click_with_a_drawing_tool_adds_nothing() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Arrow);
    drag(&mut s, v(10.0, 10.0), v(10.0, 10.0));
    assert!(s.document().items().is_empty());
}

#[test]
fn pen_collects_points_and_previews_while_drawing() {
    let mut s = EditorState::new();
    s.pointer_down(v(0.0, 0.0), 1.0);
    s.pointer_move(v(0.5, 0.0)); // too close: skipped
    s.pointer_move(v(5.0, 0.0));
    let preview = s.preview().map(|p| p.item.shape);
    assert_eq!(preview, Some(Shape::Pen(vec![v(0.0, 0.0), v(5.0, 0.0)])));
    s.pointer_up(v(9.0, 0.0));
    assert_eq!(s.document().items().len(), 1);
}

#[test]
fn select_tool_moves_and_deletes() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Line);
    drag(&mut s, v(0.0, 0.0), v(100.0, 0.0));
    s.set_tool(Tool::Select);
    drag(&mut s, v(50.0, 1.0), v(50.0, 21.0));
    assert_eq!(s.selected(), Some(0));
    let moved = &s.document().items()[0].shape;
    assert_eq!(
        moved,
        &Shape::Line {
            from: v(0.0, 20.0),
            to: v(100.0, 20.0)
        }
    );
    s.delete_selected();
    assert!(s.document().items().is_empty());
    assert!(s.undo());
    assert_eq!(s.document().items().len(), 1);
}

#[test]
fn restyling_the_selection_is_undoable() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Step);
    s.pointer_down(v(10.0, 10.0), 1.0);
    s.set_tool(Tool::Select);
    s.pointer_down(v(10.0, 10.0), 1.0);
    s.pointer_up(v(10.0, 10.0));
    let wide = Style {
        width: 8.0,
        ..s.style()
    };
    s.set_style(wide);
    assert!((s.document().items()[0].style.width - 8.0).abs() < f32::EPSILON);
    s.undo();
    assert!((s.document().items()[0].style.width - 4.0).abs() < f32::EPSILON);
}

#[test]
fn text_is_typed_committed_and_re_edited() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Text);
    s.pointer_down(v(10.0, 10.0), 1.0);
    s.type_text("Hi");
    s.new_line();
    s.type_text("there\u{8}");
    assert!(s.preview().is_some_and(|p| p.typing));
    assert!(s.escape());
    let text = |s: &EditorState| match &s.document().items()[0].shape {
        Shape::Text { text, .. } => text.clone(),
        _ => String::new(),
    };
    assert_eq!(text(&s), "Hi\nthere");
    s.pointer_down(v(12.0, 12.0), 1.0); // click on it: edit
    s.backspace();
    s.commit_text();
    assert_eq!(text(&s), "Hi\nther");
    assert_eq!(s.document().items().len(), 1);
}

#[test]
fn empty_text_is_dropped_and_escape_reports_nothing_to_do() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Text);
    s.pointer_down(v(10.0, 10.0), 1.0);
    assert!(s.escape());
    assert!(s.document().items().is_empty());
    assert!(!s.escape());
}

#[test]
fn highlighter_keeps_its_own_style() {
    let mut s = EditorState::new();
    s.set_tool(Tool::Highlighter);
    let marker = s.style();
    assert_ne!(marker.color, Style::default().color);
    s.set_style(Style {
        width: 8.0,
        ..marker
    });
    drag(&mut s, v(0.0, 0.0), v(40.0, 0.0));
    assert!(matches!(s.document().items()[0].shape, Shape::Highlight(_)));
    assert!((s.document().items()[0].style.width - 8.0).abs() < f32::EPSILON);
    s.set_tool(Tool::Pen);
    assert_eq!(s.style(), Style::default());
}

#[test]
fn crop_is_clamped_undoable_and_removable() {
    let mut s = EditorState::with_canvas(200.0, 100.0);
    s.set_tool(Tool::Crop);
    drag(&mut s, v(-20.0, 10.4), v(50.0, 300.0));
    let crop = s.document().crop().expect("cropped");
    assert_eq!((crop.min, crop.max), (v(0.0, 10.0), v(50.0, 100.0)));
    drag(&mut s, v(5.0, 5.0), v(7.0, 7.0)); // a click: no change
    assert_eq!(s.document().crop(), Some(crop));
    s.delete_selected();
    assert_eq!(s.document().crop(), None);
    assert!(s.undo());
    assert_eq!(s.document().crop(), Some(crop));
    drag(&mut s, v(-5.0, -5.0), v(500.0, 500.0)); // the whole image: no crop
    assert_eq!(s.document().crop(), None);
}

#[test]
fn crop_preview_follows_the_drag() {
    let mut s = EditorState::with_canvas(100.0, 100.0);
    s.set_tool(Tool::Crop);
    s.pointer_down(v(10.0, 10.0), 1.0);
    s.pointer_move(v(60.0, 40.0));
    let preview = s.crop_preview().expect("dragging");
    assert_eq!(preview.max, v(60.0, 40.0));
    assert!(s.preview().is_none());
}

#[test]
fn magnify_inset_goes_right_or_left_of_its_source() {
    let mut s = EditorState::with_canvas(400.0, 300.0);
    s.set_tool(Tool::Magnify);
    drag(&mut s, v(10.0, 20.0), v(60.0, 50.0));
    let Shape::Magnify { at, .. } = s.document().items()[0].shape else {
        panic!("expected a magnify item");
    };
    assert_eq!(at, v(76.0, 20.0));
    drag(&mut s, v(300.0, 280.0), v(380.0, 290.0));
    let Shape::Magnify { at, .. } = s.document().items()[1].shape else {
        panic!("expected a magnify item");
    };
    assert_eq!(at, v(124.0, 280.0));
}

#[test]
fn effect_tools_add_region_items() {
    let mut s = EditorState::new();
    for tool in [Tool::Blur, Tool::Redact, Tool::Spotlight] {
        s.set_tool(tool);
        drag(&mut s, v(0.0, 0.0), v(30.0, 30.0));
    }
    let kinds: Vec<_> = s
        .document()
        .items()
        .iter()
        .map(|a| std::mem::discriminant(&a.shape))
        .collect();
    assert_eq!(kinds.len(), 3);
    assert!(kinds[0] != kinds[1] && kinds[1] != kinds[2]);
}
