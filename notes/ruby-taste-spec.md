# Ruby and Rails in blockviz: taste spec

What a Ruby or Rails developer expects to see at each level, turned into rules
and checkable expectations for `tests/corpus_ruby.rs`. Every fixture claim below
was checked against `crates/blockviz-core/tests/fixtures/corpus-rb/`.

Vocabulary is the existing one: *subtitle* (role), *badge/tag*, *marker*
(effects and filters, shown on the block and in the inspector), *table*,
*exit* (a way out of a function, tagged with its condition), *reference* (a
step leads to the definition it calls).

---

## 1. Principles

**Rails developers already have names for every box, and the diagram should use them.**
They say "`StoriesController#show` handles `GET /s/:id`", "`Story has_many :comments`",
"`before_action :require_logged_in_user`", "that's a concern", "it enqueues
`NotifyCommentJob`". They don't say "a call to `has_many` with a symbol argument".
`bin/rails routes` prints `Prefix | Verb | URI Pattern | Controller#Action`, so that
is the vocabulary for routes ([Rails Routing guide](https://guides.rubyonrails.org/routing.html)).
rails-erd and railroady show what they expect a model diagram to be: models as
boxes and associations as the arrows between them, found by reflection and not
by reading call sites ([rails-erd](https://voormedia.github.io/rails-erd/),
[railroady](https://github.com/preston/railroady)).

**Convention is code.** In Rails, much of the wiring is never written as a call:
- an action with no `render` renders `app/views/<controller>/<action>.html.erb`
  ([Layouts and Rendering](https://guides.rubyonrails.org/layouts_and_rendering.html));
- `Story` lives in `app/models/story.rb` (Zeitwerk), so there are no `require`s;
- `resource :retry` routes to the *plural* `RetriesController`.

If blockviz only draws explicit calls, a Rails app looks disconnected. The
conventions must be applied as if they were written out.

**Declarations are not steps.** The class body of a model or controller is a list
of declarations: associations, validations, callbacks, filters, scopes. Readers
treat these as configuration of the class, the same way the README treats
decorators and settings. They become tables, subtitles and markers. They never
become a column of call boxes.

**Per level:**

| Level | What a Rails dev expects |
|---|---|
| Repository | The MVC folders layered: `models/` above `jobs/`, `helpers/` and `controllers/`; `views/` tied to controllers by `template`; `config/routes.rb` pointing into `controllers/`; `lib/` as the app's own library; `Gemfile`/gemspec as the package list; engines mounted in the routes as subsystems. This is roughly the "container/component" view of [C4](https://c4model.com/) and of [GitLab's architecture page](https://docs.gitlab.com/development/architecture/) (Puma serves the controllers, Sidekiq or Solid Queue runs the jobs, PostgreSQL holds the models). |
| File | One class per file (Rails convention). The file's role is its subtitle (`controller`, `model`, `concern`, `job`, `helpers`, `routes`, `view`, `commands`, `tasks`). Inside: the class, its declaration tables, then its methods. |
| Class | **Controller:** the filter chain, the actions (each subtitled with its routes), then private helpers. **Model:** associations, validations and callbacks in lifecycle order, scopes, then methods. **Plain class:** attributes and methods. |
| Function | The request path through an action: `params` and filter-set ivars come in, then queries (database), enqueued jobs (in background), mail and network calls, and **a response as the exit** (`render`, `redirect_to`, `head`, the implicit template). Ivars set in an action (`@story`) are the values passed to the view. |

Mastodon's own code map names the same building blocks: controllers, models,
services ("complex operations that span multiple models"), workers, serializers,
policies, lib ([Mastodon code structure](https://docs.joinmastodon.org/dev/code/)).
37signals argues for rich models plus concerns over mandatory service layers
([Vanilla Rails is plenty](https://dev.37signals.com/vanilla-rails-is-plenty/)).
So blockviz must not assume a service layer exists. It recognizes
`app/services/*`, `app/workers/*` and `app/policies/*` by folder (subtitle
`service`, `worker`, `policy`) when they do exist.

---

## 2. Ruby idioms → blocks (any Ruby)

| Construct | Decision |
|---|---|
| Calls without parentheses (`redirect_to Routes.title_path @story`, `say "…"`, `def link_post button_label, link`) | Identical to the parenthesized form. A bare identifier is a **method call** unless it was assigned earlier in the method (Ruby's own rule), so `update_story_attributes` is a call into project code, not a variable read. |
| `each`, `each_with_index`, `each_with_object`, `times` | An inline loop box, with the steps inside tagged `each` as in JS. `xs.map { |x| f(x) }` / `select` / `reject` is a comprehension (`each of xs`). `map(&:name)` is glue folded into its step: `@queue_names = ActiveJob.queues.map(&:name)` is titled `queue_names`. |
| `tap`, `then`, `yield_self` | Pass the value through like `.then` in JS, so there's no block for the chain. `Application.new(name:).tap { |a| a.add_servers(…) }` is `Application.new → add_servers`. |
| Builder blocks (`Model.create do |m| m.x = … end`, `Gem::Specification.new do |spec|`) | The block's field stores belong to the call: `create` captioned `Moderation`, holding `m · set × 3`. A gemspec block is a config table (§3). |
| `attr_accessor` / `attr_reader` / `attr_writer` | Never blocks or calls. They are the class's **attributes**: one table per class (`attributes`, rows are names). Inside methods, `self.x = …` is a field store and a bare `x` that names an attribute reads the field. For a class whose job is to carry data (`include ActiveModel::Model`, `Struct`, `Data`), the attribute table is the class's main content, like a DTO. |
| Implicit return | The last expression is the result, as in Rust. `if`/`case` as last expression: each arm's last value reaches the result. **Exception:** controller actions, filters, AR callbacks, `initialize` and job `perform` ignore their return value, so they have **no result arrow**. Their output is the response, the ivars or the side effects. |
| Modifier `if`/`unless`, `unless` blocks | A guard tag, keeping the developer's word: `redirect_to "/" if @user` is `redirect_to "/" · if @user`, and `raise … unless token.nil?` is `raise ArgumentError · unless token.nil?`. Never rewrite `unless x` to `if !x`. `return x if c` is a guarded exit. `x or raise E` / `x or return` is a guard on `x`: `raise ResourceNotFound · unless @application`. |
| `rescue` / `ensure` / `retry` | `begin … rescue E` (or method-level `rescue`) is a branch arm titled `on E`. A rescue whose body is empty or only a comment tags the guarded step `errors ignored`. `ensure` runs `on exit` like Go's `defer`; an ensure that only restores a saved value or closes something is left out (inspector only), e.g. `I18n.config = @previous_config`. `retry` is a loop-back edge labeled `retry`. |
| `raise` | An exit tagged `raises <Class>`, titled by the message when there is one (`raise Error, "Error opening URI '#{name}'"` is the exit `Error opening URI` · `raises Thor::Error`). Bare `raise` / `raise e` inside a rescue is `re-raises`. `raise ActiveRecord::Rollback` inside `transaction` is the exit `rollback`. |
| `yield`, `block.call`, `&block` | `yield` is a step titled `block` (the caller's code runs here), with arrows for the values yielded. In an `around_action` it is titled `action`. Passing `&block` along is glue. |
| `include` / `extend` / `prepend` | Not calls. A class's **subtitle lists the project modules it mixes in**, in written order (`StoriesController — StoryFinder`), the way Rust lists traits. Library mixins (`Enumerable`, `ActiveModel::Model`) are badges only when they change what the class is: `ActiveModel::Model` gives `form model`, `Enumerable` gives `collection`. Mixin methods are found from calls: `find_story` in `StoriesController` is `find_story — StoryFinder`. `prepend` is drawn the same way with the subtitle `prepended to X`. `self.included(base); base.extend ClassMethods` is recognized like a concern: `ClassMethods`' methods become class methods of the includer. |
| `module_function`, `extend self` | The module's methods are module functions (tag `module function`), callable as `Mod.f`. |
| `class << self`, `def self.x` | Never a box. The methods are class methods of the enclosing class, titled by bare name and tagged `class method`. Breadcrumb and inspector write `Thor.help` for a class method and `Thor#help` for an instance method, as RDoc does. Operator methods are titled by the operator (`Story./`). |
| `private` / `protected` (section, `private def`, `private :x`, the "indented private" style) | Methods after it go in a lower `private` band of the class, tagged. Not a block. |
| Namespace wrappers (`class Thor; module Actions; class CreateFile`, `module MissionControl; module Jobs`) | A class or module that only nests definitions is **not a box**. The file shows `CreateFile` directly, qualified as `Thor::Actions::CreateFile` in the inspector and breadcrumb. A reopened class or module is drawn where it is written and leads to its other parts, like a Rust `impl` in another module. Calls find methods across every reopening. |
| `Struct.new(:a, :b)` / `Data.define` (assigned or as a superclass) | A table of fields: `class Command < Struct.new(:name, …)` is the class `Command` with a `fields` table, plus its methods. |
| `require` / `require_relative` / `autoload` / `load` | Import edges between files. `require "thor/runner"` resolves through the gemspec's `require_paths` (`lib`). `autoload :Actions, File.expand_path("actions", __dir__)` is an edge to `actions.rb`. Gems are packages. **Stdlib** (`rbconfig`, `pathname`, `digest/sha2`, `fileutils`, `open-uri`, `open3`, `irb`, `erb`, `set`) is not listed, like Node builtins. A `require` inside a method is an optional import (inspector only). **In a Rails app or a Zeitwerk gem**, dependencies come from **constant references resolved by path** (`Story` is `app/models/story.rb`, `Mod::StoriesController` is `app/controllers/mod/stories_controller.rb`, `MissionControl::Jobs::JobScoped` is `app/controllers/concerns/mission_control/jobs/job_scoped.rb`, because `concerns/` is a root). |
| `method_missing`, `respond_to_missing?` | Drawn as methods tagged `catch-all`. Calls that resolve to no method do **not** jump to `method_missing`. |
| `define_method` / `send` / `public_send` / `class_eval` / `module_eval` strings / `instance_eval` | Static limits apply. `define_method` over a literal list or a constant array (`STATUSES.each { |s| define_method s }`) expands to the generated methods, shown as one block titled by their names (`pending, failed, in_progress, …`), badge `generated`, captioned `for each of STATUSES`, and calls to them resolve. `send(:sym)` / `__send__(:sym)` with a literal is a plain call. With a computed name, it is a step titled `public_send`, captioned with the expression, tagged `dynamic`. String `class_eval`/`module_eval` is not parsed; when its names come from a literal list (`SHELL_DELEGATED_METHODS`), it becomes a table `delegated to shell` of those names. `instance_eval(contents, path)` of loaded files is an I/O step, `eval file`. |
| `&.` | Glue. `@user&.is_moderator?` reads as `is_moderator?` with no tag, because nil-tolerance is not a branch a reader cares about. |
| `||=` | `@x ||= expr`: the step is titled `x` with badge `memoized`. A method whose whole body is `@x ||= expr` reads as the value (like Go's field getters). A local `x ||= y` is `y · if x is nil`, merged with any modifier (`story ||= Story.find(…) if @user&.is_admin?`). |
| String interpolation, heredocs, `format`/`sprintf` | Formatted text that reads the variables it names, folded into the step it feeds (`increment_value_for` captioned `"user:#{…}:stories_deleted"`). Never a block of its own. |
| `if __FILE__ == $0` / `$PROGRAM_NAME` | The entry-point marker plus a `Main` section, as for Python. A mere mention of `$PROGRAM_NAME` (`File.basename($PROGRAM_NAME)` in `base.rb`) is **not** a main guard. |
| Executables | A file with a `#!/usr/bin/env ruby` shebang and no extension (`bin/thor`) is Ruby. Files named in gemspec `executables` or in `bin/` get badge `entry point` and subtitle `executable`. |
| Rake tasks (`Rakefile`, `*.rake`, or any `.rb` whose top level is `namespace`/`desc`/`task`) | File subtitle `tasks`. Each `task` is a function titled by its **full name** (`mission_control:jobs:authentication:configure`), subtitle `rake task`, doc from `desc`. `namespace` is never a box. Prerequisites that are project tasks become edges, and `=> :environment` is the badge `loads app`. |
| Thor commands (`desc "usage", "text"` then `def`) | The method is a command: subtitle is the usage string (`install NAME`), doc from the description. `method_option(s)` give an `options` table (`--as · string`). `map "-T" => :list` puts the alias badge `-T` on `list`. The file and class subtitle is `commands`. Methods with no `desc`, under `private`, or in `no_commands` are plain methods. |
| `puts`/`warn`/`p`/`pp`, `super` with no added value, `freeze`, `to_s`/`to_sym`/`presence` | Not blocks. Prints are a `prints` marker. A `super` whose value is used (`super.tap { … }`, `super + …`) is a step titled `super` leading to the parent method. |
| Constants | Runs of constants are a constants table (`TITLE_MIN_LENGTH = 3`). A regex constant is a row, not a block. `mattr_accessor :x, default: v` × N is a **settings** table (`internal_query_count_limit = 500_000`). |
| Error classes (`class LoginBannedError < StandardError; end`) | Tagged `error type`. One-line error classes in a row gather into one table, `errors` (rows are name → base). |

---

## 3. Rails DSLs

Hard-code these, because without them a Rails app reads as a pile of method
calls. Everything not listed stays a plain call.

### Models (`< ApplicationRecord` / `ActiveRecord::Base`, transitively)

| DSL | Drawn as |
|---|---|
| The model class | Badge `DB table` (the Django/SQLModel precedent: a model is a table, not a row struct). File subtitle `model`. `self.abstract_class = true` gives `abstract` instead of `DB table`. A class in `app/models/` that doesn't descend from AR (`ShortId`, `ActiveModel::Model` classes, `CurrentAttributes`) is **not** a table. |
| `belongs_to` / `has_many` / `has_one` / `has_and_belongs_to_many` | **Both**: an `associations` table in the class (row key is the association name, value is `has_many Comment` / `belongs_to User`, `class_name:` resolved; options like `through:`, `dependent:`, `polymorphic`/`as:` go in the caption), **and association edges** between model blocks wherever both are drawn (`app/models/` and above). Edges run **parent → child**, i.e. from the referenced table to the one holding the foreign key, as rails-erd lays them out (User above Story above Comment). There is one edge per pair, labeled by the parent's association name (`comments`); `belongs_to` alone gives the edge labeled by the child's name. `through:` associations and self-references (`merged_into_story`) are rows only, never edges. Associations to models not in the repo are rows without links. |
| `validates`, `validates_*_of`, `validate :m`, `validate do`, `validates_each` | One `validations` table: row key is the attribute, value is the merged rules (`title` → `presence · length 3..150`). Constants in ranges are resolved when they are literals in the file. `validate :m` gives method `m` the subtitle `validation`. `validate do … end` / `validates_each` become a function block titled `validate` with subtitle `validation`. **Never one block per line.** |
| Callbacks (`before_validation`, `before/after/around_save/create/update/destroy`, `after_commit`, `after_initialize`, `after_*_commit`) | The callback method's subtitle says when it runs: `before save`, `after create`, `after commit on create`, `before validation on create`, with `if:`/`unless:` appended (`· if :admin?`). The class also gets a `callbacks` table **in lifecycle order** (validation → save → create → after save → commit, per the [Callbacks guide](https://guides.rubyonrails.org/active_record_callbacks.html)), not in source order. A callback given as a block or lambda is a function titled `after create`. A step that saves a record (`@story.save`) gets the inspector note "runs callbacks: …". |
| `scope :name, -> { … }` | A function titled `name`, badge `scope`, tagged `class method`. Its lambda parameters are its parameters, and its chain is a database query (below). Scopes used in a chain resolve to their definitions. |
| `enum status: {…}` | A table of values, title `status`, subtitle `enum`. The generated predicates (`draft?`) and scopes resolve to it. |
| `delegate :a, :b, to: :x` | Not a block. A `delegated` table (rows `a → x`). Calls to `a` resolve through to `x.a` where `x` is typed. |
| `has_secure_password`, `has_one_attached`, `serialize`, `store`, `normalizes`, `encrypts` | Rows in the attributes table with a caption (`password_digest · has_secure_password`). |
| ActiveRecord query chains (`Model.where(…).order(…).limit(…)`, record `save`/`update`/`destroy`/`touch`, `find_by`, `pluck`, `count`, `first_or_initialize`, `exists?`, `delete_all`, `load_async`) | **One database step per chain**, named by the call that runs it (the gorm precedent): `first` captioned `Moderation · where × 2 · order`, effect `database`. A chain assigned without a terminal call is titled by the variable (`@comments`), still `database`, because it runs when the view reads it. A chain counts as a database step **only when its receiver is a project AR model (or a scope or association of one), or a library class known to be AR** (`SolidQueue::*`). `ActiveJob.jobs.where(…)` in mission_control is project code (`ActiveJob::JobsRelation#where`), **not** a database. `Model.transaction do … end` is an inline box `transaction`. |

### Controllers (`< ActionController::Base` / `ApplicationController`, transitively)

| DSL | Drawn as |
|---|---|
| Actions (public instance methods) | Subtitle = their route(s) from `routes.rb`, as `VERB /path` with `(.:format)` dropped and optional segments kept as written (`GET /s/:id/(:title)`). Verbs on the same path merge (`PATCH, PUT /stories/:id`), and different paths join with ` · ` in routes.rb order. An action with no route has an empty subtitle (it is not an error). |
| `before_action` / `after_action` / `around_action` / `prepend_*` / `skip_*` (`only:`/`except:`/`if:`) | The **controller's `filters` table**, in chain order (inherited first, `prepend_` at the front, `skip_` applied): row key is `before require_logged_in_user`, value is the actions or `every action`. **Markers on actions**: only filters scoped with `only:`/`except:` (or skipped for some actions) appear as markers on the actions they apply to (`before: find_user_story`). Filters that apply to every action are listed once in the table, never repeated on 20 actions. A filter method's subtitle says when it runs when it has a single registration (`before destroy, edit, undelete, update`; `around show · if @user.present?`; `before every action`), otherwise it is `before action`. Block filters (`after_action only: […] do … end`) are functions titled by what they call. Filter return values are not results; a filter that renders or redirects is an exit tagged `halts`. |
| `rescue_from E do … end` / `with: :m` | A handler titled `on E` (several classes join with ` or `), containing its response exits. |
| `render`, `redirect_to`, `redirect_back(_or_to)`, `head`, `send_data`, implicit render | **Exits, not library calls.** They are titled by what the client gets: `render stories/new`, `render json`, `render plain`, `redirect_to "/"`, `redirect_to Routes.title_path`, `head 401`, with `status:` in the caption (`status 400`). `render action: "x"` / `render :x` / `render "x"` link to the template, and an action that falls off its end without rendering ends in an **implicit `render <controller>/<action>`** exit linked to that view file. `return render …` / `render … and return` exit at once. A `redirect_to` without `return` that more code follows is a step marked `responds`, because it doesn't halt ([Layouts and Rendering](https://guides.rubyonrails.org/layouts_and_rendering.html)). `flash[:x] = "…"` right before a response becomes that exit's caption (`flash: You cannot edit that story.`), not its own block. |
| `respond_to do |format| … end` | A branch (like `match`) whose arms are the formats (`html`, `json`, `rss`, `any`). `format.html { … }` is never a call box. |
| `params`, `params[:id]`, `params.dig(…)` | An **input** of the action (arrow labeled `params[:id]`), never a box. |
| Strong params (`params.require(:story).permit(…)`, `params.expect(…)`) | One step titled `permit`, captioned with the permitted keys. A method whose job is this (`story_params`, `tag_params`) gets subtitle `strong params` and a table of permitted fields. |
| Instance variables set in actions and filters | Arrows labeled `@story` into the response exit (the view's inputs). A `@story` set by a `before_action` is an input of the actions it applies to. |
| `session`, `cookies`, `flash`, `request.*` | Request state, **not** a database (the Flask precedent). `Rails.cache.*` is effect `cache`. |
| `helper_method :x` | Tag `view helper` on `x`. |
| `layout "x"` | A `layout` edge from the layout view to the controller. Without it, the convention applies (`layouts/application`). |
| `caches_page`, `protect_from_forgery`, `content_security_policy`, `http_basic_authenticate_with` | Badges on the controller or actions (`page cached` on `show`), else inspector only. |
| Route helpers (`story_unhide_path(…)`, `application_jobs_url(…)`) | Glue folded into their step, captioned with the path pattern they build when the route is known (`/stories/:story_id/unhide`). |

### Concerns, jobs, mailers, helpers, views

| DSL | Drawn as |
|---|---|
| `extend ActiveSupport::Concern` | The module gets subtitle `concern` and file subtitle `concern`. `included do … end` is **not a box**: its declarations (filters, validations, callbacks, `helper_method`, `delegate`, `rescue_from`) apply to every includer and are drawn as above, attributed `— Authenticatable`. `class_methods do` gives class methods. A concern including a concern is followed ([ActiveSupport::Concern](https://api.rubyonrails.org/classes/ActiveSupport/Concern.html)). `extend ActiveSupport::Concern` is not an import edge. |
| Jobs (`< ApplicationJob` / `ActiveJob::Base`) | File subtitle `job`. `perform`'s subtitle is `job` plus the queue when it's set (`job · default queue`). `X.perform_later(a)` / `X.set(wait: …).perform_later` is a step titled `X`, marked `in background` (plus `after 5 minutes`) and leading to `X#perform`, with arrows labeled by its arguments. `perform_now` is a plain call. `retry_on`/`discard_on`/`rescue_from` are handlers as in controllers. |
| Mailers | `X.mention(…).deliver_now/later` is a step `mention` captioned `EmailReplyMailer`, effect `email` (`deliver_later` is `in background`). Mailer classes get file subtitle `mailer`. |
| Helpers (`app/helpers/*_helper.rb`) | File subtitle `helpers`, module subtitle `view helpers`. All helpers are available to all views by default, so calls from views and from controllers that `include` them resolve. |
| Views (`*.html.erb`, `*.rss.builder`, `*.json.jbuilder`) | File subtitle `view`, `partial` (leading `_`) or `layout` (under `layouts/`). The inside is a short list, not ERB dataflow: the partials and templates it renders (as references, resolved relative to its folder, with `_` added), the helpers it calls, and the ivars it reads as inputs. Controllers link to views by a `template` edge (the Flask precedent). |
| `config/routes.rb` (`resources`, `resource`, `namespace`, `scope`, `member`/`collection`, `get "x" => "c#a"`, `match … via:`, `root`, `mount`, `redirect(…)`, `constraints`) | File subtitle `routes`. **Route tables**: one per top-level `resources`/`resource`/`namespace`/`scope`/`constraints` block (titled `resources :stories`, `namespace :mod`), and one per blank-line-separated run of loose routes, titled by its shared controller when all rows share one (`login`). Rows are `VERB /full/path` → `controller#action` (namespaced as `mod/stories#undelete`), each row linking to its action. Rules to implement, from the [Routing guide](https://guides.rubyonrails.org/routing.html): `resources` gives the seven REST actions minus `only`/`except`. Routes inside `resources` **without** `member`/`collection` nest under `/:story_id/`, while `member` uses `/:id/` and `collection` has no id. A singular `resource :retry` maps to the plural `RetriesController`. `namespace :mod` prefixes both path and controller module; `scope module:` prefixes only the module; `path:` overrides the segment. `post "/stories/fetch_url_attributes"` with no target infers `stories#fetch_url_attributes`. `to: redirect(…)` rows have value `→ /upvoted/stories`. `mount X, at: "/jobs"` is a row `/jobs` → `X`. An engine's routes (`Engine.routes.draw`) carry no mount prefix. |
| `config/application.rb`, `config/environments/*`, `config/initializers/*` | Subtitle `config`, with `config.x = …` as a settings table. The `Rails::Application` subclass is badged `app`. A `Rails::Engine` subclass gets file subtitle `engine`, and each `initializer "name" do` is a function titled `name` with subtitle `initializer`. `config.before_initialize`/`after_initialize`/`console`/`rake_tasks` blocks are functions titled `before initialize` etc. |
| `Gemfile`, `*.gemspec` | Package lists (subtitle `dependencies`). A gemspec gives `executables` (entry points), `require_paths` (import roots) and `add_dependency` packages, with development dependencies in the inspector only. |

---

## 4. Fixture expectations

Paths are relative to the fixture root. `subtitle == x` means exact. "Route
subtitles contain" means the joined subtitle includes each listed route.

### 4.1 lobsters (Rails 8 app)

**Files and repository**
1. [must] `app/controllers/stories_controller.rb` subtitle `controller`; `app/controllers/concerns/authenticatable.rb` subtitle `concern`; `app/models/story.rb` subtitle `model`; `app/jobs/notify_comment_job.rb` subtitle `job`; `app/helpers/application_helper.rb` subtitle `helpers`; `config/routes.rb` subtitle `routes`; `app/views/stories/_form.html.erb` subtitle `partial`; `app/views/layouts/application.html.erb` subtitle `layout`.
2. [must] Inside `app/`, there is a dependency path from `models/` to `controllers/` and none from `controllers/` back to `models/`. `jobs/` and `helpers/` sit above `controllers/`: `NotifyCommentJob` is used by `CommentsController#create`, and `ApplicationController` includes `ApplicationHelper`. No model depends on a controller or a job.
3. [must] Dependencies come from constant references with no `require`: `stories_controller.rb` depends on `concerns/story_finder.rb` (`include StoryFinder`) and on `models/story.rb`; `mod/stories_controller.rb` depends on `mod/mod_controller.rb` (superclass `Mod::ModController`).
4. [must] `config/routes.rb` is connected to `app/controllers/`. `Gemfile` lists the packages `rails`, `solid_queue`, `mission_control-jobs`, `sqlite3`.

**Routes**
5. [must] The routes file has a route table containing the row `GET /s/:id/(:title)` → `stories#show`, and a `namespace :mod` table containing `PATCH /mod/stories/:story_id/undelete` → `mod/stories#undelete`, plus a row `/jobs` → `MissionControl::Jobs::Engine` (mount).
6. [must] `StoriesController#show` route subtitle contains `GET /stories/:id` and `GET /s/:id/(:title)`.
7. [must] Nested non-member routes use the parent's id: `StoriesController#upvote` subtitle `POST /stories/:story_id/upvote`. Member routes use `:id`: `CommentsController#upvote` subtitle `POST /comments/:id/upvote`.
8. [must] `StoriesController#destroy` subtitle contains `DELETE /stories/:id` and `PATCH /stories/:story_id/destroy`. `StoriesController#update` subtitle contains `PATCH, PUT /stories/:id`. `StoriesController#fetch_url_attributes` subtitle `POST /stories/fetch_url_attributes` (target inferred from the path).
9. [must] `LoginController#index` subtitle `GET /login`, `LoginController#login` subtitle `POST /login`, `UsersController#show` subtitle `GET /~:username`, `UsersController#tree` subtitle contains `GET /users` and `GET /moderators`, `Mod::StoriesController#edit` subtitle `GET /mod/stories/:id/edit`, `Mod::TagsController#new` subtitle `GET /mod/tags/new`.
10. [nice] Rows routing to controllers that are not in the fixture (`home#index`, `settings#index`) are present and unlinked, with no diagnostic. Redirect rows have values starting `→` (`GET /upvoted` → `→ /upvoted/stories`).

**Controllers**
11. [must] `StoriesController` subtitle `StoryFinder`. `ApplicationController` subtitle `Authenticatable, ApplicationHelper`. `IntervalHelper` is not in the fixture, so it is left out of the subtitle and kept in the inspector, the way Go lists only the project's interfaces. `Mod::StoriesController` subtitle `StoryFinder`, with base `Mod::ModController`.
12. [must] Filter subtitles: `StoriesController#find_user_story` subtitle `before destroy, edit, undelete, update`; `StoriesController#track_story_reads` subtitle starts with `around show`; `Authenticatable#authenticate_user` subtitle `before every action`.
13. [must] Filter markers: `StoriesController#destroy` carries `require_logged_in_user` and `find_user_story`; `StoriesController#show` carries neither. No lobsters action carries `authenticate_user` or `set_traffic_style` as a marker, because those run before every action and are listed once in the filter table.
14. [must] `Mod::StoriesController`'s filter table lists `require_logged_in_moderator` (inherited from `Mod::ModController`) before `find_story!`. `LoginController`'s filter table shows `clear_session_cookie` as skipped.
15. [must] The call `find_story` inside `StoriesController#upvote` references `StoryFinder#find_story` (mixin resolution). The call `filtered_tags` inside `TagsController#index` references `ApplicationHelper#filtered_tags`.
16. [must] `StoriesController#edit` ends in an implicit render exit that references `app/views/stories/edit.html.erb`. `LoginController#index` ends in one for `app/views/login/index.html.erb`. `StoriesController#create`'s `render action: "new"` references `app/views/stories/new.html.erb`. `Mod::TagsController#create`'s `render :new` references `app/views/mod/tags/new.html.erb`.
17. [must] `StoriesController#upvote` has an exit `render plain` with caption `status 400`, tagged `if !(@story = find_story) || @story.is_gone?` (or the elided form), and a final exit `render plain` (the `"ok"`). Neither `render` nor `params` appears as a non-exit block.
18. [must] `StoriesController#hide` has a `respond_to` branch with arms `html` and `json`. `TagsController#index` has a `respond_to` with arms `html` and `json`.
19. [must] `CommentsController#create` contains a step `NotifyCommentJob` marked `in background` that references `NotifyCommentJob#perform`, with an arrow labeled `@comment`. `StoriesController#create` has a `transaction` box containing an exit `rollback`.
20. [must] `StoriesController#story_params` subtitle `strong params`, with permitted fields `title, url, description, user_is_author, user_is_following, tags`. `Mod::TagsController#tag_params` lists `quorum`.
21. [must] `Authenticatable#authenticate_user` has effect `database` (`User.find_by`). Its `session[:u]` read is not a database step. `Authenticatable#require_logged_in_user` has no result arrow for its `true`.
22. [nice] `ApplicationController` has handlers titled `on ActionController::ParameterMissing` and `on ActiveRecord::ConnectionNotEstablished`, each containing a `render plain` exit.

**Models**
23. [must] `Story`, `Comment`, `User`, `Tag`, `Tagging`, `Vote`, `Hat`, `HiddenStory`, `SavedStory`, `Category` and `Moderation` are badged `DB table`. `ApplicationRecord` is `abstract`, not `DB table`. `ShortId` and `ShortId::CandidateId` are **not** `DB table`.
24. [must] Within `app/models/`: association edges `Story → Comment` (label `comments`), `User → Story` (label `stories`), `Tag → Tagging` and `Story → Tagging` (label `taggings`), and `User → Hat` (Hat's `granted_by_user` resolves to `User` via `class_name:`). There is no edge for `Story has_many :tags, through: :taggings` and no self-edge for `merged_into_story`.
25. [must] `Story`'s associations table has the rows `comments` (`has_many Comment`), `user` (`belongs_to User`) and `tags` (`has_many Tag`, caption containing `through: :taggings`). `Comment`'s `notifications` row is captioned `polymorphic` (`as: :notifiable`).
26. [must] `Story` has **one** `validations` table (no block per `validates` line), with row `title` containing `presence`. `Story#validate` (the `validate do` block) is a function with subtitle `validation`.
27. [must] Callback subtitles: `Story#log_moderation` `before save`; `Story#assign_initial_attributes` `before validation on create`; `Story#update_story_text` `after save`; `Comment#record_initial_upvote` `after commit on create`; `Hat#log_moderation` `after create`. `Story`'s callbacks table lists `assign_initial_attributes` before `log_moderation` before `mark_submitter` before `bust_comment_redirect_cache` (lifecycle order).
28. [must] `Story` has scope functions `hottest`, `not_deleted` and `for_presentation` (badge `scope`). `hottest`'s body references `base` and `not_hidden_by`, and is a single database step, not one block per chained call.
29. [must] `Token` (`app/models/concerns/token.rb`) has subtitle `concern`. `Story`, `Comment` and `Hat` subtitles contain `Token`. `Token`'s `included do` block is not a box, and its `after_initialize` block appears as a function with subtitle `after initialize`.
30. [nice] `HiddenStory.hide_story_for_user` is tagged `class method`, and the call in `StoriesController#flag` references it. `Story./` is titled `/` and tagged `class method`.
31. [nice] `Category#log_modifications` shows `create` (captioned `Moderation`, effect `database`) holding the field stores `action`, `moderator_user_id`, `category_id`, not three separate blocks.

**Jobs and views**
32. [must] `NotifyCommentJob#perform` subtitle starts with `job` and shows `comments.each` → `deliver_comment_notifications`. `NotifyCommentJob#notify_once` has a branch `on ActiveRecord::RecordNotUnique` ending in the result `false`.
33. [nice] `deliver_mention_notifications` has effect `email` (`EmailReplyMailer.mention(…).deliver_now`), and the mailer step is tagged `errors ignored` (the empty `rescue => e`).
34. [must] There is a `template` edge from `app/views/stories/` to `stories_controller.rb` (drawn where the two meet). `app/views/stories/edit.html.erb` references `app/views/stories/_form.html.erb`, and `app/views/mod/tags/edit.html.erb` references `app/views/mod/tags/_form.html.erb` (relative partial).
35. [nice] `config/application.rb` subtitle `config`, with a settings table containing `time_zone`. `Lobsters::Application` is badged `app`.

### 4.2 mission_control-jobs (Rails engine)

1. [must] `lib/mission_control/jobs/engine.rb` subtitle `engine`. Its `initializer "mission_control-jobs.middleware"` is a function with that title and subtitle `initializer`.
2. [must] `config/routes.rb` subtitle `routes` (engine routes, no mount prefix). Controllers resolve under `isolate_namespace MissionControl::Jobs`: the row `GET /queues` → `queues#index` links to `MissionControl::Jobs::QueuesController#index`.
3. [must] `MissionControl::Jobs::QueuesController#index` subtitle contains `GET /applications/:application_id/queues`, `GET /queues` and `GET /` (from `root`).
4. [must] `scope module: :queues` plus singular `resource :pause`: `MissionControl::Jobs::Queues::PausesController#create` subtitle `POST /applications/:application_id/queues/:queue_id/pause`, and `#destroy` subtitle `DELETE /applications/:application_id/queues/:queue_id/pause`.
5. [must] A singular resource maps to the plural controller: `RetriesController#create` subtitle `POST /applications/:application_id/jobs/:job_id/retry`. `collection do resource :bulk_retries` gives `BulkRetriesController#create` subtitle `POST /applications/:application_id/jobs/bulk_retries`.
6. [must] `path: ":status/jobs"`: `JobsController#index` subtitle contains `GET /applications/:application_id/:status/jobs` and `GET /:status/jobs`. `RecurringTasksController#update` subtitle contains `PATCH, PUT /applications/:application_id/recurring_tasks/:id`.
7. [must] `MissionControl::Jobs::ApplicationController` subtitle lists `HostRouteHelpers, BasicAuthentication, ApplicationScoped, NotFoundRedirections, AdapterFeatures, JobFilters` (the two-module `include` counts both). `JobsController`, `RetriesController`, `DiscardsController` and `DispatchesController` subtitles contain `JobScoped`.
8. [must] Every file under `app/controllers/concerns/mission_control/jobs/` has subtitle `concern`. `JobScoped#set_job` and `ApplicationScoped#set_application` are subtitled `before every action`. `ApplicationScoped#activating_job_server` is subtitled `around every action`.
9. [must] `skip_before_action :set_job, only: :index`: `JobsController#show` carries the marker `set_job` and `JobsController#index` doesn't.
10. [must] The call `jobs_relation` inside `JobScoped#set_job` leads to the overriding methods in the includers (several, so it is named like Go's interface calls: `jobs_relation — JobsController or RetriesController or …`), not just the `raise NotImplementedError` stub.
11. [must] `JobsController#show` (empty body) ends in an implicit render exit referencing `app/views/mission_control/jobs/jobs/show.html.erb`. The `layout "mission_control/jobs/application"` links `app/views/layouts/mission_control/jobs/application.html.erb` to `MissionControl::Jobs::ApplicationController`.
12. [must] `Batch`, `Worker` and `RecurringTask` (`include ActiveModel::Model`) are **not** `DB table`. Each is badged `form model` with an attributes table (`Worker` rows include `hostname` and `last_heartbeat_at`). `MissionControl::Jobs::ApplicationRecord` is `abstract`.
13. [must] `ActiveJob.jobs.where(**@job_filters)` in `JobsController#filtered_jobs` is not a database step: its `where` references `ActiveJob::JobsRelation#where` in `lib/active_job/jobs_relation.rb`. Code in `lib/active_job/` is project code even though it shares the framework's namespace.
14. [must] `ActiveJob::JobsRelation` has the generated methods from `STATUSES.each { define_method … }` as one block whose title contains `pending` and `failed`, badge `generated`. `RetriesController#jobs_relation`'s `ActiveJob.jobs.failed` resolves its `failed` to it. `ActiveJob::JobsRelation`'s subtitle or badges show `Enumerable` as `collection`.
15. [must] `lib/mission_control/jobs.rb` shows a settings table from the `mattr_accessor`s, with rows `internal_query_count_limit` = `500_000` and `http_basic_auth_enabled` = `true`. `mattr_accessor` never becomes a block.
16. [must] `QueueScoped#set_queue` (`@queue = … or raise …`) has an exit tagged `raises MissionControl::Jobs::Errors::ResourceNotFound`, titled from the message `Queue '…' not found`.
17. [must] `lib/mission_control/jobs/tasks.rb` subtitle `tasks`, holding a function titled `mission_control:jobs:authentication:configure` with subtitle `rake task` and badge `loads app`. Its body references `MissionControl::Jobs::Authentication.configure`.
18. [nice] `ActiveJob::QueueAdapters::SolidQueueExt` subtitle contains `prepended to SolidQueueAdapter` (from the engine's `before_initialize`). Its `queues` method has effect `database` (`SolidQueue::Pause.where(…)`). `ActiveJob::Querying`, `Executing` and `Failed` are `concern`s noted `included into ActiveJob::Base`.
19. [nice] `MissionControl::Jobs::Server#activating` drops its restoring `ensure` (inspector only) and shows `queue_adapter.activating` → `block`.
20. [nice] `MissionControl::Jobs::HostRouteHelpers` shows `define_from` tagged `class method` (from `class << self`), with its `define_method` inside a loop tagged `dynamic`. No method list is invented for it.

### 4.3 thor (plain gem)

1. [must] `bin/thor` (no extension, Ruby shebang) is analyzed as Ruby, with badge `entry point` (gemspec `executables`) and subtitle `executable`. Its `start` step references `start` in `lib/thor/base.rb` (`Thor::Runner < Thor`, and `Thor` includes `Thor::Base`, whose `included` hook extends `ClassMethods`).
2. [must] Repository: `bin/thor` depends on `lib/thor/runner.rb` (`require "thor/runner"` resolved through `require_paths`). `lib/thor/runner.rb` depends on `lib/thor.rb` and `lib/thor/group.rb` (`require_relative "../thor"`, `"group"`).
3. [must] `autoload` is an import edge: `lib/thor/base.rb` imports `lib/thor/actions.rb`, `rake_compat.rb` and `group.rb` (`autoload :Actions, File.expand_path("actions", __dir__)`), and `lib/thor/shell.rb` imports `shell/basic.rb`, `shell/color.rb` and `shell/html.rb`. The `group.rb` ↔ `base.rb` cycle (`group.rb` does `require_relative "base"`) is drawn without a diagnostic.
4. [must] The stdlib requires `rbconfig`, `digest/sha2`, `pathname`, `fileutils`, `open-uri` and `open3` are not listed as packages. The gemspec gives no runtime packages (only the development dependency `bundler`, in the inspector).
5. [must] `lib/thor/runner.rb` subtitle `commands`. `Thor::Runner` commands have subtitles `install NAME`, `version`, `uninstall NAME`, `update NAME`, `installed` and `list [SEARCH]`, each with its `desc` text as doc.
6. [must] `install` has an options table with rows `--as` (`string`), `--relative` (`boolean`) and `--force` (`boolean`). `list` has rows `--substring`, `--group`, `--all`, `--debug`.
7. [must] `map "-T" => :list, "-i" => :install, "-u" => :update, "-v" => :version` puts the alias badges `-T` on `list`, `-i` on `install`, `-u` on `update` and `-v` on `version`.
8. [must] `thor_root`, `thor_yaml`, `save_yaml` and `initialize_thorfiles` in `runner.rb` are in a `private` band with no command subtitle. `Thor::Runner#method_missing` is tagged `catch-all`.
9. [must] `lib/thor.rb`: `Thor#help` subtitle `help [COMMAND]`, `Thor#tree` subtitle `tree`. The class-level `desc`, `method_option` and `register` (inside `class << self`) are tagged `class method`, and there is no `class << self` box. `build_command_tree` is private.
10. [must] There are no namespace-wrapper boxes: `lib/thor/shell/basic.rb` shows `Basic` (qualified `Thor::Shell::Basic`) as its top definition, with no `Thor` or `Shell` box around it. `lib/thor/actions/create_file.rb` shows `CreateFile` with base `EmptyDirectory`, and `create_link.rb` shows `CreateLink` with base `CreateFile`.
11. [must] `Thor::Command` (`< Struct.new(…)`) has a fields table with the rows `name, description, long_description, wrap_long_description, usage, options, options_relation, ancestor_name`. `HiddenCommand` and `DynamicCommand` have base `Command`.
12. [must] `Thor::Actions` uses `def self.included(base); base.extend ClassMethods`, so the methods of `Thor::Actions::ClassMethods` are class methods of the includer, and the `self.included` hook is not drawn as a regular method block.
13. [must] `lib/thor/error.rb`: `Error`, `UndefinedCommandError`, `InvocationError` and `RequiredArgumentMissingError` are tagged `error type`, with bases `StandardError`, `Error`, `Error` and `InvocationError`. `prepend Correctable if Correctable` is a guarded mixin (`Correctable · if Correctable`), not a call box.
14. [must] `Thor::Runner#install`: the exits `Error opening URI` and `Error opening file` are tagged `raises Thor::Error`, inside the arms `on OpenURI::HTTPError` and `on Errno::ENOENT`. Effects include `network` (`URI.open`) and `file` (`File.open`). The lazy `require "open-uri"` is not a block.
15. [must] `Thor::Util` (all methods in `class << self`): `find_class_and_command_by_namespace` is tagged `class method`, and the call `Thor::Util.find_class_and_command_by_namespace(meth)` in `Thor::Runner#help` references it.
16. [nice] `Thor::Base.shell` defined in `lib/thor/shell.rb` (a reopened `module Base`) is found from `Thor::Base.shell.new` in `base.rb`'s `start`. `Thor::Shell`'s `SHELL_DELEGATED_METHODS.each { module_eval … }` is a table `delegated to shell` with 13 rows including `say` and `ask`.
17. [nice] `Thor::Shell::Color` has base `Basic`, and its `set_color` overrides `Basic#set_color` (said in the inspector or with an `overrides` tag). `Thor::Shell::Basic#say` has the marker `prints`.
18. [nice] `lib/thor/rake_compat.rb`'s top-level `instance_eval do … end` is drawn as a section, not as an unnamed call box, and `klass.send :define_method, non_namespaced_name` inside it is tagged `dynamic`.
19. [nice] `Thor::Base::ClassMethods#start` (`lib/thor/base.rb`) is `Thor::Base.shell.new → dispatch`, with arms `on Thor::Error` (a branch whose `raise e` exit is tagged `re-raises`, the alternative being `error` on the shell, then `exit(false) · if exit_on_failure?`) and `on Errno::EPIPE`.

---

## 5. Anti-patterns (plausible diagrams that read wrong to a Rails developer)

1. **A block per declaration line.** Twelve `validates` boxes on `Story`, 25 `has_many` boxes, eight `before_action` call boxes in a controller's body. These are tables and markers.
2. **`params` as a box** (or `params[:id]` as a call step). `params` is an input. `flash[:error] = …` as its own block next to every redirect is just as wrong.
3. **`render`/`redirect_to` as library-call boxes** that flow into nothing. They are the response, and the action's exits.
4. **A result arrow from a filter or action** (`require_logged_in_user` "returns `true`", `edit` "returns `@title`"). Rails ignores those values, so the result of an action is its response, including the implicit template.
5. **A disconnected Rails app**: controllers with no edges to views (implicit render ignored), models with no edges to controllers (no `require`s, so no imports), `find_story` unresolved because it comes from a concern. The conventions *are* the imports.
6. **`included do` as a box**, `extend ActiveSupport::Concern` as a dependency edge to a library, `class << self` or `private` as a box, or `module MissionControl; module Jobs` as two nested empty boxes around every engine file.
7. **One giant `Thor` class merged from 30 files**, or 30 `Thor` boxes, one per file. A reopened namespace is not a component.
8. **Everything with `.where` marked as database.** `ActiveJob.jobs.where(…)` is a project relation over a queue adapter, and `ActiveModel::Model` classes (`Worker`, `Batch`) or a PORO in `app/models/` (`ShortId`) badged `DB table` is wrong too. `session`/`cookies` are not a database either.
9. **A query chain drawn as `where → where → order → first`, four boxes.** It is one database step named by the call that runs it.
10. **`respond_to` as a call with two lambda boxes** labeled `html`/`json`. It is a branch on format.
11. **Routes as one 250-row table**, or as 250 `get` call boxes inside a `draw` block. They are route tables grouped as the file groups them, with rows linking to actions and actions subtitled with routes.
12. **Association edges for every declaration**: two arrows per pair (`has_many` and `belongs_to`), extra edges for `through:`, self-loops for `merged_into_story`. That is a hairball rails-erd itself avoids.
13. **Filters repeated on every action.** `authenticate_user`, `set_traffic_style` and `mini_profiler` as markers on all 25 lobsters actions drown the meaningful ones (`require_logged_in_user`, `find_user_story`).
14. **Callbacks in source order.** Readers reason about `before_validation → before_save → after_create → after_save → after_commit`, not about where the line happens to be.
15. **`attr_accessor`, `mattr_accessor`, `helper_method`, `delegate`, `include`, `queue_as`, `layout` as call steps** in a class body.
16. **ERB drawn as Ruby dataflow**, with a box per `<%= %>`, `link_to` and `f.submit`. A view is the partials it renders, the helpers it calls and the ivars it reads.
17. **Code in titles**: `self.hide_story_for_user`, `ActiveJob.jobs.failed.where(**@job_filters).limit(MAX…`, `format.html { … }`, `STATUSES.each do |status| define_method status`. Titles are names (`hide_story_for_user` tagged `class method`, `bulk_limited_filtered_failed_jobs`, `html`, `pending, failed, …`).
18. **Treating `method_missing` as the target of every unresolved call.** It is a catch-all that is drawn, not a resolution strategy.
19. **The Python and Go words**: `except`, `defer`, `err != nil`, `__init__`. Ruby says `rescue`, `ensure`, `raise` and `initialize`, and Rails says `action`, `filter`, `callback`, `concern`, `scope` and `association`.
