# Ruby corpus fixtures

Trimmed snapshots of real repositories of different shapes, used by
`tests/corpus_ruby.rs` to check that diagrams read the way a developer would
describe each codebase. Lockfiles, assets, JavaScript and tests are left out;
each keeps its license file.

| Fixture | Source | Commit | License |
|---|---|---|---|
| lobsters | github.com/lobsters/lobsters (a Rails 8 app: `config/routes.rb`, `config/application.rb`, ten controllers with their concerns and three of the `mod/` namespace, fourteen models, two jobs, two helpers, the views of stories, comments, users, tags, login and the layout) | 4075364 | BSD-3-Clause |
| mission_control-jobs | github.com/rails/mission_control-jobs (a Rails engine: `app/` controllers, concerns, models and helpers, `lib/`, `config/routes.rb`, the index and show views) | ee06c67 | MIT |
| thor | github.com/rails/thor (a gem without Rails: `lib/`, `bin/thor`, the gemspec) | cb11e8e | MIT |
