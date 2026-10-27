//! The window driven headlessly with GPUI's test platform: clicks, keys and
//! what the panels, bars and dialogs show.

use crate::camera::Framing;
use crate::live::{self, File, Files};
use crate::model::Model;
use crate::panel::Content;
use crate::viewer::{Dialog, Menu, Viewer};
use gpui::{point, px, Entity, Modifiers, Pixels, Point, TestAppContext, VisualTestContext};
use std::path::PathBuf;
use std::rc::Rc;

const MODEL_PY: &str = "class Block:\n    def __init__(self):\n        self.attn = Attention()\n\n    def forward(self, x):\n        x = x + self.attn(x)\n        x = x + self.mlp(x)\n        return x\n\n\nclass Attention:\n    def __call__(self, x):\n        q = self.q(x)\n        return q\n";
const TRAIN_PY: &str = "from model import Block\nb = Block()\nb.forward(1)\n";

fn files(list: &[(&str, &str)]) -> Files {
    list.iter().map(|(p, t)| (p.to_string(), File { text: Some(t.to_string()), bytes: t.len() as u64 })).collect()
}

fn model(f: &Files) -> Model {
    Model::new(blockviz_core::analyze("t", live::inputs(f)), live::texts(f))
}

/// Draws the window as the next frame would (the test platform draws only on input),
/// twice: a frame can ask for another, as the breadcrumbs do when the view moved.
fn frame(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear());
    }
    cx.run_until_parked();
}

/// Settings are kept under the home folder: tests get one of their own, not the reader's.
fn sandbox_home() {
    static HOME: std::sync::Once = std::sync::Once::new();
    HOME.call_once(|| {
        let home = std::env::temp_dir().join(format!("blockviz-test-home-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&home);
        std::env::set_var("HOME", home);
    });
}

fn open(cx: &mut TestAppContext) -> (Entity<Viewer>, &mut VisualTestContext) {
    sandbox_home();
    cx.update(|cx| {
        crate::input::bind_keys(cx);
        crate::menus::bind(cx);
    });
    let f = files(&[("model.py", MODEL_PY), ("train.py", TRAIN_PY)]);
    let m = model(&f);
    let (view, cx) = cx.add_window_view(|window, cx| Viewer::new(window, cx));
    view.update(cx, |v, cx| v.show(PathBuf::from("/tmp/t"), f, m, cx));
    frame(cx);
    (view, cx)
}

fn titled(v: &Viewer, title: &str) -> u32 {
    v.m.as_ref().unwrap().scene.nodes.iter().find(|n| n.title == title).unwrap().id
}

fn center(b: gpui::Bounds<Pixels>) -> Point<Pixels> {
    point(b.origin.x + b.size.width / 2.0, b.origin.y + b.size.height / 2.0)
}

/// Finishes the flight under way, as its last frame would.
fn land(view: &Entity<Viewer>, cx: &mut VisualTestContext) {
    view.update(cx, |v, cx| {
        if let Some((_, plan)) = v.flight.take() {
            let (w, h) = (v.width, v.height);
            let m = v.m.clone().unwrap();
            v.camera.step(&m, &plan, 1.0, w, h);
            cx.notify();
        }
    });
    frame(cx);
}

fn content(view: &Entity<Viewer>, cx: &mut VisualTestContext) -> Content {
    view.read_with(cx, |v, cx| v.inspector.read(cx).content())
}

#[gpui::test]
fn a_click_opens_the_panel_and_a_line_of_source_selects_its_block(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let (file, at) = view.read_with(cx, |v, _| {
        let file = titled(v, "model.py");
        let r = v.hits.iter().find(|h| h.id == file).unwrap().rect;
        (file, point(px((r.x + r.w / 2.0) as f32), px((r.y + 6.0) as f32)))
    });
    cx.simulate_click(at, Modifiers::none());
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected), Some(file));
    assert_eq!(content(&view, cx), Content::Block(file));
    let panel = cx.debug_bounds("inspector").expect("the panel is open");
    // A step is shown in its function, its own line marked; clicking that line selects the block drawn for it.
    let step = view.update(cx, |v, cx| {
        let forward = titled(v, "forward");
        let m = v.m.clone().unwrap();
        let step = m.block_at(forward, 6);
        assert_ne!(step, forward);
        v.select(Some(step), Some(m.canonical_path(step)), cx);
        step
    });
    frame(cx);
    let line = cx.debug_bounds("marked-line").expect("its line is marked");
    assert!(panel.contains(&line.origin), "{line:?} in {panel:?}");
    cx.simulate_click(center(line), Modifiers::none());
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected), Some(step));
    // Its close button closes it, and so does Esc.
    let close = cx.debug_bounds("panel-close").expect("a close button");
    cx.simulate_click(center(close), Modifiers::none());
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected), None);
    view.update(cx, |v, cx| v.select(Some(step), None, cx));
    frame(cx);
    cx.simulate_keystrokes("escape");
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected), None);
}

#[gpui::test]
fn breadcrumbs_follow_a_flight_and_back_returns_where_the_reader_was(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let start = view.read_with(cx, |v, _| (v.camera.path.clone(), v.camera.xf));
    view.update(cx, |v, cx| {
        let path = v.m.as_ref().unwrap().canonical_path(titled(v, "forward"));
        v.fly_to(&path, Framing::Open, 0.08, cx);
    });
    land(&view, cx);
    let crumbs = view.read_with(cx, |v, _| v.crumbs.iter().map(|&c| v.m.as_ref().unwrap().node(c).title.clone()).collect::<Vec<_>>());
    assert_eq!(crumbs.last().map(String::as_str), Some("forward"), "{crumbs:?}");
    assert!(view.read_with(cx, |v, _| v.history.can_back()));
    // Back (⌥←) flies to exactly where the view was.
    cx.simulate_keystrokes("alt-left");
    land(&view, cx);
    let back = view.read_with(cx, |v, _| (v.camera.path.clone(), v.camera.xf));
    assert_eq!(back.0, start.0);
    assert!((back.1.s - start.1.s).abs() < 1e-6 && (back.1.x - start.1.x).abs() < 1e-3);
    assert!(view.read_with(cx, |v, _| v.history.can_forward()));
    // A crumb flies back out to fit its block.
    view.update(cx, |v, cx| {
        let path = v.m.as_ref().unwrap().canonical_path(titled(v, "forward"));
        v.fly_to(&path, Framing::Open, 0.08, cx);
    });
    land(&view, cx);
    let root = cx.debug_bounds("crumb-root").expect("crumbs are shown");
    cx.simulate_click(center(root), Modifiers::none());
    frame(cx);
    land(&view, cx);
    let root_id = view.read_with(cx, |v, _| v.m.as_ref().unwrap().root());
    assert_eq!(view.read_with(cx, |v, _| v.camera.path.clone()), [root_id]);
}

#[gpui::test]
fn keys_zoom_pan_fit_and_go_home(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let s0 = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].s);
    cx.simulate_keystrokes("=");
    frame(cx);
    let s1 = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].s);
    assert!((s1 / s0 - 1.5).abs() < 1e-6, "{s0} → {s1}");
    let x = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].x);
    cx.simulate_keystrokes("left");
    frame(cx);
    let x1 = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].x);
    assert!((x1 - x - 90.0).abs() < 1e-6);
    cx.simulate_keystrokes("h");
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.flight.is_some()));
}

#[gpui::test]
fn search_finds_a_method_by_its_qualified_name_and_flies_there(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    cx.simulate_keystrokes("/");
    frame(cx);
    assert!(cx.debug_bounds("search").is_some());
    let typed = |v: &Viewer, cx: &gpui::App| match &v.dialog {
        Some(Dialog::Search(s)) => Some(s.input.read(cx).content.to_string()),
        _ => None,
    };
    // The key that opened the palette is not typed into it.
    assert_eq!(view.read_with(cx, |v, cx| typed(v, cx)), Some(String::new()));
    cx.simulate_input("Block.forward");
    frame(cx);
    let forward = view.read_with(cx, |v, _| titled(v, "forward"));
    let first = view.read_with(cx, |v, _| match &v.dialog {
        Some(Dialog::Search(s)) => s.results.first().copied(),
        _ => None,
    });
    assert_eq!(first, Some(forward));
    // Typing never runs the diagram's keys.
    assert!(view.read_with(cx, |v, _| v.flight.is_none()));
    cx.simulate_keystrokes("down up enter");
    frame(cx);
    view.read_with(cx, |v, _| {
        assert!(v.dialog.is_none());
        assert_eq!(v.selected, Some(forward));
        assert!(v.flight.is_some());
    });
    cx.simulate_keystrokes("cmd-k");
    frame(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.dialog, Some(Dialog::Search(_)))));
    cx.simulate_keystrokes("escape");
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.dialog.is_none()));
}

#[gpui::test]
fn following_a_value_lists_where_it_comes_from_and_goes(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    // Open Block.forward and follow `x`: the value panel names it.
    view.update(cx, |v, cx| {
        let path = v.m.as_ref().unwrap().canonical_path(titled(v, "forward"));
        v.fly_to(&path, Framing::Open, 0.08, cx);
    });
    land(&view, cx);
    let edge = view.read_with(cx, |v, _| v.m.as_ref().unwrap().node(titled(v, "forward")).edges.iter().position(|e| e.label == "x").map(|i| (titled(v, "forward"), i))).expect("an edge carrying x");
    view.update(cx, |v, cx| v.select_edge_for_test(edge, cx));
    frame(cx);
    match content(&view, cx) {
        Content::Value { owner, edges, .. } => {
            assert_eq!(owner, edge.0);
            assert!(edges.len() >= 2, "{edges:?}");
        }
        c => panic!("not a value: {c:?}"),
    }
    assert!(view.read_with(cx, |v, _| v.trace.is_some()));
    cx.simulate_keystrokes("escape");
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.value.is_none()));
}

#[gpui::test]
fn a_comparison_lists_the_changes_and_steps_through_them(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    // The base is the code before `mlp` was added, and before `attn` was renamed.
    let old = files(&[("model.py", &MODEL_PY.replace("        x = x + self.mlp(x)\n", "")), ("train.py", TRAIN_PY)]);
    view.update(cx, |v, cx| {
        let base = Rc::new(model(&old));
        let head = v.m.clone().unwrap();
        let changes = crate::git::changes(&old, &v.project.as_ref().unwrap().files);
        let diff = Rc::new(crate::diff::SceneDiff::new(base.clone(), head, changes));
        v.compare = Some(crate::viewer::Compared { rev: "HEAD".into(), kind: "Uncommitted changes".into(), label: "HEAD".into(), title: "Working tree vs HEAD".into(), base_files: old.clone(), base, diff, head_label: "working tree".into() });
        v.changes_open = true;
        v.sync_panel(cx);
        cx.notify();
    });
    frame(cx);
    assert_eq!(content(&view, cx), Content::Changes);
    let total = view.read_with(cx, |v, _| v.compare.as_ref().unwrap().diff.changes.len());
    assert!(total >= 1);
    // N marks the first change in the list, without leaving it or moving to it.
    cx.simulate_keystrokes("n");
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.marked), Some(0));
    assert_eq!(content(&view, cx), Content::Changes);
    // Its block stays pointed out: with the view moved far away, a marker on the edge leads back to it.
    assert!(view.read_with(cx, |v, _| v.marker.is_none()));
    view.update(cx, |v, cx| {
        v.pan_by(-40000.0, 0.0, cx);
    });
    frame(cx);
    let (area, _) = view.read_with(cx, |v, _| v.marker).expect("a marker for the block out of view");
    let before = view.read_with(cx, |v, _| v.camera.xf);
    cx.simulate_click(point(px((area.x + area.w / 2.0) as f32), px((area.y + area.h / 2.0) as f32)), Modifiers::none());
    land(&view, cx);
    let after = view.read_with(cx, |v, _| v.camera.xf);
    assert_ne!((before.x, before.y), (after.x, after.y));
    assert!((before.s - after.s).abs() < 1e-9);
    // Zooming to a change (a double click) reads it in the panel.
    view.update(cx, |v, cx| v.go_to_change(0, cx));
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.change_index()), Some(0));
    assert!(matches!(content(&view, cx), Content::Block(_) | Content::Removed(_)));
    // The list is limited to what the view is zoomed into, unless switched off.
    let all = view.read_with(cx, |v, _| v.visible_changes().len());
    assert_eq!(all, total);
    view.update(cx, |v, cx| v.set_scoped(false, cx));
    assert_eq!(view.read_with(cx, |v, _| v.visible_changes().len()), total);
    view.update(cx, |v, cx| v.set_scoped(true, cx));
    // Esc goes back to the list, and again closes it.
    view.update(cx, |v, cx| v.show_changes(cx));
    frame(cx);
    cx.simulate_keystrokes("escape");
    frame(cx);
    assert!(!view.read_with(cx, |v, _| v.changes_open));
}

#[gpui::test]
fn the_toolbar_opens_its_menus_and_switches_the_scheme(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let dark = view.read_with(cx, |v, _| v.dark());
    view.update(cx, |v, cx| v.toggle_scheme(cx));
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| (v.dark(), v.theme.dark)), (!dark, !dark));
    view.update(cx, |v, cx| v.toggle_scheme(cx));
    view.update(cx, |v, cx| {
        v.menu = Some(Menu::Download);
        cx.notify();
    });
    frame(cx);
    cx.simulate_keystrokes("escape");
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.menu.is_none()));
    // The legend lists what this diagram draws.
    view.update(cx, |v, cx| {
        v.legend = true;
        cx.notify();
    });
    frame(cx);
}

#[gpui::test]
fn compare_reads_the_folder_at_a_revision_and_shows_what_changed(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("blockviz-compare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null"]).args(args).current_dir(&dir).output().unwrap().status.success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("model.py"), MODEL_PY.replace("        x = x + self.mlp(x)\n", "")).unwrap();
    std::fs::write(dir.join("train.py"), TRAIN_PY).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    // The working tree adds a step.
    std::fs::write(dir.join("model.py"), MODEL_PY).unwrap();
    sandbox_home();
    cx.update(crate::input::bind_keys);
    let f = live::read(&dir);
    let m = model(&f);
    let (view, cx) = cx.add_window_view(|window, cx| Viewer::new(window, cx));
    view.update(cx, |v, cx| {
        v.show(dir.clone(), f, m, cx);
        let repo = crate::git::Repo::find(&dir).expect("a repository");
        v.project.as_mut().unwrap().repo = Some(repo);
        v.start_compare("HEAD".into(), "Uncommitted changes".into(), "HEAD".into(), "Working tree vs HEAD".into(), cx);
    });
    for _ in 0..50 {
        frame(cx);
        if view.read_with(cx, |v, _| v.compare.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (files, changes) = view.read_with(cx, |v, _| {
        let c = v.compare.as_ref().expect("comparing");
        (c.diff.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), c.diff.changes.len())
    });
    assert_eq!(files, ["model.py"]);
    assert!(changes >= 1);
    assert_eq!(content(&view, cx), Content::Changes);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[gpui::test]
fn the_menu_bar_drives_the_view(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    // Choosing a menu item dispatches its action to the window.
    cx.dispatch_action(crate::menus::Search);
    frame(cx);
    assert!(view.read_with(cx, |v, _| matches!(v.dialog, Some(Dialog::Search(_)))));
    cx.dispatch_action(crate::menus::Search);
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.dialog.is_none()));
    let s0 = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].s);
    cx.dispatch_action(crate::menus::ZoomIn);
    frame(cx);
    let s1 = view.read_with(cx, |v, _| v.camera.transforms(v.m.as_ref().unwrap())[0].s);
    assert!((s1 / s0 - 1.5).abs() < 1e-6);
    cx.dispatch_action(crate::menus::ToggleLegend);
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.legend));
    cx.simulate_keystrokes("cmd-0");
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.flight.is_some()));
    // The menus list the folders opened lately.
    let menus = crate::menus::menus(&["/a/one".into(), "/b/two".into()]);
    let names: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
    assert_eq!(names, ["blockviz", "File", "Edit", "View", "Compare", "Window"]);
}

#[gpui::test]
fn a_commit_about_to_be_made_is_shown_as_staged_against_head_whatever_is_on_disk(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("blockviz-staged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(["-c", "user.name=T", "-c", "user.email=t@e.c", "-c", "commit.gpgsign=false"]).args(args).current_dir(&dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("model.py"), MODEL_PY.replace("        x = x + self.mlp(x)\n", "")).unwrap();
    std::fs::write(dir.join("train.py"), TRAIN_PY).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    // Staged: the step added. On disk afterwards: train.py changed too, but not staged.
    std::fs::write(dir.join("model.py"), MODEL_PY).unwrap();
    git(&["add", "model.py"]);
    std::fs::write(dir.join("train.py"), "print(1)\n").unwrap();
    sandbox_home();
    cx.update(crate::input::bind_keys);
    let (view, cx) = cx.add_window_view(|window, cx| Viewer::new(window, cx));
    view.update(cx, |v, cx| v.open_comparison(dir.clone(), crate::request::Spec::Staged, Some("Add the mlp".into()), cx));
    for _ in 0..100 {
        frame(cx);
        if view.read_with(cx, |v, _| v.compare.is_some()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    view.read_with(cx, |v, _| {
        let c = v.compare.as_ref().expect("comparing");
        assert_eq!(c.diff.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>(), ["model.py"]);
        assert_eq!((c.title.as_str(), c.head_label.as_str()), ("Add the mlp", "staged"));
        assert!(c.diff.changes.len() >= 1);
        assert_eq!(v.snapshot.as_deref(), Some("staged"));
    });
    assert_eq!(content(&view, cx), Content::Changes);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[gpui::test]
fn picking_a_commit_shows_just_the_change_it_made_against_its_parent(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("blockviz-commit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(["-c", "user.name=T", "-c", "user.email=t@e.c", "-c", "commit.gpgsign=false"]).args(args).current_dir(&dir).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.py"), "def a():\n    return 1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "first"]);
    let first = git(&["rev-parse", "HEAD"]);
    std::fs::write(dir.join("a.py"), "def a():\n    return 2\n").unwrap();
    std::fs::write(dir.join("b.py"), "def b():\n    return 1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "second"]);
    let second = git(&["rev-parse", "HEAD"]);
    std::fs::write(dir.join("c.py"), "def c():\n    return 1\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "third"]);
    // Later work on disk, which the commit shown must not include.
    std::fs::write(dir.join("a.py"), "def a():\n    return 3\n").unwrap();
    sandbox_home();
    cx.update(crate::input::bind_keys);
    let (view, cx) = cx.add_window_view(|window, cx| Viewer::new(window, cx));
    let shown = |view: &Entity<Viewer>, cx: &mut VisualTestContext, sha: &str, title: &str| {
        view.update(cx, |v, cx| {
            v.compare = None;
            v.open_comparison(dir.clone(), crate::request::Spec::Commit(sha.into()), Some(title.into()), cx)
        });
        for _ in 0..100 {
            frame(cx);
            if view.read_with(cx, |v, _| v.compare.is_some()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        view.read_with(cx, |v, _| {
            let c = v.compare.as_ref().expect("comparing");
            let mut files: Vec<String> = c.diff.files.iter().map(|f| f.path.clone()).collect();
            files.sort();
            (files, c.title.clone(), v.snapshot.clone())
        })
    };
    let (files, title, snapshot) = shown(&view, cx, &second, "second");
    assert_eq!(files, ["a.py", "b.py"]);
    assert_eq!((title.as_str(), snapshot.as_deref()), ("second", Some(&second[..7])));
    // The first commit has no parent: all it has is new.
    let (files, _, snapshot) = shown(&view, cx, &first, "first");
    assert_eq!(files, ["a.py"]);
    assert_eq!(snapshot.as_deref(), Some(&first[..7]));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Opens the details panel and gives its left edge (where the resize handle is) and the pointer's steps.
fn panel_edge(cx: &mut TestAppContext) -> (Entity<Viewer>, &mut VisualTestContext, Point<Pixels>) {
    let (view, cx) = open(cx);
    view.update(cx, |v, cx| {
        v.panel_w = crate::viewer::PANEL_W;
        let id = titled(v, "forward");
        v.select(Some(id), Some(v.m.as_ref().unwrap().canonical_path(id)), cx);
    });
    frame(cx);
    let panel = cx.debug_bounds("inspector").expect("the panel is open");
    let edge = point(panel.origin.x, panel.origin.y + panel.size.height / 2.0);
    (view, cx, edge)
}

fn press(cx: &mut VisualTestContext, at: Point<Pixels>, count: usize) {
    cx.simulate_event(gpui::MouseDownEvent { position: at, modifiers: Modifiers::none(), button: gpui::MouseButton::Left, click_count: count, first_mouse: false });
    frame(cx);
}

fn release(cx: &mut VisualTestContext, at: Point<Pixels>, count: usize) {
    cx.simulate_event(gpui::MouseUpEvent { position: at, modifiers: Modifiers::none(), button: gpui::MouseButton::Left, click_count: count });
    frame(cx);
}

fn drag_to(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_move(at, gpui::MouseButton::Left, Modifiers::none());
    frame(cx);
}

#[gpui::test]
fn dragging_the_panels_edge_left_widens_it_and_letting_go_ends_the_drag(cx: &mut TestAppContext) {
    let (view, cx, edge) = panel_edge(cx);
    let w0 = view.read_with(cx, |v, _| v.panel_w);
    press(cx, edge, 1);
    assert!(view.read_with(cx, |v, _| v.resizing.is_some()));
    let to = point(edge.x - px(60.0), edge.y);
    drag_to(cx, to);
    assert_eq!(view.read_with(cx, |v, _| v.panel_w), w0 + 60.0);
    release(cx, to, 1);
    assert!(view.read_with(cx, |v, _| v.resizing.is_none()));
    assert_eq!(view.read_with(cx, |v, _| v.panel_w), w0 + 60.0);
}

#[gpui::test]
fn dragging_it_right_over_the_panel_narrows_it_and_letting_go_ends_the_drag(cx: &mut TestAppContext) {
    let (view, cx, edge) = panel_edge(cx);
    let w0 = view.read_with(cx, |v, _| v.panel_w);
    press(cx, edge, 1);
    let to = point(edge.x + px(50.0), edge.y);
    drag_to(cx, to);
    assert_eq!(view.read_with(cx, |v, _| v.panel_w), w0 - 50.0);
    release(cx, to, 1);
    assert!(view.read_with(cx, |v, _| v.resizing.is_none()));
}

#[gpui::test]
fn a_double_click_on_the_edge_resets_the_width(cx: &mut TestAppContext) {
    let (view, cx, edge) = panel_edge(cx);
    // Widened first, then the edge (now elsewhere) is double-clicked.
    press(cx, edge, 1);
    let to = point(edge.x - px(80.0), edge.y);
    drag_to(cx, to);
    release(cx, to, 1);
    assert_ne!(view.read_with(cx, |v, _| v.panel_w), crate::viewer::PANEL_W);
    press(cx, to, 1);
    release(cx, to, 1);
    press(cx, to, 2);
    release(cx, to, 2);
    assert_eq!(view.read_with(cx, |v, _| v.panel_w), crate::viewer::PANEL_W);
    assert!(view.read_with(cx, |v, _| v.resizing.is_none()));
}

#[gpui::test]
fn the_window_still_answers_after_a_resize_even_if_the_button_is_released_elsewhere(cx: &mut TestAppContext) {
    let (view, cx, edge) = panel_edge(cx);
    press(cx, edge, 1);
    drag_to(cx, point(edge.x + px(30.0), edge.y));
    // Released outside the window's content, or never seen: the next press anywhere ends the drag, and acts.
    let file = view.read_with(cx, |v, _| {
        let id = titled(v, "model.py");
        (id, v.hits.iter().find(|h| h.id == id).map(|h| point(px((h.rect.x + h.rect.w / 2.0) as f32), px((h.rect.y + 6.0) as f32))))
    });
    let (id, Some(at)) = file else { panic!("the file is drawn") };
    // That press ends the drag; the next acts.
    cx.simulate_click(at, Modifiers::none());
    frame(cx);
    assert!(view.read_with(cx, |v, _| v.resizing.is_none()));
    cx.simulate_click(at, Modifiers::none());
    frame(cx);
    assert_eq!(view.read_with(cx, |v, _| v.selected), Some(id));
}

/// A panel whose source runs far past the window, with the forward block selected.
fn long_panel(cx: &mut TestAppContext) -> (Entity<Viewer>, &mut VisualTestContext) {
    sandbox_home();
    cx.update(|cx| {
        crate::input::bind_keys(cx);
        crate::menus::bind(cx);
    });
    let long = format!("{MODEL_PY}{}", (0..400).map(|i| format!("# filler {i}\n")).collect::<String>());
    let f = files(&[("model.py", &long), ("train.py", TRAIN_PY)]);
    let m = model(&f);
    let (view, cx) = cx.add_window_view(|window, cx| Viewer::new(window, cx));
    view.update(cx, |v, cx| v.show(PathBuf::from("/tmp/t"), f, m, cx));
    frame(cx);
    view.update(cx, |v, cx| {
        v.panel_w = crate::viewer::PANEL_W;
        let id = titled(v, "model.py");
        v.select(Some(id), Some(v.m.as_ref().unwrap().canonical_path(id)), cx);
    });
    frame(cx);
    (view, cx)
}

#[gpui::test]
fn a_long_panel_has_a_scrollbar_whose_thumb_drags_the_text(cx: &mut TestAppContext) {
    let (view, cx) = long_panel(cx);
    let bar = view.read_with(cx, |v, cx| v.inspector.read(cx).bar());
    let body = cx.debug_bounds("inspector-body").expect("the body is drawn");
    let max: f32 = bar.handle.max_offset().height.into();
    assert!(max > 100.0, "the source is longer than the panel ({max})");
    assert_eq!(f32::from(bar.handle.offset().y), 0.0);
    // The thumb sits at the top of the strip along the right edge; take hold of it and pull down.
    let x = body.right() - px(5.0);
    let from = point(x, body.top() + px(8.0));
    let to = point(x, from.y + px(60.0));
    press(cx, from, 1);
    assert!(bar.dragging());
    drag_to(cx, to);
    let scrolled = -f32::from(bar.handle.offset().y);
    assert!(scrolled > 0.0, "dragging the thumb scrolled the text");
    // Pulled over the other way, it follows back.
    drag_to(cx, from);
    assert!(-f32::from(bar.handle.offset().y) < scrolled);
    release(cx, to, 1);
    assert!(!bar.dragging());
    // A click lower on the track jumps there, centering the thumb.
    press(cx, point(x, body.bottom() - px(4.0)), 1);
    release(cx, point(x, body.bottom() - px(4.0)), 1);
    assert!(-f32::from(bar.handle.offset().y) > max * 0.9);
}

#[gpui::test]
fn a_long_commit_subject_is_cut_short_inside_the_compare_menu(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    view.update(cx, |v, cx| {
        v.set_text_size(1.6, cx);
        let subject = "School: a Magister client that rotates the refresh token and keeps it encrypted at rest".to_string();
        v.log = vec![crate::git::Commit { sha: "bad409e0".into(), short: "bad409e".into(), subject, author: "A".into(), when: 0 }];
        v.menu = Some(Menu::Compare);
        cx.notify();
    });
    frame(cx);
    let menu = cx.debug_bounds("compare-menu").expect("the menu is open");
    let subject = cx.debug_bounds("commit-subject").expect("the commit is listed");
    eprintln!("ROW {:?}", cx.debug_bounds("commit-row"));
    assert!(subject.right() <= menu.right(), "{menu:?} {subject:?} the subject ends at {:?}, past the menu's edge at {:?}", subject.right(), menu.right());
}

#[gpui::test]
fn selecting_a_folder_opens_no_panel(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    let f = files(&[("pkg/model.py", MODEL_PY), ("pkg/util.py", "def f():\n    return 1\n"), ("train.py", &TRAIN_PY.replace("from model", "from pkg.model"))]);
    let m = model(&f);
    view.update(cx, |v, cx| v.show(PathBuf::from("/tmp/t"), f, m, cx));
    frame(cx);
    let dir = view.read_with(cx, |v, _| titled(v, "pkg/"));
    view.update(cx, |v, cx| {
        let path = v.m.as_ref().unwrap().canonical_path(dir);
        v.select(Some(dir), Some(path), cx);
    });
    frame(cx);
    assert!(!view.read_with(cx, |v, _| v.panel_open()));
    assert_eq!(content(&view, cx), Content::None);
}
