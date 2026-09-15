// SPDX-License-Identifier: MPL-2.0
// Headless harness: link studio.html's pure engine prefix to its pure recipe layer.
// No browser, no Three.js, no DOM. See ROADMAP P-7.
import fs from 'node:fs';
import crypto from 'node:crypto';

const SRC = new URL('../studio.html', import.meta.url);

// Regions are located by marker//signature, never by hardcoded line numbers,
// so the harness cannot silently drift when the page is edited.
function region(lines, startRe, endRe, what){
  const a = lines.findIndex(l => startRe.test(l));
  if (a < 0) throw new Error('harness: could not find start of ' + what);
  const b = lines.findIndex((l, i) => i > a && endRe.test(l));
  if (b < 0) throw new Error('harness: could not find end of ' + what);
  return { from: a + 1, to: b, text: lines.slice(a, b).join('\n') };
}

export function loadStudio(){
  const lines = fs.readFileSync(SRC, 'utf8').split('\n');

  // pure engine: from the shared-engine marker to failAll, which is the page's
  // first DOM touch and therefore the exact boundary of the headless-testable core.
  const engine = region(lines,
    /^\s*\/\/ ===== BEGIN SHARED ENGINE =====/,
    /^\s*function failAll\b/, 'pure engine prefix');

  // recipe layer: from the compiler's constants to the first line after recipeDiff
  const recipe = region(lines,
    /^\s*var FIT_MARGIN\b/,
    /^\s*\/\/ ---------- page furniture ----------/, 'recipe layer');

  for (const [name, r] of [['engine', engine], ['recipe', recipe]]) {
    for (const bad of [/\bTHREE\./, /\bdocument\./, /\bwindow\./]) {
      if (bad.test(r.text)) throw new Error(`harness: ${name} region is not pure — matched ${bad}`);
    }
  }

  const src = `
    ${engine.text}
    var FORMS = {};
    ${recipe.text}
    return { FORMS, compileRecipe, generateSpecimen, EXAMPLES,
             NCELL, NX, NZ, NY, HX, HZ, HY, cellX, cellY, cellZ, voxelDiff, recipeDiff };
  `;
  const mod = new Function(src)();
  mod._regions = { engine: [engine.from, engine.to], recipe: [recipe.from, recipe.to] };
  return mod;
}

export const sha = buf =>
  crypto.createHash('sha256').update(Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength)).digest('hex');

// The partition claim, re-derived from buffers only (ROADMAP P-5).
// NOTE: generateSpecimen returns {type, col, treeOrder, removeOrder} and does NOT
// expose gkey — it is consumed internally by the sort that produces treeOrder. So the
// partition is checked against the two order arrays, and gkey is captured separately
// by invoking form.fill directly (see captureGkey).
export function invariant(m, spec){
  const { type, treeOrder, removeOrder } = spec;
  let form = 0, waste = 0, badType = 0;
  for (let i = 0; i < m.NCELL; i++) {
    const t = type[i];
    if (t === 0) waste++;
    else if (t === 1 || t === 2) form++;
    else badType++;
  }
  // disjointness and completeness of the two ORDERS, which is the claim the page makes
  const seen = new Uint8Array(m.NCELL);
  let dupes = 0, overlap = 0;
  for (const ci of treeOrder){ if (seen[ci]) dupes++; seen[ci] = 1; }
  for (const ci of removeOrder){ if (seen[ci] === 1) overlap++; if (seen[ci]) dupes++; seen[ci] = 2; }
  let uncovered = 0;
  for (let i = 0; i < m.NCELL; i++) if (!seen[i]) uncovered++;
  // convergence: the Gardener's build set is exactly the form set
  let mismatch = 0;
  for (const ci of treeOrder) if (type[ci] === 0) mismatch++;
  for (const ci of removeOrder) if (type[ci] !== 0) mismatch++;

  return {
    form, waste, badType,
    nTree: treeOrder.length, nWaste: removeOrder.length,
    disjoint: overlap === 0 && dupes === 0,
    complete: uncovered === 0 && treeOrder.length + removeOrder.length === m.NCELL,
    convergent: mismatch === 0,
    degenerate: form === 0 || waste === 0,   // the case the invariant CANNOT see
    overlap, dupes, uncovered, mismatch,
  };
}

// gkey never leaves generateSpecimen, so capture it at the form contract itself.
export function captureGkey(m, form, seed = 7){
  const type = new Uint8Array(m.NCELL);
  const col  = new Float32Array(m.NCELL * 3);
  const gkey = new Float32Array(m.NCELL);
  form.fill(seed, Math.random, () => 0, type, col, gkey);
  let nonFinite = 0;
  for (let i = 0; i < m.NCELL; i++) if (type[i] !== 0 && !Number.isFinite(gkey[i])) nonFinite++;
  return { type, col, gkey, nonFinite };
}
