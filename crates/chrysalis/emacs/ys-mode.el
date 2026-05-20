;;; ys-mode.el --- Major mode for chrYSalis (.ys) source -*- lexical-binding: t; -*-

;; Author: prism contributors
;; Keywords: languages, chrysalis, bigraphs, simulation
;; Version: 0.1.0

;;; Commentary:

;; Syntax highlighting + light indentation for chrysalis source
;; files.  Chrysalis is a surface programming language whose semantics
;; compile to the prism process-bigraph Rust runtime.  See
;; `docs/chrysalis-design.md' in the prism repo for the language spec.
;;
;; Kernel forms recognised:
;;
;;   - Definers (lowercase): process, step, composite, reaction,
;;     pattern, let, if, then, else, where, in, replace, with, expr
;;   - Controls (Capitalised): highlighted via face for capitalised
;;     identifiers
;;   - Pattern variables: ?name
;;   - Link variables: ~name (as bare token)
;;   - Port bindings: ~{ ... } -> { ... }
;;   - Reaction arrow: =>
;;   - Single-quoted strings with {expr} interpolation
;;   - `#' line comments
;;   - Numbers (int, float)
;;
;; Installation: see `ys-mode.el' header in the project repo's
;; `crates/chrysalis/emacs/' for the snippet to drop into your
;; init.el.

;;; Code:

(defgroup ys nil
  "Major mode for editing chrysalis (.ys) source files."
  :group 'languages
  :prefix "ys-")

(defcustom ys-indent-offset 2
  "Indentation offset for `ys-mode'."
  :type 'integer
  :group 'ys)

;; ── Faces ───────────────────────────────────────────────────────────

(defface ys-pattern-var-face
  '((t :inherit font-lock-variable-name-face :weight bold))
  "Face for chrysalis pattern variables (`?name')."
  :group 'ys)

(defface ys-link-var-face
  '((t :inherit font-lock-builtin-face))
  "Face for chrysalis link variables (`~name')."
  :group 'ys)

(defface ys-port-arrow-face
  '((t :inherit font-lock-keyword-face :weight bold))
  "Face for port-direction markers `~{', `->{', `}', and the reaction arrow `=>'.
The closing `}' of a port-binding block is highlighted symmetrically
with the opening, so the brackets read as a single visual unit."
  :group 'ys)

(defface ys-declaration-face
  '((t :inherit font-lock-function-name-face :weight bold))
  "Face for the declared name in a definer header — the `Grow' in
`process Grow', the `Cell' in `composite Cell', etc.  Distinguished
from generic capitalised-identifier references."
  :group 'ys)

(defface ys-body-separator-face
  '((t :inherit font-lock-comment-face :weight bold))
  "Face for the trailing `|' body separator at the end of a statement
inside a body block — visually distinct from the mid-line `|' used
for parallel composition."
  :group 'ys)

;; ── Keyword sets ────────────────────────────────────────────────────

(defconst ys-definer-keywords
  '("process" "step" "composite" "reaction" "pattern" "expr"
    "let" "in" "if" "then" "else" "where" "replace" "with")
  "Lowercase definer keywords.")

(defconst ys-builtin-type-keywords
  '("Float" "Int" "Integer" "Bool" "Boolean" "String"
    "Map" "List" "Tree" "Maybe" "Tuple" "Array"
    "Any" "None"))

(defconst ys-builtin-control-keywords
  '("BRS" "Reaction" "Pattern" "ProcessDef" "StepDef"
    "Bridge" "Interface")
  "Built-in capitalised constructors.")

;; ── Syntax table ────────────────────────────────────────────────────

(defvar ys-mode-syntax-table
  (let ((st (make-syntax-table)))
    ;; '#' starts a line comment, newline ends it
    (modify-syntax-entry ?#  "<" st)
    (modify-syntax-entry ?\n ">" st)
    ;; Single-quoted strings
    (modify-syntax-entry ?\' "\"" st)
    ;; Double-quote also treated as string for safety
    (modify-syntax-entry ?\" "\"" st)
    ;; Identifier extensions — chrysalis idents allow ?
    (modify-syntax-entry ?_ "_" st)
    (modify-syntax-entry ?? "_" st)
    (modify-syntax-entry ?! "_" st)
    ;; Bracket types
    (modify-syntax-entry ?\( "()" st)
    (modify-syntax-entry ?\) ")(" st)
    (modify-syntax-entry ?\[ "(]" st)
    (modify-syntax-entry ?\] ")[" st)
    (modify-syntax-entry ?\{ "(}" st)
    (modify-syntax-entry ?\} "){" st)
    st)
  "Syntax table for `ys-mode'.")

;; ── Font-lock ───────────────────────────────────────────────────────

(defconst ys-font-lock-keywords
  `(
    ;; Definer header: `process Name`, `composite Name`, etc.  Done
    ;; first so the declared name gets its own face before the generic
    ;; capitalised-identifier fallback below.  `regexp-opt' wraps the
    ;; keyword alternation in a shy group `\(?:...\)' so our explicit
    ;; capture group below sits at position 1.
    (,(concat "\\<"
              (regexp-opt '("process" "step" "composite"
                            "reaction" "pattern" "expr"))
              "\\>\\s-+\\([A-Z][A-Za-z0-9_]*\\)")
     1 'ys-declaration-face)

    ;; Definer keywords
    (,(regexp-opt ys-definer-keywords 'symbols)
     . font-lock-keyword-face)

    ;; Built-in type names
    (,(regexp-opt ys-builtin-type-keywords 'symbols)
     . font-lock-type-face)

    ;; Built-in capitalised controls
    (,(regexp-opt ys-builtin-control-keywords 'symbols)
     . font-lock-builtin-face)

    ;; Reaction arrow `=>'
    ("\\(=>\\)" 1 'ys-port-arrow-face)

    ;; Port-binding block: opening `~{` or `->{` AND the matching `}'
    ;; — both highlighted symmetrically so the brackets read as one
    ;; unit.  `[^{}]*' means this doesn't handle braces nested inside
    ;; a port block (rare in chrysalis source).
    ("\\(~{\\|->{\\)[^{}]*\\(}\\)"
     (1 'ys-port-arrow-face)
     (2 'ys-port-arrow-face))

    ;; Trailing `|' body separator — `|' at end-of-line, optionally
    ;; followed by whitespace and/or a trailing `#' comment.
    ;; Distinguished from mid-line `|' (parallel composition) which
    ;; keeps the default face.
    ("\\(|\\)\\s-*\\(?:#.*\\)?$" 1 'ys-body-separator-face)

    ;; Pattern variable: ?name
    ("\\(\\?[A-Za-z_][A-Za-z0-9_]*\\)" 1 'ys-pattern-var-face)

    ;; Link variable: ~name (must not be ~{ — port marker already handled above)
    ("\\(~[A-Za-z_][A-Za-z0-9_]*\\)" 1 'ys-link-var-face)

    ;; Unbound port `!'
    ("\\(!\\)" 1 font-lock-warning-face)

    ;; Self / parent path tokens
    ("\\([@^]\\)" 1 font-lock-builtin-face)

    ;; Numbers (int and float)
    ("\\b\\(-?[0-9]+\\(\\.[0-9]+\\)?\\)\\b"
     1 font-lock-constant-face)

    ;; Capitalised identifiers as controls — generic fallback
    ("\\b\\([A-Z][A-Za-z0-9_]*\\)\\b"
     1 font-lock-type-face))
  "Font-lock highlighting for `ys-mode'.")

;; ── Indentation (light) ─────────────────────────────────────────────
;;
;; Quick heuristic — increase indent inside open parens/brackets/braces,
;; decrease on closing.  Not semantic; good enough for tier-1 source.

(defun ys-indent-line ()
  "Indent current line for `ys-mode'."
  (interactive)
  (let ((indent
         (save-excursion
           (beginning-of-line)
           (let ((paren-depth
                  (save-excursion
                    (or (ignore-errors
                          (car (syntax-ppss
                                (line-beginning-position))))
                        0))))
             (* (max 0 paren-depth) ys-indent-offset)))))
    (if (looking-at "^\\s-*[])}]")
        ;; Closing bracket on its own line — dedent.
        (setq indent (max 0 (- indent ys-indent-offset))))
    (save-excursion
      (beginning-of-line)
      (delete-horizontal-space)
      (indent-to indent))
    (when (looking-at "\\s-*$")
      (end-of-line))))

;; ── Mode definition ─────────────────────────────────────────────────

;;;###autoload
(define-derived-mode ys-mode prog-mode "chrYSalis"
  "Major mode for editing chrysalis (.ys) source.

\\{ys-mode-map}"
  :group 'ys
  :syntax-table ys-mode-syntax-table
  (setq-local font-lock-defaults '(ys-font-lock-keywords))
  (setq-local comment-start "# ")
  (setq-local comment-end "")
  (setq-local comment-start-skip "#+\\s-*")
  (setq-local indent-line-function #'ys-indent-line)
  (setq-local indent-tabs-mode nil))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.ys\\'" . ys-mode))

(provide 'ys-mode)

;;; ys-mode.el ends here
