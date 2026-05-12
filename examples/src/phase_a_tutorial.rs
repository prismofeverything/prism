//! Phase A tutorial — a single-file walkthrough of Milner's
//! bigraphical algebra over `prism-schema`'s `Pattern` type.
//!
//! Run with:
//!
//! ```text
//! cargo run --bin phase_a_tutorial -- --out out
//! open out/phase_a_tutorial.html
//! ```
//!
//! The binary walks through interfaces, composition, tensor, and the
//! elementary bigraphs in order, rendering each step to SVG (via
//! Graphviz when available, in-process otherwise) and bundling the
//! whole sequence into a single HTML report.

use std::fs;
use std::path::{Path, PathBuf};

use prism_schema::reaction::Pattern;
use prism_schema::{
    barren, compose, interfaces, ion, is_ground, merge, tensor, to_value,
    Value,
};
use prism_viz::{render_pattern_dot, render_pattern_svg, render_to_file};

struct Step {
    heading: &'static str,
    body: String,
    figures: Vec<Figure>,
}

struct Figure {
    file: String,
    caption: String,
}

struct Ctx {
    out_dir: PathBuf,
    use_graphviz: bool,
    figure_counter: usize,
}

impl Ctx {
    fn render(&mut self, pattern: &Pattern, basename: &str) -> String {
        self.figure_counter += 1;
        let filename = format!("phase_a_fig_{:02}_{basename}.svg", self.figure_counter);
        let path = self.out_dir.join(&filename);
        if self.use_graphviz {
            let dot = render_pattern_dot(pattern, "");
            if render_to_file(&dot, &path, "svg").is_err() {
                let _ = fs::write(&path, render_pattern_svg(pattern, ""));
            }
        } else {
            let _ = fs::write(&path, render_pattern_svg(pattern, ""));
        }
        filename
    }
}

fn main() {
    let out = parse_out_dir();
    fs::create_dir_all(&out).expect("create out");
    let use_graphviz = which_dot();
    let mut ctx = Ctx {
        out_dir: out.clone(),
        use_graphviz,
        figure_counter: 0,
    };

    println!(
        "📖 Generating Phase A tutorial (renderer: {})",
        if use_graphviz { "graphviz" } else { "in-process SVG" }
    );

    let steps = vec![
        step_intro(&mut ctx),
        step_interfaces(&mut ctx),
        step_ground(&mut ctx),
        step_barren(&mut ctx),
        step_merge(&mut ctx),
        step_ion(&mut ctx),
        step_compose(&mut ctx),
        step_tensor(&mut ctx),
        step_layered(&mut ctx),
        step_to_value(&mut ctx),
        step_recap(&mut ctx),
    ];

    let html_path = out.join("phase_a_tutorial.html");
    let html = render_html(&steps);
    fs::write(&html_path, html).expect("write html");
    println!("📰 {}", html_path.display());
}

fn parse_out_dir() -> PathBuf {
    let mut out = PathBuf::from("out");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--out" {
            if let Some(s) = args.next() {
                out = PathBuf::from(s);
            }
        }
    }
    out
}

fn which_dot() -> bool {
    std::process::Command::new("dot")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── Steps ───────────────────────────────────────────────────────────

fn step_intro(_ctx: &mut Ctx) -> Step {
    Step {
        heading: "What this is",
        body: r#"
<p>This is a hands-on walk through <strong>Phase A</strong> of our
bigraphs port: the algebraic core of Milner's bigraphical formalism
that lets you build place graphs out of small standard pieces using
two operators — <code>compose</code> and <code>tensor</code> — and a
handful of <em>elementary bigraphs</em>.</p>

<p>The point: instead of writing every state by hand, you can
<em>assemble</em> states algebraically. Reaction rules then become
arrows in a category, you can talk about their faces (where they
plug in), and you get rigorous laws (associativity, identity) for
free.</p>

<p>Concretely, in this codebase, all of the algebra operates on
<code>Pattern</code> — the same type the reaction matcher uses for
redex/reactum. A pattern with no <code>Site</code> markers is a
<em>ground</em> bigraph (a concrete state); a pattern <em>with</em>
sites is a <em>context</em> waiting to be filled.</p>

<p>References, in order of usefulness:</p>
<ul>
<li>Milner, <em>Space and Motion of Communicating Agents</em> (2008) —
  Chapters 2 (definitions) and 3 (algebra). The PDF lives at
  <code>~/Downloads/Bigraphs-draft.pdf</code> if you want primary source.</li>
<li><code>crates/prism-schema/src/assembly.rs</code> — the implementation.</li>
</ul>
"#.into(),
        figures: vec![],
    }
}

fn step_interfaces(ctx: &mut Ctx) -> Step {
    // Compartment with one site, one root.
    let p = Pattern::sort("Compartment", [("body", Pattern::site())]);
    let p_outer = Pattern::map([("region0", p.clone())]);
    let fig = ctx.render(&p_outer, "interface");

    let (inner, outer) = interfaces(&p_outer);
    let body = format!(
        r#"
<p>Every bigraph has an <strong>interface</strong>
<code>I = ⟨m, X⟩</code>. <code>m</code> is the place-graph width — the
number of <em>sites</em> (holes) — and <code>X</code> is the set of
<em>names</em> on the link graph. (Names are link-graph endpoints
awaiting connection from outside; we don't model those as first-class
yet, so this tutorial focuses on the place half.)</p>

<p>In our model:</p>
<ul>
<li><code>InnerFace.sites</code> = the positions of every
  <code>Pattern::Site</code> in the tree, walking left-to-right.</li>
<li><code>OuterFace.roots</code> = the top-level dict keys of the
  pattern — the <em>regions</em> the bigraph exposes.</li>
</ul>

<p>The figure below shows a single Compartment with one site. Its
interface is <code>⟨1, ∅⟩ → ⟨1, ∅⟩</code> — one site (the
<code>body</code> hole), one root (<code>region0</code>):</p>

<pre><code>let p = Pattern::sort("Compartment", [("body", Pattern::site())]);
let p_outer = Pattern::map([("region0", p)]);
let (inner, outer) = interfaces(&amp;p_outer);
assert_eq!(inner.width(), 1);   // {sites_count}
assert_eq!(outer.width(), 1);   // {roots_count}
</code></pre>

<p>Inner-face sites: <code>{sites:?}</code>.<br/>
Outer-face roots: <code>{roots:?}</code>.</p>
"#,
        sites_count = inner.width(),
        roots_count = outer.width(),
        sites = inner.sites,
        roots = outer.roots,
    );
    Step {
        heading: "Interfaces — what plugs into what",
        body,
        figures: vec![Figure {
            file: fig,
            caption: "A Compartment with one open site, hosting one region.".into(),
        }],
    }
}

fn step_ground(ctx: &mut Ctx) -> Step {
    let open = Pattern::map([(
        "region0",
        Pattern::sort("Compartment", [("body", Pattern::site())]),
    )]);
    let ground = Pattern::map([(
        "region0",
        Pattern::sort(
            "Compartment",
            [(
                "body",
                Pattern::sort("Cell", Vec::<(&str, Pattern)>::new()),
            )],
        ),
    )]);

    let f_open = ctx.render(&open, "open");
    let f_ground = ctx.render(&ground, "ground");
    let body = format!(
        r#"
<p>A pattern with no sites is <strong>ground</strong>. That's what
every concrete simulation state is, and it's what every reaction
firing produces. Open patterns are the in-between objects you
manipulate algebraically before grounding everything.</p>

<pre><code>assert!(!is_ground(&amp;open));    // has a Site hole
assert!( is_ground(&amp;ground));   // no Sites, ready to run
</code></pre>

<p>Same Compartment, before and after grounding. (The dashed
<em>?</em> on the left is the <code>Site</code> marker; on the right
it's been filled with a Cell.)</p>
"#,
    );
    let _ = (is_ground(&open), is_ground(&ground)); // keep the assertion live

    Step {
        heading: "Ground vs open — the same Compartment, before and after",
        body,
        figures: vec![
            Figure { file: f_open, caption: "Open: one hole, awaiting a child.".into() },
            Figure { file: f_ground, caption: "Ground: the hole is filled with Cell.".into() },
        ],
    }
}

fn step_barren(ctx: &mut Ctx) -> Step {
    let b = barren("region0");
    let f = ctx.render(&b, "barren");
    Step {
        heading: "barren — the empty bigraph 1 : 0 → 1",
        body: r#"
<p>The simplest elementary bigraph. One region, no sites, no nodes.
Milner calls this <code>1</code> and treats it as the identity-like
element. Compose anything with the right interface against
<code>1</code> and you get just that thing back.</p>

<pre><code>let b = barren("region0");
// inner: ε,  outer: ⟨1, ∅⟩
</code></pre>
"#.into(),
        figures: vec![Figure {
            file: f,
            caption: "barren(\"region0\") — one empty region.".into(),
        }],
    }
}

fn step_merge(ctx: &mut Ctx) -> Step {
    let m0 = merge(0, "root");
    let m1 = merge(1, "root");
    let m3 = merge(3, "root");
    let f0 = ctx.render(&m0, "merge0");
    let f1 = ctx.render(&m1, "merge1");
    let f3 = ctx.render(&m3, "merge3");

    Step {
        heading: "merge_n — collapse n regions into 1",
        body: r#"
<p><code>merge_n : n → 1</code> is a single region containing
<code>n</code> sites. Composed with anything that has <code>n</code>
roots, it pulls those roots side-by-side into a single region.
<code>merge_0 = 1</code> (the barren root).</p>

<pre><code>let m3 = merge(3, "root");   // outer face: 1 root, inner face: 3 sites
</code></pre>

<p>This is the "put these all inside one container" combinator.
You'll see it whenever you have a bunch of independently-built
sub-bigraphs and want to drop them under a shared parent.</p>
"#.into(),
        figures: vec![
            Figure { file: f0, caption: "merge_0 = barren — degenerate case.".into() },
            Figure { file: f1, caption: "merge_1 — one region with one site.".into() },
            Figure { file: f3, caption: "merge_3 — one region with three parallel sites.".into() },
        ],
    }
}

fn step_ion(ctx: &mut Ctx) -> Step {
    let k = ion("K", &["x", "y"], "site0");
    let f = ctx.render(&k, "ion");
    Step {
        heading: "ion K_⃗x — a single labelled node with a hole",
        body: r#"
<p>An <strong>ion</strong> is one labelled node with one site
inside and zero or more named ports. Compose an ion with a child
bigraph and you've built a hierarchical compartment. Stack a chain
of ions and you've got nested places — which is how the MAPK
example builds <code>Cell ⊃ Cytoplasm ⊃ Nucleus</code>.</p>

<pre><code>let k = ion("K", &amp;["x", "y"], "site0");
// outer face: one root labelled "K"
// inner face: one site
// names: ports x and y (recorded; first-class Link composition is a
//        future milestone)
</code></pre>
"#.into(),
        figures: vec![Figure {
            file: f,
            caption: "K-ion with two port names and one open site.".into(),
        }],
    }
}

fn step_compose(ctx: &mut Ctx) -> Step {
    // outer: { region0: Compartment { body: Site } }
    // inner: { body: ERK }                          (a 1-root, 0-site arrow)
    // composed: { region0: Compartment { body: ERK } }
    let outer = Pattern::map([(
        "region0",
        Pattern::sort("Compartment", [("body", Pattern::site())]),
    )]);
    let inner = Pattern::map([(
        "body",
        Pattern::sort("ERK", Vec::<(&str, Pattern)>::new()),
    )]);
    let composed = compose(&outer, &inner).expect("compose");

    let f_outer = ctx.render(&outer, "compose_outer");
    let f_inner = ctx.render(&inner, "compose_inner");
    let f_composed = ctx.render(&composed, "compose_result");

    let (outer_inner, _) = interfaces(&outer);
    let (_, inner_outer) = interfaces(&inner);

    let body = format!(
        r#"
<p><code>compose(outer, inner)</code> substitutes
<code>inner</code>'s roots into <code>outer</code>'s sites, in walk
order. Milner Def. 2.5 calls this <code>G ∘ F</code> and writes:</p>

<blockquote><code>G ∘ F : I → K</code> requires
<code>outer(F) = inner(G)</code>; the mediating face disappears, and
the composite has <code>F</code>'s inner face and <code>G</code>'s
outer face.</blockquote>

<p>In our model: outer has <strong>{outer_sites}</strong> sites,
inner has <strong>{inner_roots}</strong> roots. They match, so compose
is well-defined. The result is ground.</p>

<pre><code>let composed = compose(&amp;outer, &amp;inner)?;
assert!(is_ground(&amp;composed));   // the hole is gone
</code></pre>

<p>Below: outer (with a hole) on top, inner (the filler) in the
middle, the composite on the bottom.</p>
"#,
        outer_sites = outer_inner.width(),
        inner_roots = inner_outer.width(),
    );

    Step {
        heading: "compose — plug a bigraph into another's hole",
        body,
        figures: vec![
            Figure { file: f_outer, caption: "outer: Compartment with one open site.".into() },
            Figure { file: f_inner, caption: "inner: an ERK at the matching root key.".into() },
            Figure { file: f_composed, caption: "compose(outer, inner): Compartment containing ERK.".into() },
        ],
    }
}

fn step_tensor(ctx: &mut Ctx) -> Step {
    let left = Pattern::map([(
        "cytoplasm",
        Pattern::sort("Compartment", [("kind", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new()))]),
    )]);
    let right = Pattern::map([(
        "nucleus",
        Pattern::sort("Compartment", [("kind", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new()))]),
    )]);
    let combined = tensor(&left, &right).expect("tensor");

    let f_l = ctx.render(&left, "tensor_left");
    let f_r = ctx.render(&right, "tensor_right");
    let f_t = ctx.render(&combined, "tensor_result");

    Step {
        heading: "tensor — place two bigraphs side by side",
        body: r#"
<p><code>tensor(left, right)</code> juxtaposes two bigraphs into one,
concatenating their faces (Milner Def. 2.7). The top-level region
keys must be disjoint — bigraphs share state by composition, not by
sloppy merge.</p>

<pre><code>let combined = tensor(&amp;left, &amp;right)?;
// outer face width = left.outer + right.outer = 2
</code></pre>

<p>This is the &ldquo;independent worlds&rdquo; operator: combining
two non-interacting bigraphs into a parallel composite. To then
<em>connect</em> them, you compose against an outer template that
holds them inside a shared parent.</p>
"#.into(),
        figures: vec![
            Figure { file: f_l, caption: "left: a cytoplasm compartment.".into() },
            Figure { file: f_r, caption: "right: a nucleus compartment.".into() },
            Figure { file: f_t, caption: "left ⊗ right: both regions, disjoint.".into() },
        ],
    }
}

fn step_layered(ctx: &mut Ctx) -> Step {
    // Build a Cell containing both Cytoplasm and Nucleus, using tensor
    // for the children and compose to drop them inside Cell.
    let cyto = Pattern::map([("cyto", Pattern::sort("Cytoplasm", Vec::<(&str, Pattern)>::new()))]);
    let nuc = Pattern::map([("nuc", Pattern::sort("Nucleus", Vec::<(&str, Pattern)>::new()))]);
    let children = tensor(&cyto, &nuc).expect("tensor");

    // outer: a Cell with TWO sites side by side.
    let outer = Pattern::map([(
        "region0",
        Pattern::sort(
            "Cell",
            [("cyto", Pattern::site()), ("nuc", Pattern::site())],
        ),
    )]);

    let composed = compose(&outer, &children).expect("compose");
    let f_outer = ctx.render(&outer, "layered_outer");
    let f_children = ctx.render(&children, "layered_children");
    let f_result = ctx.render(&composed, "layered_result");

    Step {
        heading: "compose + tensor together — building Cell ⊃ {Cyto, Nuc}",
        body: r#"
<p>The two operators compose (no pun intended) to build complex
structure from small pieces. Here, two singleton bigraphs are
tensored to form <em>"a Cytoplasm and a Nucleus, disjoint"</em>; then
an outer Cell pattern with two sites is composed against them to get
a Cell that contains both, in their original places.</p>

<pre><code>let children = tensor(&amp;cyto, &amp;nuc)?;       // outer width 2
let outer    = Pattern::map([("region0",
    Pattern::sort("Cell",
        [("cyto", Pattern::site()), ("nuc", Pattern::site())]))]);
let composed = compose(&amp;outer, &amp;children)?;  // ground
</code></pre>

<p>This is the same shape the MAPK example uses for its place graph,
just hand-built one piece at a time.</p>
"#.into(),
        figures: vec![
            Figure { file: f_outer, caption: "outer: Cell with two named sites.".into() },
            Figure { file: f_children, caption: "children: tensor of two singleton bigraphs.".into() },
            Figure { file: f_result, caption: "compose(outer, children): the assembled cell.".into() },
        ],
    }
}

fn step_to_value(ctx: &mut Ctx) -> Step {
    let outer = Pattern::map([(
        "region0",
        Pattern::sort("Cell", [("body", Pattern::site())]),
    )]);
    let inner = Pattern::map([(
        "body",
        Pattern::sort(
            "Cytoplasm",
            [("mass", Pattern::atom(Value::Float(1.0.into())))],
        ),
    )]);
    let composed = compose(&outer, &inner).expect("compose");
    let v = to_value(&composed).expect("ground value");

    let json = serde_json::to_string_pretty(&v).unwrap_or_default();
    let f = ctx.render(&composed, "to_value");

    Step {
        heading: "Grounding the assembled pattern → simulation state",
        body: format!(
            r#"
<p>Once a pattern is ground (no sites left), <code>to_value</code>
converts it to a <code>Value</code> that the engine can run. This
is the bridge from algebraic assembly to actual simulation state.</p>

<pre><code>let composed = compose(&amp;outer, &amp;inner)?;
assert!(is_ground(&amp;composed));
let state: Value = to_value(&amp;composed)?;
</code></pre>

<p>Serialized as JSON, the grounded state looks like:</p>

<pre><code>{json}</code></pre>
"#,
            json = html_escape(&json),
        ),
        figures: vec![Figure {
            file: f,
            caption: "Final ground pattern, ready for to_value.".into(),
        }],
    }
}

fn step_recap(_ctx: &mut Ctx) -> Step {
    Step {
        heading: "Recap",
        body: r#"
<p>What you've now got at your disposal:</p>

<dl>
<dt><code>interfaces(pattern) → (InnerFace, OuterFace)</code></dt>
<dd>The site positions (inner) and root keys (outer) of any pattern.</dd>

<dt><code>is_ground(pattern) → bool</code></dt>
<dd>True iff the pattern has no remaining holes.</dd>

<dt><code>barren(key)</code>, <code>merge(n, key)</code>,
    <code>ion(label, names, site)</code></dt>
<dd>The three elementary place-bigraphs from Milner Ch. 3.</dd>

<dt><code>compose(outer, inner)</code></dt>
<dd>Substitute <code>inner</code>'s roots into <code>outer</code>'s
sites. Fails if face widths don't match.</dd>

<dt><code>tensor(left, right)</code></dt>
<dd>Place two bigraphs side by side. Fails if region keys overlap.</dd>

<dt><code>to_value(pattern)</code> / <code>from_value(value)</code></dt>
<dd>Round-trip between ground patterns and engine-ready
<code>Value</code>s.</dd>
</dl>

<p>What's <em>not</em> in Phase A yet (deferred):</p>
<ul>
<li>First-class <code>Link</code> nodes with link-graph composition.
  (Today we encode wiring as paths inside <code>outputs: {...}</code>;
  promoting that to a typed Link is straightforward but ripples
  through the matcher.)</li>
<li><code>substitution(y/X)</code> and <code>closure(/x)</code> — the
  remaining elementary bigraphs that operate on names.</li>
<li>Sorting disciplines from Ch. 6 — useful for enforcing
  &ldquo;Compartments contain Compartments&rdquo; structural
  invariants, but not needed for MAPK.</li>
</ul>

<p>These slot in cleanly as additions to the same module; nothing in
Phase A's API would have to change. Once they land we'll have the
full Milner Ch. 2–3 algebra running in Rust.</p>
"#.into(),
        figures: vec![],
    }
}

// ── HTML rendering ──────────────────────────────────────────────────

fn render_html(steps: &[Step]) -> String {
    let toc: String = steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let n = i + 1;
            let h = html_escape(s.heading);
            format!(r##"<li><a href="#step-{n}">{h}</a></li>"##)
        })
        .collect();

    let body: String = steps
        .iter()
        .enumerate()
        .map(|(i, s)| render_step(i + 1, s))
        .collect();

    format!(
        r##"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Phase A tutorial — bigraphical algebra in prism</title>
<style>
  :root {{
    --bg: #fcfcfc; --fg: #222; --muted: #555;
    --rule-line: #d0d0d0;
    --card-bg: #fff; --card-border: #e6e6e6;
    --tag-bg: #f4f4f4; --accent: #2c5aa0;
  }}
  html {{ scroll-behavior: smooth; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI",
                 system-ui, sans-serif;
    background: var(--bg); color: var(--fg);
    max-width: 980px; margin: 0 auto;
    padding: 28px 24px 80px; line-height: 1.55;
  }}
  h1 {{ margin: 0 0 4px 0; }}
  h2 {{
    margin-top: 2.2em; padding-bottom: 6px;
    border-bottom: 1px solid var(--rule-line);
    font-size: 22px;
  }}
  p.subtitle {{ margin: 0 0 24px 0; color: var(--muted); }}
  .step-num {{
    display: inline-block; margin-right: 8px;
    padding: 0 8px; background: var(--accent); color: white;
    border-radius: 999px; font-size: 13px; vertical-align: middle;
    font-weight: 700;
  }}
  nav.toc {{
    background: var(--card-bg); border: 1px solid var(--card-border);
    border-radius: 10px; padding: 12px 20px; margin: 12px 0 8px 0;
  }}
  nav.toc ol {{ margin: 6px 0 0 0; padding-left: 22px; }}
  nav.toc a {{ color: var(--accent); text-decoration: none; }}
  nav.toc a:hover {{ text-decoration: underline; }}
  pre {{
    background: #f6f6f6; border-radius: 6px;
    padding: 12px 14px; overflow-x: auto;
    border: 1px solid var(--card-border);
  }}
  code {{
    background: #f3f3f3; border-radius: 3px;
    padding: 1px 5px; font-size: 92%;
  }}
  pre code {{ background: none; padding: 0; }}
  blockquote {{
    margin: 8px 0; padding: 6px 14px;
    border-left: 4px solid var(--accent); color: var(--muted);
    background: var(--card-bg); border-radius: 4px;
  }}
  dl dt {{ margin-top: 10px; }}
  dl dt code {{ font-weight: 600; }}
  dl dd {{ margin-left: 22px; color: var(--muted); }}
  figure {{
    margin: 14px 0; padding: 14px;
    background: var(--card-bg);
    border: 1px solid var(--card-border);
    border-radius: 10px;
  }}
  figure img {{ width: 100%; height: auto; display: block; }}
  figcaption {{
    margin-top: 8px; color: var(--muted);
    font-size: 13px; text-align: center; font-style: italic;
  }}
  .fig-grid {{
    display: grid;
    grid-template-columns: 1fr;
    gap: 12px;
  }}
  @media (min-width: 720px) {{
    .fig-grid.two {{ grid-template-columns: 1fr 1fr; }}
    .fig-grid.three {{ grid-template-columns: 1fr 1fr 1fr; }}
  }}
</style>
</head><body>

<h1>Phase A — bigraphical algebra</h1>
<p class="subtitle">A walkthrough of <code>compose</code>, <code>tensor</code>, and the elementary bigraphs in <code>prism-schema</code>.</p>

<nav class="toc">
  <strong>Contents</strong>
  <ol>{toc}</ol>
</nav>

{body}

<hr style="margin-top:48px;border:none;border-top:1px solid var(--rule-line);" />
<p style="color:var(--muted);font-size:13px;text-align:center;">
  Generated by <code>cargo run --bin phase_a_tutorial</code>.
  Source: <code>examples/src/phase_a_tutorial.rs</code>.
  Implementation: <code>crates/prism-schema/src/assembly.rs</code>.
</p>

</body></html>
"##
    )
}

fn render_step(num: usize, s: &Step) -> String {
    let class = match s.figures.len() {
        0 => "",
        1 => "",
        2 => "two",
        _ => "three",
    };
    let figs: String = s
        .figures
        .iter()
        .map(|f| {
            format!(
                r#"<figure><img src="{}" alt="{}"><figcaption>{}</figcaption></figure>"#,
                html_escape(&f.file),
                html_escape(&f.caption),
                html_escape(&f.caption),
            )
        })
        .collect();

    let fig_block = if s.figures.is_empty() {
        String::new()
    } else if s.figures.len() == 1 {
        format!("{figs}")
    } else {
        format!(r#"<div class="fig-grid {class}">{figs}</div>"#)
    };

    format!(
        r#"
<section id="step-{num}">
<h2><span class="step-num">{num}</span>{heading}</h2>
{body}
{fig_block}
</section>
"#,
        num = num,
        heading = html_escape(s.heading),
        body = s.body,
        fig_block = fig_block,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
