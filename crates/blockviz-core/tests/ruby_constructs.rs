//! Ruby constructs read the way a developer would draw them: the same rules as
//! the other languages, through Ruby's idioms. Methods called without a
//! receiver or parentheses, blocks as loops and comprehensions, `@fields`,
//! implicit returns, `unless` and modifier guards, `raise` and `rescue`, and
//! classes known by their full names across files that never require each other.

use blockviz_core::ir::{Kind, Node, Scene};
use blockviz_core::project::InFile;

fn analyze(files: &[(&str, &str)]) -> Scene {
    let s = blockviz_core::analyze("t", files.iter().map(|(p, t)| InFile { path: (*p).into(), text: Some((*t).into()), bytes: t.len() as u64 }).collect());
    assert!(!s.diagnostics.iter().any(|d| d.contains("Syntax error")), "{:?}", s.diagnostics);
    s
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
    descendants(s, n).into_iter().find(|c| c.title == title).unwrap_or_else(|| panic!("{} has no {title:?}; has {:?}", n.title, descendants(s, n).iter().map(|c| &c.title).collect::<Vec<_>>()))
}

fn file<'a>(s: &'a Scene, path: &str) -> &'a Node {
    let i = s.files.iter().position(|f| f.path == path).unwrap_or_else(|| panic!("no file {path}"));
    s.nodes.iter().find(|n| matches!(n.kind, Kind::File) && n.span.as_ref().is_some_and(|sp| sp.file as usize == i)).unwrap()
}

/// A definition directly in a file (not a call of it).
fn def<'a>(s: &'a Scene, n: &'a Node, title: &str) -> &'a Node {
    descendants(s, n).into_iter().find(|c| c.title == title && matches!(c.kind, Kind::Function | Kind::Class)).unwrap_or_else(|| panic!("{} defines no {title:?}", n.title))
}

const SHOP: &str = r##"require "faraday"
require_relative "shop/cart"

# Shop entry points.
module Shop
  def self.checkout(items)
    cart = Cart.new(items)
    return nil if cart.empty?
    total = cart.total
    receipt = Receipt.new(total)
    receipt.print
  end
end
"##;

const CART: &str = r##"module Shop
  class Cart
    attr_reader :items

    def initialize(items)
      @items = items
    end

    def empty?
      items.empty?
    end

    # The sum of the item prices, with tax.
    def total
      subtotal = items.map { |i| i.price }.sum
      raise ArgumentError, "negative total" unless subtotal >= 0
      with_tax(subtotal)
    end

    def each_line
      items.each do |item|
        next if item.price.zero?
        puts "#{item.name}: #{item.price}"
      end
    end

    private

    def with_tax(amount)
      amount * 1.21
    end
  end

  class Receipt
    def initialize(total)
      @total = total
    end

    def print
      response = Faraday.post("https://example.com/receipts", total: @total)
      response.success?
    rescue Faraday::Error => e
      warn e.message
      false
    end
  end
end
"##;

fn shop() -> Scene {
    analyze(&[("Gemfile", "source \"https://rubygems.org\"\ngem \"faraday\"\n"), ("lib/shop.rb", SHOP), ("lib/shop/cart.rb", CART)])
}

#[test]
fn calls_reach_the_methods_of_the_classes_they_construct() {
    let s = shop();
    let checkout = def(&s, file(&s, "lib/shop.rb"), "checkout");
    assert!(checkout.badges.contains(&"class method".to_string()), "{:?}", checkout.badges);
    // `Cart.new(items)` constructs a `Cart`, so `cart.total` is `Cart#total`,
    // found though `Cart` is written inside `module Shop` in another file.
    let cart = file(&s, "lib/shop/cart.rb");
    assert_eq!(within(&s, checkout, "total").reference, Some(def(&s, cart, "total").id));
    assert_eq!(within(&s, checkout, "print").reference, Some(def(&s, cart, "print").id));
    // `return nil if cart.empty?` is a guarded exit; the last expression is the result.
    let guard = within(&s, checkout, "return nil");
    assert!(guard.badges.iter().any(|b| b == "if cart.empty?"), "{:?}", guard.badges);
    // A namespace is not a block around its classes, and classes are drawn by their own names.
    let kids: Vec<&str> = cart.children.iter().map(|&c| s.nodes[c as usize].title.as_str()).collect();
    assert_eq!(kids, ["Cart", "Receipt"]);
    // The file using `Shop::Cart` depends on the file defining it.
    let root = &s.nodes[0];
    let uses = root.edges.iter().chain(descendants(&s, root).iter().flat_map(|n| n.edges.iter())).any(|e| e.label.contains("Shop::Cart"));
    assert!(uses);
}

#[test]
fn ruby_idioms_are_steps_guards_and_loops() {
    let s = shop();
    let cart = file(&s, "lib/shop/cart.rb");
    // A method called without a receiver is the class's own.
    let total = def(&s, cart, "total");
    assert_eq!(within(&s, total, "with_tax").reference, Some(def(&s, cart, "with_tax").id));
    assert_eq!(total.doc, "The sum of the item prices, with tax.");
    // `unless` guards the raise, in Ruby's words.
    let raise = within(&s, total, "raise ArgumentError");
    assert!(raise.badges.iter().any(|b| b.starts_with("unless")), "{:?}", raise.badges);
    assert!(total.effects.contains(&"raises".to_string()));
    // `items.each do |item|` is a loop over the items; `puts` prints, as a marker.
    let each = def(&s, cart, "each_line");
    let looped = within(&s, each, "for item in items");
    assert_eq!(looped.kind, Kind::Loop);
    assert!(!descendants(&s, each).iter().any(|n| n.title == "puts"));
    assert!(each.effects.contains(&"prints".to_string()));
    // `rescue` is a lane of its own, named as Ruby names it; Faraday is network I/O.
    let print = def(&s, cart, "print");
    assert!(descendants(&s, print).iter().any(|n| n.title == "rescue Faraday::Error"));
    assert!(print.effects.contains(&"network".to_string()), "{:?}", print.effects);
    // Gems are packages; the standard library is not.
    let packages = s.nodes.iter().find(|n| n.title == "External packages").expect("packages");
    assert!(packages.rows.iter().any(|r| r.key == "faraday"));
}

#[test]
fn reopened_and_namespaced_classes() {
    let s = analyze(&[
        ("thor.gemspec", "Gem::Specification.new { |s| s.name = \"thor\" }\n"),
        ("lib/thor.rb", "class Thor\n  def help\n    say \"hi\"\n  end\nend\n"),
        ("lib/thor/error.rb", "class Thor\n  class Error < StandardError\n  end\n\n  class UndefinedCommandError < Error\n  end\nend\n"),
        ("lib/thor/version.rb", "class Thor\n  def version = \"1.0\"\nend\n"),
    ]);
    // `class Thor` opened only to nest `Error` is no block of its own.
    let errors = file(&s, "lib/thor/error.rb");
    let kids: Vec<(&str, &str)> = errors.children.iter().map(|&c| (s.nodes[c as usize].title.as_str(), s.nodes[c as usize].subtitle.as_str())).collect();
    assert_eq!(kids, [("Error", "StandardError"), ("UndefinedCommandError", "Thor::Error")]);
    // Methods added in another file lead to the class they extend.
    let version = file(&s, "lib/thor/version.rb");
    let ext = &s.nodes[version.children[0] as usize];
    assert_eq!(ext.link, Some(def(&s, file(&s, "lib/thor.rb"), "Thor").id));
}

#[test]
fn a_ruby_executable_and_its_main_guard() {
    let s = analyze(&[
        ("bin/tool", "#!/usr/bin/env ruby\nrequire_relative \"../lib/tool\"\nTool.run(ARGV)\n"),
        ("lib/tool.rb", "class Tool\n  def self.run(args)\n    new.call(args)\n  end\n\n  def call(args)\n    args.each { |a| puts a }\n  end\nend\n\nif __FILE__ == $0\n  Tool.run(ARGV)\nend\n"),
    ]);
    let bin = file(&s, "bin/tool");
    assert_eq!(bin.subtitle, "executable");
    assert!(bin.badges.contains(&"entry point".to_string()));
    // `if __FILE__ == $0` runs the file as a program.
    assert!(file(&s, "lib/tool.rb").badges.contains(&"entry point".to_string()));
}
