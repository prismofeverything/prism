# `ys-mode` — Emacs major mode for chrysalis (`.ys`)

Syntax highlighting + light indentation for chrysalis source.

## What it highlights

- **Definer keywords** (lowercase): `process`, `step`, `composite`,
  `reaction`, `pattern`, `let`, `if`, `then`, `else`, `where`, `in`,
  `replace`, `with`, `expr`
- **Declared names** — `Grow` in `process Grow`, `Cell` in
  `composite Cell`, etc. — own face (`ys-declaration-face`),
  distinct from references to those names later in the file
- **Built-in types**: `Float`, `Int`, `Bool`, `String`, `Map`,
  `List`, `Tree`, `Maybe`, `Tuple`, `Array`, `Any`
- **Built-in controls**: `BRS`, `Reaction`, `Pattern`, `Bridge`,
  `Interface`
- **Pattern variables**: `?name` (bold variable-name face)
- **Link variables**: `~name` (builtin face)
- **Port-binding blocks** — `~{ … }` and `->{ … }` —
  opening AND closing brace highlighted symmetrically with the
  port-arrow face
- **Reaction arrow** `=>` — port-arrow face
- **Trailing body separator** — `|` at end of a line gets its own
  face (`ys-body-separator-face`), distinct from mid-line `|`
  which is parallel composition
- **Unbound port** `!` (warning face)
- **Path tokens** `@` and `^` (builtin face)
- **Capitalised identifiers** — generic type face
- **Comments** — `# ...` to end of line
- **Single-quoted strings** with `{expr}` interpolation literals
- **Numbers** — constant face

## Installation

These snippets take one per-machine knob — where prism is checked out
— and derive every path from it. Set it once in your init:

```elisp
(defvar prism-root (expand-file-name "~/code/prism")
  "Absolute path to the prism checkout on this machine.")

(add-to-list 'load-path (expand-file-name "crates/chrysalis/emacs" prism-root))
```

Then load the mode. A plain `require` is enough — the file's top-level
`auto-mode-alist` entry hooks `.ys` buffers automatically:

```elisp
(require 'ys-mode)
```

Or, equivalently, via `use-package` (the `add-to-list` line above must
run first, so the package is on `load-path`):

```elisp
(use-package ys-mode
  :mode ("\\.ys\\'" . ys-mode))
```

### Try it without touching your init

In a running Emacs:

```elisp
M-x load-file RET ~/code/prism/crates/chrysalis/emacs/ys-mode.el RET
M-x ys-mode  ; in a .ys buffer
```

## Customisation

Indent width:

```elisp
(setq ys-indent-offset 4)  ; default is 2
```

Five custom faces are defined for theming:

```elisp
(set-face-attribute 'ys-pattern-var-face     nil :foreground "tomato")
(set-face-attribute 'ys-link-var-face        nil :foreground "deepskyblue")
(set-face-attribute 'ys-port-arrow-face      nil :foreground "orchid")
(set-face-attribute 'ys-declaration-face     nil :foreground "gold1" :weight 'bold)
(set-face-attribute 'ys-body-separator-face  nil :foreground "gray50")
```

## Status

Tier-1 quick mode — covers the kernel forms from
`docs/chrysalis-design.md` enough to make source files legible.
Indentation is paren-depth-based, not semantic; will get smarter when
the chrysalis parser lands.
