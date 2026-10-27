# Rails views: ERB and view components

## Where it stands

Rails support stops at the controller. A file under `app/views/` gets a role from
where it lives (view, partial, layout, see `view_role` in
`crates/blockviz-core/src/ruby/rails.rs`), and an action links to the views it
renders. What a view contains is not read: its Ruby, the partials it renders, the
helpers it calls and the instance variables it uses are all unseen.

## What to extend to

- **ERB** (`.html.erb`, `.turbo_stream.erb`, `.text.erb`): pull the Ruby out of
  `<% %>` and `<%= %>` tags, keeping its positions in the file, and parse it with
  Prism like any other Ruby. Blocks that span tags (`<% @stories.each do |s| %> … <% end %>`)
  are loops, `<% if %>` branches are branches.
- **Partials and layouts**: `render "form"`, `render partial:`, `render @stories`
  (a collection, so `stories/_story`), `render layout:`, and `yield` / `content_for`
  link a view to what it draws in or around it.
- **Instance variables**: `@story` set in an action reaches the view that action
  renders, so the arrows run from controller to view; a partial's `locals:` are its
  arguments.
- **Helpers**: a call without a receiver in a view is a method of `app/helpers/`
  (or a Rails helper such as `link_to`, `form_with`, `turbo_frame_tag`, read as
  framework calls).
- **View components** (`app/components/*_component.rb` with a template beside it, or
  a `call` method): the component class and its template are one block; `render
  FooComponent.new(...)` in a view links to it, its `initialize` arguments are its
  inputs, and slots (`renders_one`, `renders_many`) are its parts. Phlex views
  (`Views::Base`, `Components::Base`) are Ruby already and need only the same render
  links.
- **Other templates**, after ERB: HAML and Slim (their Ruby lines are marked by
  `-` and `=`), Jbuilder (Ruby, a JSON response), Turbo Stream templates as responses.

## Tests

Add a trimmed Rails app with views, partials and a ViewComponent to
`tests/corpus_ruby.rs` (lobsters already has many ERB views), and check that an
action's view lists the partials and helpers it uses and the instance variables it
reads from the action.
