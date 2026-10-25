//! Real Ruby repositories (trimmed snapshots in fixtures/corpus-rb, see its
//! README) read the way their developers would describe them: a Rails app
//! layered models → jobs and helpers → controllers without a single
//! `require`, models as tables, concerns and mixins, an engine's namespaced
//! classes, and a gem's files and its executable.

use blockviz_core::ir::{Kind, Node, Scene};
use blockviz_core::project::InFile;
use std::path::Path;

fn load(dir: &Path, root: &Path, out: &mut Vec<InFile>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if p.is_dir() {
            load(&p, root, out);
        } else {
            let bytes = e.metadata().unwrap().len();
            out.push(InFile { path: rel, text: std::fs::read_to_string(&p).ok(), bytes });
        }
    }
}

fn repo(name: &str) -> Scene {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus-rb").join(name);
    let mut files = vec![];
    load(&root, &root, &mut files);
    let s = blockviz_core::analyze(name, files);
    assert!(!s.diagnostics.iter().any(|d| d.contains("Syntax error") || d.contains("Skipped")), "{:?}", s.diagnostics);
    s
}

fn file<'a>(s: &'a Scene, path: &str) -> &'a Node {
    let i = s.files.iter().position(|f| f.path == path).unwrap_or_else(|| panic!("no file {path}"));
    s.nodes.iter().find(|n| matches!(n.kind, Kind::File) && n.span.as_ref().is_some_and(|sp| sp.file as usize == i)).unwrap_or_else(|| panic!("no node for {path}"))
}

fn descendants<'a>(s: &'a Scene, n: &'a Node) -> Vec<&'a Node> {
    let mut out = vec![];
    for &c in &n.children {
        let c = &s.nodes[c as usize];
        out.push(c);
        out.extend(descendants(s, c));
    }
    out
}

fn within<'a>(s: &'a Scene, n: &'a Node, title: &str) -> &'a Node {
    descendants(s, n).into_iter().find(|c| c.title == title).unwrap_or_else(|| panic!("{} has no {title:?}", n.title))
}

/// A definition in a file (not a call of it).
fn def<'a>(s: &'a Scene, n: &'a Node, title: &str) -> &'a Node {
    descendants(s, n).into_iter().find(|c| c.title == title && matches!(c.kind, Kind::Function | Kind::Class)).unwrap_or_else(|| panic!("{} defines no {title:?}", n.title))
}

/// Edges inside a directory (or the root), as (from, to, label).
fn edges_in(s: &Scene, n: &Node) -> Vec<(String, String, String)> {
    n.edges.iter().map(|e| (s.nodes[e.from as usize].title.clone(), s.nodes[e.to as usize].title.clone(), e.label.clone())).collect()
}

fn dir<'a>(s: &'a Scene, title: &str) -> &'a Node {
    s.nodes.iter().find(|n| n.kind == Kind::Dir && n.title == title).unwrap_or_else(|| panic!("no directory {title}"))
}

#[test]
fn lobsters_is_a_rails_app() {
    let s = repo("lobsters");
    // Files say what they are by where Rails keeps them.
    for (path, role) in [
        ("app/controllers/stories_controller.rb", "controller"),
        ("app/controllers/concerns/authenticatable.rb", "concern"),
        ("app/models/story.rb", "model"),
        ("app/models/concerns/token.rb", "concern"),
        ("app/jobs/notify_comment_job.rb", "job"),
        ("app/helpers/application_helper.rb", "helpers"),
        ("config/routes.rb", "routes"),
        ("config/application.rb", "config"),
    ] {
        assert_eq!(file(&s, path).subtitle, role, "{path}");
    }
    // Without a `require`, the app is layered by the constants it names:
    // models under jobs and controllers, helpers and jobs under controllers.
    let app = edges_in(&s, dir(&s, "app/"));
    let has = |from: &str, to: &str| app.iter().any(|(f, t, _)| f == from && t == to);
    assert!(has("models/", "controllers/") || has("models/", "jobs/") && has("jobs/", "controllers/"), "{app:?}");
    assert!(has("helpers/", "controllers/") && has("jobs/", "controllers/"), "{app:?}");
    assert!(!has("controllers/", "models/"), "{app:?}");
    // Models are tables; the abstract base and plain classes are not.
    let models = |name: &str| def(&s, file(&s, &format!("app/models/{}.rb", name.to_lowercase())), name);
    for m in ["Story", "Comment", "User", "Tag", "Tagging", "Vote", "Hat", "Category", "Moderation"] {
        assert!(models(m).badges.contains(&"DB table".to_string()), "{m}: {:?}", models(m).badges);
    }
    let record = def(&s, file(&s, "app/models/application_record.rb"), "ApplicationRecord");
    assert!(record.badges.contains(&"abstract".to_string()) && !record.badges.contains(&"DB table".to_string()));
    assert!(!def(&s, file(&s, "app/models/short_id.rb"), "ShortId").badges.contains(&"DB table".to_string()));
    // Concerns say so; classes list the project modules they mix in.
    assert_eq!(def(&s, file(&s, "app/controllers/concerns/story_finder.rb"), "StoryFinder").subtitle, "concern");
    assert_eq!(def(&s, file(&s, "app/models/concerns/token.rb"), "Token").subtitle, "concern");
    let stories = def(&s, file(&s, "app/controllers/stories_controller.rb"), "StoriesController");
    assert!(stories.subtitle.contains("StoryFinder"), "{}", stories.subtitle);
    assert!(models("Story").subtitle.contains("Token"));
    let app_controller = def(&s, file(&s, "app/controllers/application_controller.rb"), "ApplicationController");
    assert!(app_controller.subtitle.starts_with("Authenticatable, ApplicationHelper"), "{}", app_controller.subtitle);
    // `Mod::StoriesController` is drawn by its own name inside `mod/`, its base by its full one.
    let mod_stories = def(&s, file(&s, "app/controllers/mod/stories_controller.rb"), "StoriesController");
    assert!(mod_stories.subtitle.contains("Mod::ModController"), "{}", mod_stories.subtitle);
}

#[test]
fn lobsters_calls_reach_concerns_and_models() {
    let s = repo("lobsters");
    // `find_story` in an action is the concern's method, mixed in.
    let stories = file(&s, "app/controllers/stories_controller.rb");
    let finder = def(&s, file(&s, "app/controllers/concerns/story_finder.rb"), "find_story");
    let upvote = def(&s, stories, "upvote");
    assert_eq!(within(&s, upvote, "find_story").reference, Some(finder.id));
}

#[test]
fn mission_control_is_an_engine_of_namespaced_classes() {
    let s = repo("mission_control-jobs");
    for (path, role) in [
        ("app/controllers/mission_control/jobs/jobs_controller.rb", "controller"),
        ("app/controllers/concerns/mission_control/jobs/job_scoped.rb", "concern"),
        ("app/models/mission_control/jobs/worker.rb", "model"),
        ("app/helpers/mission_control/jobs/jobs_helper.rb", "helpers"),
        ("config/routes.rb", "routes"),
    ] {
        assert_eq!(file(&s, path).subtitle, role, "{path}");
    }
    // `module MissionControl; module Jobs` wraps every file without being drawn.
    let jobs = file(&s, "app/controllers/mission_control/jobs/jobs_controller.rb");
    let top: Vec<&str> = jobs.children.iter().map(|&c| s.nodes[c as usize].title.as_str()).filter(|t| !t.is_empty()).collect();
    assert_eq!(top.first(), Some(&"JobsController"), "{top:?}");
    // Mixins are found by their full names from inside the namespace.
    let controller = def(&s, jobs, "JobsController");
    assert!(controller.subtitle.contains("MissionControl::Jobs::JobScoped"), "{}", controller.subtitle);
    // `include ActiveModel::Model` makes a form model, not a table.
    let worker = def(&s, file(&s, "app/models/mission_control/jobs/worker.rb"), "Worker");
    assert!(worker.badges.contains(&"form model".to_string()) && !worker.badges.contains(&"DB table".to_string()), "{:?}", worker.badges);
    let record = def(&s, file(&s, "app/models/mission_control/jobs/application_record.rb"), "ApplicationRecord");
    assert!(record.badges.contains(&"abstract".to_string()), "{:?}", record.badges);
}

#[test]
fn thor_is_a_gem_and_its_executable() {
    let s = repo("thor");
    // A shebang makes `bin/thor` Ruby, and the program it is.
    let bin = file(&s, "bin/thor");
    assert_eq!(bin.subtitle, "executable");
    assert!(bin.badges.contains(&"entry point".to_string()));
    // `class Thor` opened only to nest definitions is never a block.
    let basic = file(&s, "lib/thor/shell/basic.rb");
    let top: Vec<&str> = basic.children.iter().map(|&c| s.nodes[c as usize].title.as_str()).collect();
    assert_eq!(top, ["Basic"]);
    // Reopened to add a constant (`Correctable`), it is drawn and leads to `Thor`.
    let errors = file(&s, "lib/thor/error.rb");
    let thor = def(&s, file(&s, "lib/thor.rb"), "Thor");
    assert!(errors.children.iter().map(|&c| &s.nodes[c as usize]).filter(|n| n.title == "Thor").all(|n| n.link == Some(thor.id)));
    assert_eq!(def(&s, errors, "UndefinedCommandError").subtitle, "Thor::Error");
    // Classes in other files that use a base depend on it.
    let create_link = def(&s, file(&s, "lib/thor/actions/create_link.rb"), "CreateLink");
    assert_eq!(create_link.subtitle, "Thor::Actions::CreateFile");
    // The standard library is not a package.
    let packages = s.nodes.iter().find(|n| n.title == "External packages");
    for std in ["fileutils", "open-uri", "open3", "rbconfig", "pathname", "digest"] {
        assert!(!packages.is_some_and(|p| p.rows.iter().any(|r| r.key == std)), "{std}");
    }
    // Calls through `Thor::Util`'s class methods reach them.
    let runner = file(&s, "lib/thor/runner.rb");
    let util = file(&s, "lib/thor/util.rb");
    let find = def(&s, util, "find_class_and_command_by_namespace");
    assert!(find.badges.contains(&"class method".to_string()));
    assert_eq!(within(&s, def(&s, runner, "help"), "find_class_and_command_by_namespace").reference, Some(find.id));
}

/// The route a table in `routes.rb` lists for a key, as `(key, value)`.
fn route_row<'a>(s: &'a Scene, routes: &'a Node, key: &str) -> Option<(&'a str, &'a str, &'a str)> {
    descendants(s, routes).into_iter().filter(|n| n.kind == Kind::Table).find_map(|t| t.rows.iter().find(|r| r.key == key).map(|r| (t.title.as_str(), r.key.as_str(), r.value.as_str())))
}

#[test]
fn lobsters_routes_name_their_actions() {
    let s = repo("lobsters");
    let routes = file(&s, "config/routes.rb");
    // Route tables as `bin/rails routes` prints them, one per block of the file.
    assert_eq!(route_row(&s, routes, "GET /s/:id/(:title)").map(|r| r.2), Some("stories#show"));
    assert_eq!(route_row(&s, routes, "PATCH /mod/stories/:story_id/undelete"), Some(("namespace :mod", "PATCH /mod/stories/:story_id/undelete", "mod/stories#undelete")));
    assert_eq!(route_row(&s, routes, "/jobs").map(|r| r.2), Some("MissionControl::Jobs::Engine"));
    assert_eq!(route_row(&s, routes, "GET /upvoted").map(|r| r.2), Some("→ /upvoted/stories"));
    assert!(descendants(&s, routes).iter().all(|n| n.kind == Kind::Table), "a route file is its tables");
    // Actions are named by their routes: REST, nested, member and inferred.
    let action = |path: &str, class: &str, name: &str| def(&s, def(&s, file(&s, path), class), name).subtitle.clone();
    let stories = "app/controllers/stories_controller.rb";
    assert_eq!(action(stories, "StoriesController", "show"), "GET /stories/:id · GET /s/:id/(:title)");
    assert_eq!(action(stories, "StoriesController", "upvote"), "POST /stories/:story_id/upvote");
    assert_eq!(action(stories, "StoriesController", "update"), "PATCH, PUT /stories/:id");
    assert_eq!(action(stories, "StoriesController", "destroy"), "DELETE /stories/:id · PATCH /stories/:story_id/destroy");
    assert_eq!(action(stories, "StoriesController", "fetch_url_attributes"), "POST /stories/fetch_url_attributes");
    assert_eq!(action("app/controllers/comments_controller.rb", "CommentsController", "upvote"), "POST /comments/:id/upvote");
    assert_eq!(action("app/controllers/login_controller.rb", "LoginController", "index"), "GET /login");
    assert_eq!(action("app/controllers/users_controller.rb", "UsersController", "show"), "GET /~:username");
    assert!(action("app/controllers/users_controller.rb", "UsersController", "tree").contains("GET /moderators"));
    assert_eq!(action("app/controllers/mod/stories_controller.rb", "StoriesController", "edit"), "GET /mod/stories/:id/edit");
    assert_eq!(action("app/controllers/mod/tags_controller.rb", "TagsController", "new"), "GET /mod/tags/new");
}

#[test]
fn mission_control_routes_are_the_engines() {
    let s = repo("mission_control-jobs");
    let action = |path: &str, class: &str, name: &str| def(&s, def(&s, file(&s, &format!("app/controllers/mission_control/jobs/{path}")), class), name).subtitle.clone();
    // An engine's routes reach its namespaced controllers; `root` too.
    assert_eq!(action("queues_controller.rb", "QueuesController", "index"), "GET /applications/:application_id/queues · GET /queues · GET /");
    // `scope module: :queues` and a singular `resource :pause`.
    assert_eq!(action("queues/pauses_controller.rb", "PausesController", "create"), "POST /applications/:application_id/queues/:queue_id/pause");
    assert_eq!(action("queues/pauses_controller.rb", "PausesController", "destroy"), "DELETE /applications/:application_id/queues/:queue_id/pause");
    // A singular resource is served by the plural controller; `collection do` drops the id.
    assert_eq!(action("retries_controller.rb", "RetriesController", "create"), "POST /applications/:application_id/jobs/:job_id/retry");
    assert_eq!(action("bulk_retries_controller.rb", "BulkRetriesController", "create"), "POST /applications/:application_id/jobs/bulk_retries");
    // `path: ":status/jobs"`.
    assert_eq!(action("jobs_controller.rb", "JobsController", "index"), "GET /applications/:application_id/:status/jobs · GET /:status/jobs");
    assert_eq!(action("recurring_tasks_controller.rb", "RecurringTasksController", "update"), "PATCH, PUT /applications/:application_id/recurring_tasks/:id");
}

fn table<'a>(s: &'a Scene, n: &'a Node, title: &str) -> &'a Node {
    n.children.iter().map(|&c| &s.nodes[c as usize]).find(|c| c.kind == Kind::Table && c.title == title).unwrap_or_else(|| panic!("{} has no table {title:?}", n.title))
}

#[test]
fn lobsters_declarations_are_tables_and_roles() {
    let s = repo("lobsters");
    let story = def(&s, file(&s, "app/models/story.rb"), "Story");
    // Associations, validations and callbacks are tables of the model, never a block per line.
    let associations = table(&s, story, "associations");
    let row = |t: &Node, k: &str| t.rows.iter().find(|r| r.key == k).map(|r| (r.value.clone(), r.note.clone()));
    assert_eq!(row(associations, "user").map(|r| r.0), Some("belongs_to User".to_string()));
    assert_eq!(row(associations, "comments").map(|r| r.0), Some("has_many Comment".to_string()));
    let tags = row(associations, "tags").expect("tags");
    assert_eq!(tags.0, "has_many Tag");
    assert!(tags.1.contains("through: taggings"), "{tags:?}");
    let comment = def(&s, file(&s, "app/models/comment.rb"), "Comment");
    assert!(row(table(&s, comment, "associations"), "notifications").unwrap().1.contains("polymorphic"));
    assert!(row(table(&s, story, "validations"), "title").unwrap().0.contains("presence"));
    assert!(!descendants(&s, story).iter().any(|n| matches!(n.title.as_str(), "validates" | "has_many" | "belongs_to")));
    // Callbacks say when they run; the callbacks table is in the order Rails runs them.
    let method = |c: &Node, name: &str| def(&s, c, name).subtitle.clone();
    assert_eq!(method(story, "assign_initial_attributes"), "before validation on create");
    assert_eq!(method(comment, "record_initial_upvote"), "after commit on create");
    let callbacks: Vec<&str> = table(&s, story, "callbacks").rows.iter().map(|r| r.value.as_str()).collect();
    let at = |m: &str| callbacks.iter().position(|c| *c == m).unwrap_or_else(|| panic!("{m} not in {callbacks:?}"));
    assert!(at("assign_initial_attributes") < at("log_moderation"), "{callbacks:?}");
    // Scopes are class methods of the model.
    let hottest = def(&s, story, "hottest");
    assert!(hottest.badges.contains(&"scope".to_string()) && hottest.badges.contains(&"class method".to_string()), "{:?}", hottest.badges);
    // Filters: when each runs, from the controller's declarations or a concern's `included do`.
    let stories = def(&s, file(&s, "app/controllers/stories_controller.rb"), "StoriesController");
    assert_eq!(method(stories, "find_user_story"), "before destroy, edit, undelete, update");
    assert!(method(stories, "track_story_reads").starts_with("around show"));
    let auth = def(&s, file(&s, "app/controllers/concerns/authenticatable.rb"), "Authenticatable");
    assert_eq!(method(auth, "authenticate_user"), "before every action");
    let login = def(&s, file(&s, "app/controllers/login_controller.rb"), "LoginController");
    assert!(table(&s, login, "filters").rows.iter().any(|r| r.key == "skip clear_session_cookie"));
}
