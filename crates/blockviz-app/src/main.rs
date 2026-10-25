//! blockviz as a native app: a folder on disk analyzed in process and drawn
//! with GPUI, the same diagram and reading tools as the web app, following
//! the folder live as files change, and comparing it with any git revision.
//!
//! `blockviz-app [folder]` opens a folder (or asks for one).
//! `blockviz-app folder --svg out.svg|out.png [--at file/Class/method] [--dark] [--size 1440x900]`
//! draws a view into an SVG (or PNG) without a window, as the window would draw it.

mod anchor;
mod camera;
mod code;
mod compare;
mod crumbs;
mod diff;
mod export;
mod git;
mod gpuipen;
mod history;
mod hook;
mod icons;
mod input;
mod linediff;
mod lines;
mod live;
mod menus;
mod model;
mod panel;
mod pen;
mod render;
mod request;
mod scope;
mod settings;
mod shell;
mod svgpen;
#[cfg(test)]
mod tests;
mod theme;
mod trace;
mod viewer;

use gpui::{px, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};
use request::Request;
use std::path::PathBuf;
use viewer::Viewer;

/// `hook`: shows a commit the coding agent is about to make. `hook claude` reads Claude Code's PreToolUse
/// input from stdin and answers it; `hook --command "<shell command>" [--cwd <dir>]` takes the command
/// directly, for agents whose hooks pass it another way, and prints nothing.
fn hook(args: &[String]) {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let claude = args.first().is_some_and(|a| a == "claude");
    let (command, cwd) = match value("--command") {
        Some(c) => (c, value("--cwd").map(PathBuf::from).or_else(|| std::env::current_dir().ok()).unwrap_or_default()),
        None => {
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            match hook::claude_command(&input) {
                Some((c, d)) if claude => (c, PathBuf::from(d)),
                _ => return,
            }
        }
    };
    let Some(launch) = hook::launch_for(&command, &cwd) else { return };
    let Some(repo) = git::Repo::find(&launch.folder) else { return };
    // The window outlives this hook, and the agent does not wait for it.
    let mut app = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("blockviz-app")));
    app.arg(repo.root()).args(["--compare", launch.compare]);
    if let Some(t) = &launch.title {
        app.args(["--title", t]);
    }
    app.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut app, 0);
    if app.spawn().is_ok() && claude {
        println!("{}", hook::CLAUDE_ASK);
    }
}

/// Draws a view of `folder` into an SVG, without a window.
fn draw_svg(folder: &std::path::Path, out: &str, at: Option<&str>, dark: bool, (w, h): (f64, f64)) {
    let files = live::read(folder);
    let name = folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let scene = blockviz_core::analyze(&name, live::inputs(&files));
    let m = model::Model::new(scene, live::texts(&files));
    let mut cam = camera::Camera::new(&m);
    cam.insets = camera::Insets { top: 56.0, right: 12.0, bottom: 52.0, left: 12.0 };
    cam.fit_root(&m, w, h);
    if let Some(at) = at {
        // A path of titles from the top: `model.py/Block/forward`; a title's
        // start will do (`for block`), and `#2` is the third child, for untitled ones.
        let mut id = m.root();
        for part in at.split('/') {
            let o = m.owner(m.node(id)).map_or(id, |o| o.id);
            let kids = &m.node(o).children;
            let found = match part.strip_prefix('#').and_then(|i| i.parse::<usize>().ok()) {
                Some(i) => kids.get(i).copied(),
                None => kids.iter().copied().find(|&c| m.node(c).title == part).or_else(|| kids.iter().copied().find(|&c| m.node(c).title.starts_with(part))),
            };
            match found {
                Some(c) => id = c,
                None => {
                    let titles: Vec<String> = kids.iter().enumerate().map(|(i, &c)| format!("#{i} {:?} {:?}", m.node(c).kind, m.node(c).title)).collect();
                    return eprintln!("no {part} in {}:\n{}", m.node(o).title, titles.join("\n"));
                }
            }
        }
        let plan = cam.flight(&m, &m.canonical_path(id), w, h, camera::Framing::Open, 0.08);
        cam.step(&m, &plan, 1.0, w, h);
    }
    let theme = if dark { theme::Theme::dark() } else { theme::Theme::light() };
    let xfs = cam.transforms(&m);
    let p = render::Paint { width: w, height: h, hover: None, selected: None, path: &cam.path, xfs: &xfs, related: None, trace: None, hover_edge: None, diff: None };
    let mut pen = svgpen::SvgPen::estimating();
    render::Renderer::default().draw(&mut pen, &m, &theme, &p);
    let svg = pen.svg(w, h);
    // A PNG is the SVG rasterized, as the Download menu makes one.
    if out.ends_with(".png") {
        std::fs::write(out, export::png(&svg, 1.0).expect("rasterized")).unwrap();
    } else {
        std::fs::write(out, svg).unwrap();
    }
    println!("wrote {out}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let svg = value("--svg");
    let at = value("--at");
    let sized = value("--size");
    let compare = value("--compare");
    let title = value("--title");
    if args.first().is_some_and(|a| a == "hook") {
        return hook(&args[1..]);
    }
    let flags_with_values = [svg.as_ref(), at.as_ref(), sized.as_ref(), compare.as_ref(), title.as_ref()];
    let folder = args.iter().find(|a| !a.starts_with("--") && !flags_with_values.contains(&Some(*a))).map(PathBuf::from);

    if let Some(out) = svg {
        let folder = folder.unwrap_or_else(|| PathBuf::from("."));
        let (w, h) = sized.and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))).unwrap_or((1440.0, 900.0));
        return draw_svg(&folder, &out, at.as_deref(), args.iter().any(|a| a == "--dark"), (w, h));
    }

    // Opening a repository while the app is running shows it there, and raises that window.
    let request = folder.map(|f| Request { repo: f.canonicalize().unwrap_or(f).to_string_lossy().to_string(), compare, title });
    #[cfg(unix)]
    let incoming = {
        let socket = request::socket_path();
        if request.as_ref().is_some_and(|r| request::send(&socket, r)) {
            return;
        }
        request::serve(&socket).ok()
    };

    Application::new().with_assets(icons::Icons).run(move |cx: &mut App| {
        input::bind_keys(cx);
        menus::bind(cx);
        cx.on_action(|_: &menus::Quit, cx| cx.quit());
        cx.on_action(|_: &menus::Hide, cx| cx.hide());
        cx.on_action(|_: &menus::HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &menus::ShowAll, cx| cx.unhide_other_apps());
        cx.set_menus(menus::menus(&settings::Settings::load().recent));
        // The app is its window: closing it quits.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        let window = cx
            .open_window(options, |window, cx| {
                cx.new(|cx| {
                    let mut v = Viewer::new(window, cx);
                    if let Some(r) = request {
                        v.open_request(r, cx);
                    }
                    v
                })
            })
            .unwrap();
        // Requests from other launches arrive on a thread of their own, and are handled here.
        #[cfg(unix)]
        if let Some(incoming) = incoming {
            let (tx, mut rx) = futures::channel::mpsc::unbounded::<Request>();
            std::thread::spawn(move || {
                for r in incoming {
                    if tx.unbounded_send(r).is_err() {
                        break;
                    }
                }
            });
            cx.spawn(async move |cx| {
                use futures::StreamExt;
                while let Some(r) = rx.next().await {
                    let _ = window.update(cx, |v, window, cx| {
                        v.open_request(r, cx);
                        window.activate_window();
                    });
                    let _ = cx.update(|cx| cx.activate(true));
                }
            })
            .detach();
        }
        cx.activate(true);
    });
}
