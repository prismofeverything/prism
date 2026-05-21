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
    ;; Single quotes delimit strings, but ONLY when balanced on a line —
    ;; the string fences are applied by `ys--syntax-propertize' below.
    ;; Default to punctuation so a lone apostrophe (common in prose
    ;; comments, or the instant before you type a closing quote) cannot
    ;; flip the rest of the buffer into string syntax.
    (modify-syntax-entry ?\' "." st)
    ;; Double quotes are not string delimiters in chrysalis (strings are
    ;; single-quoted); keep them inert so a stray `"' in a comment is
    ;; harmless rather than the start of a buffer-spanning string.
    (modify-syntax-entry ?\" "." st)
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

;; ── Syntactic string fences ─────────────────────────────────────────
;;
;; chrysalis strings are single-quoted (`'0'`, `'{id}_0'`).  Making `''
;; a global string delimiter is a foot-gun: any unbalanced apostrophe —
;; a possessive in a comment, or just the instant before you type the
;; closing quote — flips the whole tail of the buffer into string syntax
;; and forces a re-parse + re-fontify of that tail on every keystroke.
;; Instead we mark only a *balanced* single-quoted run on a single line
;; as a string, via generic string fences (syntax class `|').  An
;; unpaired `'' matches nothing here and stays inert punctuation.

(defconst ys--syntax-propertize
  (syntax-propertize-rules
   ("\\('\\)[^'\n]*\\('\\)" (1 "|") (2 "|")))
  "Apply string-fence syntax to balanced single-quoted runs in `ys-mode'.")

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
  "Indent the current line for `ys-mode'.

One `ys-indent-offset' per open bracket at the start of the line; a line
that itself begins with a closing bracket is dedented one level so it
aligns with its opener.  Uses `indent-line-to', which rewrites the
leading whitespace only when it differs from the target — so the
re-indent that `electric-indent-mode' performs on every newline does not
dirty an already-correct line (and thus does not trigger a refontify)."
  (interactive)
  (let* ((depth (max 0 (car (save-excursion
                              (syntax-ppss (line-beginning-position))))))
         (closing (save-excursion
                    (back-to-indentation)
                    (looking-at-p "[]})]")))
         (target (* (max 0 (- depth (if closing 1 0))) ys-indent-offset)))
    ;; If point sits in the leading whitespace, leave it at the new
    ;; indentation (the usual newline / TAB feel); otherwise keep it put.
    (if (<= (current-column) (current-indentation))
        (indent-line-to target)
      (save-excursion (indent-line-to target)))))

;; ── Mode definition ─────────────────────────────────────────────────

;;;###autoload
(define-derived-mode ys-mode prog-mode "chrYSalis"
  "Major mode for editing chrysalis (.ys) source.

\\{ys-mode-map}"
  :group 'ys
  :syntax-table ys-mode-syntax-table
  (setq-local font-lock-defaults '(ys-font-lock-keywords))
  ;; Port blocks (`~{ … }`, `->{ … }`) and the trailing `|' separator are
  ;; matched by patterns that can cross a line boundary.  Mark such
  ;; matches so jit-lock extends the refontified region to the whole
  ;; match on edit, instead of repainting one line and leaving a
  ;; contextual pass to fix up the stale highlight afterwards.
  (setq-local font-lock-multiline t)
  (setq-local syntax-propertize-function ys--syntax-propertize)
  (setq-local comment-start "# ")
  (setq-local comment-end "")
  (setq-local comment-start-skip "#+\\s-*")
  (setq-local indent-line-function #'ys-indent-line)
  ;; Indentation is a paren-depth heuristic, and chrysalis source
  ;; hand-aligns multiline continuations (port blocks, daughter maps)
  ;; that the heuristic doesn't reproduce.  Inhibit `electric-indent's
  ;; reindent of the line being left on RET so it only indents the new
  ;; line — otherwise every newline re-flows, and often un-aligns, the
  ;; line above.
  (setq-local electric-indent-inhibit t)
  (setq-local indent-tabs-mode nil))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.ys\\'" . ys-mode))

(provide 'ys-mode)

;;; ys-mode.el ends here
