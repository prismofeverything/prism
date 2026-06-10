#!/usr/bin/env bash
#
# undo-71.sh (Option 2) — move #71 into your MAIN repo as UNSTAGED changes,
# undo my merge on `icon`, and remove the worktree — so you recommit in your
# normal flow (no worktree, no separate merge step). Pushes NOTHING.
#
# >>> Make your backup first, then read this through and run it.
# >>> I am NOT running it — every git write below is yours to execute.
#
set -eu   # POSIX-clean: works under sh/dash or bash (no `pipefail`; the only pipes
          # here are informational `--stat | tail`, and every critical step is unpiped)

MAIN=/home/youdonotexist/code/prism        # your repo, on branch: icon
WT=/home/youdonotexist/code/prism-units     # the worktree, on branch: units-in-schema
BASE=dba6f6f                                # icon's tip just BEFORE my #71 merge
TIP=0f66231                                 # branch tip = #71 + the coord doctest fix
PATCH=/tmp/units-in-schema-71.patch

echo "## sanity checks"
git -C "$WT"   cat-file -e "${TIP}^{commit}"  || { echo "ABORT: branch tip $TIP gone"; exit 1; }
git -C "$MAIN" cat-file -e "${BASE}^{commit}" || { echo "ABORT: base $BASE gone";      exit 1; }
git -C "$MAIN" merge-base --is-ancestor "$BASE" HEAD \
  || { echo "ABORT: $BASE is not an ancestor of icon HEAD — state moved, stop + ping me"; exit 1; }

echo "## 1. capture #71 as a patch (its 19-file delta over icon's tip; conflicts already resolved)"
git -C "$WT" diff "$BASE" "$TIP" > "$PATCH"
git -C "$WT" diff "$BASE" "$TIP" --stat | tail -1

echo "## 2. undo my merge on icon — stash unify's in-flight work first so it survives"
POP=0
if [ -n "$(git -C "$MAIN" status --porcelain)" ]; then
  git -C "$MAIN" stash push -u -m "in-flight, preserved across #71 undo"
  POP=1
fi
git -C "$MAIN" reset --hard "$BASE"
if [ "$POP" = 1 ]; then
  git -C "$MAIN" stash pop          # if this conflicts, the script stops here — ping me
fi

echo "## 3. drop #71 into the main tree as UNSTAGED changes"
git -C "$MAIN" apply --3way "$PATCH"

echo "## 4. remove the worktree (the units-in-schema branch is KEPT as a safety ref)"
git -C "$MAIN" worktree remove "$WT"

cat <<EOF

DONE.
  - Your main repo ($MAIN) is on 'icon' at $BASE, with #71 + unify's in-flight
    work now UNSTAGED. The worktree is gone.
  - Review:   cd $MAIN && git status     (and: git diff)
  - You're already ON icon, so committing the #71 files lands them directly —
    no separate merge needed. All of #71 lives under crates/, e.g.:
        git add crates/
        git commit -m "#71 units-in-schema: carry Dimension in the schema (Axis-A units half)"
    (unify's work is in coord/ + docs/, so 'git add crates/' won't touch it.)
    NB: that patch also carries a 1-line coord.rs doctest fix — 'git add' it
    separately first if you want it as its own commit.
  - Safety net: the old commits still live on branch 'units-in-schema'
    (delete when happy:  git -C $MAIN branch -D units-in-schema).
  - Optional: reclaim the isolated build cache:  rm -rf /mnt/data/archive/prism-units-target
EOF
