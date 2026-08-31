<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->
# Contributing

Thank you for wanting to improve the Sculptor and the Gardener. This page is
deliberately one self-contained HTML file, and the bar for changes is honesty
rather than size — the whole point of the artifact is that nothing on it is
faked.

## The two invariants

Any change must preserve these, and the easiest way to fail review is to break
one while making something else look better:

1. **One definition, two pathways.** Both panes consume the *identical*
   deterministic voxel set produced by `generateSpecimen(seed)`. The Sculptor
   removes waste from a block that already contains the form; the Gardener
   adds the same form's voxels in growth order. If your change lets the two
   drift apart — even transiently — it is wrong, whatever it fixes.
2. **Instruments report engine state.** Ledgers read `InstancedMesh.count`;
   triangle counts read `renderer.info.render.triangles`; the in-page verifier
   re-derives its claims from the live buffers. Never write an expected number
   into a readout.

## Working on it

- Open `index.html` in any modern browser. No build step, no install. The only
  external script is Three.js 0.160.0 (pinned, cdnjs, UMD — later release
  lines dropped UMD builds, so do not bump it casually).
- The committed file carries **no doctype** — it is Artifact-shaped, and the
  Pages workflow prepends `<!doctype html>` at deploy time. CI enforces this.
- `window.llmProof` in the console exposes live counts and the verifier.
- Check `prefers-reduced-motion` behaviour: the page parks at a static
  mid-carve frame rather than animating. That is a feature.

## Licences

Two planes, enforced by CI on line 1 of every file:

- **Code** (`index.html`, workflows, `.a2ml` metadata): MPL-2.0.
- **Prose** (`.adoc`, `.md`): CC-BY-SA-4.0.

By contributing you agree your contribution is licensed under the plane it
lands in. Sign commits with `git commit -s` (DCO 1.1).

## Pull requests

Small and single-purpose beats large and mixed. Say what you verified and how
— a screenshot of the verifier line and the ledgers after your change is worth
more than prose. If you found a way to make the page *lie* (a state where a
readout disagrees with the engine), that is the most valuable issue you can
file, even without a fix.
