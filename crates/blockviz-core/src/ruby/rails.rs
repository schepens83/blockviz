//! What Rails conventions make of a Ruby project: the role of each file by
//! where it lives (`app/controllers/` holds controllers, `config/routes.rb`
//! the routes), which classes are models, and the views an action renders.

/// The role of a file by its place in a Rails app or engine (whose `app/`
/// may be below the repository root), a gem's `bin/` or a Rake file.
pub fn file_role(path: &str) -> Option<&'static str> {
    let parts: Vec<&str> = path.split('/').collect();
    let base = *parts.last()?;
    if base == "Rakefile" || base.ends_with(".rake") {
        return Some("tasks");
    }
    if parts.len() >= 2 && parts[parts.len() - 2] == "bin" && !base.contains('.') {
        return Some("executable");
    }
    if let Some(i) = parts.iter().position(|p| *p == "app") {
        let inside = &parts[i + 1..parts.len() - 1];
        if inside.contains(&"concerns") {
            return Some("concern");
        }
        return Some(match *inside.first()? {
            "controllers" => "controller",
            "models" => "model",
            "jobs" => "job",
            "mailers" => "mailer",
            "helpers" => "helpers",
            "channels" => "channel",
            "mailboxes" => "mailbox",
            "services" => "service",
            "workers" => "worker",
            "policies" => "policy",
            "serializers" => "serializer",
            "validators" => "validator",
            "components" => "component",
            _ => return None,
        });
    }
    if let Some(i) = parts.iter().position(|p| *p == "config") {
        return Some(if parts.len() == i + 2 && base == "routes.rb" { "routes" } else { "config" });
    }
    if base == "seeds.rb" && parts.len() >= 2 && parts[parts.len() - 2] == "db" {
        return Some("script");
    }
    None
}

/// A class that descends from `ActiveRecord::Base`, through the project's
/// own classes (`ApplicationRecord`).
pub fn is_model(consts: &super::Consts, name: &str) -> bool {
    consts.ancestors(name).iter().any(|a| consts.defs[*a].superclass.as_deref().is_some_and(|s| matches!(s.trim_start_matches("::"), "ActiveRecord::Base")))
}
