# `ys-mode` — Emacs major mode for chrysalis (`.ys`)

Syntax highlighting + light indentation for chrysalis source.

## What it highlights

- **Definer keywords** (lowercase): `def`, `type`, `process`, `step`,
  `composite`, `reaction`, `pattern`, `extern`, `unit`, `context`,
  `contract`
- **Modifier keywords**: `from`, `import`, `using`, `fulfills`, `with`,
  `where`, `replace`; **control/logic**: `let`, `in`, `if`, `then`,
  `else`, `for`, `not`, `and`, `or`; **constants**: `true`, `false`
- **Declared names** — `Grow` in `process Grow`, `Cell` in
  `composite Cell`, the name after `def`, etc. — own face
  (`ys-declaration-face`), distinct from later references
- **Built-in types** (lowercase schema leaves): `float`, `int`,
  `string`, `bool`, `map`, `list`, `tree`, `any`, `array`, `maybe`
- **Built-in controls**: `BRS`, `RunProcess`
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

Covers the current kernel forms from `docs/chrysalis-design.md`
(definers incl. `def` / `contract`, native `from … import …`, contracts)
enough to make source files legible. Indentation is paren-depth-based,
not semantic.
