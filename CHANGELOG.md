<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# Changelog

Notable changes, in [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
form. Dates are release-to-main dates.

## [0.4.0] — 2026-09-14

### Added

- `studio.html` (roadmap §3 v0.2 landed): prompt-driven forms. Claude compiles
  a prompt into a compact JSON *recipe*; the page rasterises it deterministically
  into the same 29,160-cell block, and both pathways build it unchanged. The
  recipe — not the prompt — is the real interface: a JSON editor and an example
  library work fully offline, and the prompt box appears only where sampling is
  available. `index.html` is untouched as the clean demonstrator.
- Edit-and-diff revision: a follow-up prompt edits the retained recipe, and the
  page reports exactly what changed at both levels — which parts and fields, and
  how many voxels were added, removed and recoloured.
- Golden determinism suite (`test/determinism.test.mjs`, `test/golden.json`):
  12 fingerprints over the engine core, run headlessly under `node --test` with
  no Three.js. This is the first test in the repository and it discharges P-7's
  evidence clause — see ROADMAP §P.

### Changed

- `index.html` split into a marked shared-engine region and a page-specific
  tail, so `studio.html` can carry a byte-identical copy of the engine. The
  duplication is deliberate: the Artifact sandbox cannot load a local `.js`.
- CI gates widened from the literal `index.html` to every root `*.html`, so a
  new page can no longer be added ungated. Each widened gate asserts it matched
  at least two pages, so an empty glob fails instead of passing vacuously.
- `pages.yml` assembles every committed page rather than only `index.html`.

### Added (CI)

- Shared-engine drift gate: the marked region of every page is hashed and the
  build fails unless all pages agree. The gate asserts the region is at least
  100 lines and that at least two regions were compared, so moved markers fail
  rather than silently disarming it.
- A second CI job runs the determinism suite on every push.

## [0.3.0] — 2026-08-31

### Added

- Forms picker (roadmap §2 shipped): tree, torus knot, letter M, chess rook,
  castle — a `FORMS` registry where each form writes only its own cells;
  marble, BFS carve ordering, count-scrub and instruments are shared.
- In-page verifier: `llmProof.verify()` re-derives disjointness, completeness
  and convergence from the live buffers; the verdict renders under the
  timeline on every build.
- Per-form colour bootstrap: the Gardener's 1-bit threshold and 4-colour
  palette are luminance quartiles of the chosen form, not tree constants.
- Headless CI-adjacent gate: all forms × seeds checked for partition,
  convergence and byte-identical regeneration under bun.

### Changed

- Pane epigraphs now use Michelangelo's own Italian — *per forza di levare* /
  *per via di porre* (1549 letter to Benedetto Varchi) — replacing the
  uncoined Latin *per demptionem*/*per additionem*.
- Labels neutralised from tree to form throughout (formulas, instruments,
  invariant line, canvas aria-labels are per-form and dynamic).
- Reduced-motion now re-parks at the static mid-carve frame on form change
  and respecimen, instead of jumping to an empty t=0.

## [0.2.0] — 2026-08-31

### Added

- Maths-in-motion instruments: per-pane render-algebra readouts
  (`draw(marble[0…N−k]) ∪ draw(form[ALL])` vs `draw(form[0…k])`),
  engine-reported triangle counts, and append-only operation tapes.
- "Not a trick" honesty section naming exactly how the effect is achieved.
- Read-only live counts at `window.llmProof` for console sceptics.
- `ROADMAP.adoc`.

## [0.1.0] — 2026-08-31

### Added

- The Sculptor and the Gardener: one deterministic voxel tree (29,160-cell
  block; 2,639 form + 26,521 waste), carved by removal only and grown by
  addition only on one shared timeline. X-ray toggle, falling chips and
  rubble, colour-depth bootstrap, seeded respecimen, reduced-motion static
  frame.
