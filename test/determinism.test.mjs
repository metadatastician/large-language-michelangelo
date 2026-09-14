// SPDX-License-Identifier: MPL-2.0
// R1 — golden determinism + invariant suite for studio.html.
// Discharges ROADMAP P-7 ("byte-identical regeneration under test") as a fact.
// Runs under `node --test`. No browser, no Three.js, no DOM, no network.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { loadStudio, sha, invariant, captureGkey } from './harness.mjs';

const GOLDEN = new URL('./golden.json', import.meta.url);
const SEEDS = [1, 7, 4242];

const fingerprint = spec =>
  [sha(spec.type), sha(spec.col),
   sha(Int32Array.from(spec.treeOrder)), sha(Int32Array.from(spec.removeOrder))].join(':');

function compileAll(m){
  const out = {};
  for (const ex of m.EXAMPLES){
    const r = m.compileRecipe(ex.json);
    assert.ok(r.ok, `example "${ex.name}" must compile: ${r.errors.join('; ')}`);
    out[ex.name] = r;
  }
  return out;
}

test('the two pure regions are genuinely pure', () => {
  const m = loadStudio();           // loadStudio throws if THREE./document./window. appear
  assert.ok(m._regions.engine[1] > m._regions.engine[0]);
  assert.ok(m._regions.recipe[1] > m._regions.recipe[0]);
});

test('the shared engine region is byte-identical in both pages', () => {
  const grab = f => {
    const L = fs.readFileSync(new URL(`../${f}`, import.meta.url), 'utf8').split('\n');
    const a = L.findIndex(l => /===== BEGIN SHARED ENGINE =====/.test(l));
    const b = L.findIndex(l => /===== END SHARED ENGINE =====/.test(l));
    assert.ok(a >= 0 && b > a, `${f} must carry both shared-engine markers`);
    return L.slice(a, b + 1).join('\n');
  };
  assert.equal(grab('index.html'), grab('studio.html'),
    'index.html and studio.html shared-engine regions have drifted');
});

test('grid is the documented 27x27x40 column', () => {
  const m = loadStudio();
  assert.deepEqual([m.NX, m.NZ, m.NY], [27, 27, 40]);
  assert.equal(m.NCELL, 29160);
});

test('every example compiles, partitions the block, and converges', () => {
  const m = loadStudio();
  for (const [name, r] of Object.entries(compileAll(m))){
    m.FORMS.studio = r.form;
    const spec = m.generateSpecimen(7, 'studio');
    const inv = invariant(m, spec);
    assert.ok(inv.disjoint,   `${name}: form and waste overlap (${inv.overlap} cells, ${inv.dupes} dupes)`);
    assert.ok(inv.complete,   `${name}: ${inv.uncovered} cells belong to neither pathway`);
    assert.ok(inv.convergent, `${name}: ${inv.mismatch} cells disagree with the type buffer`);
    assert.ok(!inv.degenerate,`${name}: degenerate fill — the invariant passes trivially here`);
    assert.equal(captureGkey(m, r.form).nonFinite, 0, `${name}: non-finite growth key`);
    const frac = r.stats.fraction;
    assert.ok(frac > 0.01 && frac < 0.35, `${name}: fill ${(frac*100).toFixed(2)}% outside the readable band`);
  }
});

test('P-7: regeneration is byte-identical across independent evaluations', () => {
  const rounds = [0,1,2].map(() => {
    const m = loadStudio();
    const acc = {};
    for (const [name, r] of Object.entries(compileAll(m))){
      m.FORMS.studio = r.form;
      for (const seed of SEEDS) acc[`${name}|${seed}`] = fingerprint(m.generateSpecimen(seed, 'studio'));
    }
    return acc;
  });
  assert.deepEqual(rounds[1], rounds[0], 'round 1 drifted from round 0');
  assert.deepEqual(rounds[2], rounds[0], 'round 2 drifted from round 0');
  assert.equal(Object.keys(rounds[0]).length, 4 * SEEDS.length);
});

test('P-7 golden: fingerprints match the committed fixtures', () => {
  const m = loadStudio();
  const now = {};
  for (const [name, r] of Object.entries(compileAll(m))){
    m.FORMS.studio = r.form;
    for (const seed of SEEDS) now[`${name}|${seed}`] = fingerprint(m.generateSpecimen(seed, 'studio'));
  }
  if (process.env.UPDATE_GOLDEN === '1'){
    fs.writeFileSync(GOLDEN, JSON.stringify(now, null, 2) + '\n');
    return;
  }
  assert.ok(fs.existsSync(GOLDEN), 'golden.json missing — regenerate with UPDATE_GOLDEN=1');
  assert.deepEqual(now, JSON.parse(fs.readFileSync(GOLDEN, 'utf8')));
});

test('the seed moves the Sculptor trajectory but not the sculpture', () => {
  const m = loadStudio();
  const r = m.compileRecipe(m.EXAMPLES[0].json);
  m.FORMS.studio = r.form;
  const specs = SEEDS.map(s => m.generateSpecimen(s, 'studio'));
  assert.equal(new Set(specs.map(s => sha(s.type))).size, 1,
    'the final form must not depend on the seed');
  assert.equal(new Set(specs.map(s => sha(Int32Array.from(s.removeOrder)))).size, SEEDS.length,
    'the removal order must depend on the seed');
});

test('a revision diff is numerically consistent with the voxel counts', () => {
  const m = loadStudio();
  const base = structuredClone(m.EXAMPLES[0].json);
  const r1 = m.compileRecipe(base); m.FORMS.studio = r1.form;
  const s1 = m.generateSpecimen(7, 'studio');

  const rev = structuredClone(base);
  const w = rev.parts.find(p => /wing/i.test(p.id)) ?? rev.parts[1];
  w.h ? (w.h = [w.h[0]*1.35, w.h[1], w.h[2]]) : (w.r = [w.r[0]*1.35, w.r[1]]);
  const r2 = m.compileRecipe(rev); m.FORMS.studio = r2.form;
  const s2 = m.generateSpecimen(7, 'studio');

  const vd = m.voxelDiff(s1.type, s1.col, s2.type, s2.col);
  const rd = m.recipeDiff(base, rev);
  assert.deepEqual(rd.addedIds, [], 'no part was added');
  assert.deepEqual(rd.removedIds, [], 'no part was removed');
  assert.equal(rd.changed.length, 1, 'exactly one part changed');
  assert.deepEqual(rd.changed[0].fields, ['h'], 'exactly one field changed');
  assert.equal(rd.untouched, base.parts.length - 1);
  assert.equal(vd.added - vd.removed, r2.stats.filled - r1.stats.filled,
    'voxel diff must reconcile with the change in filled count');
});

test('compiler rejects recipes it cannot build', () => {
  const m = loadStudio();
  for (const [label, input] of [
    ['empty string',      ''],
    ['unparseable JSON',  '{nope'],
    ['no parts key',      { v:1, noun:'x' }],
    ['empty parts array', { v:1, parts: [] }],
    ['only unknown shapes', { parts:[{ id:'a', k:'dodecahedron', p:[0,0,0] }] }],
  ]) assert.equal(m.compileRecipe(input).ok, false, `${label} must be rejected`);
});

test('compiler warns rather than failing on salvageable recipes', () => {
  const m = loadStudio();
  const warnsOn = input => {
    const r = m.compileRecipe(input);
    assert.ok(r.ok, 'should still build');
    return r.warnings.join(' | ');
  };
  assert.match(warnsOn({ parts:[{ id:'good', k:'box', p:[0,0,0], h:[.4,.4,.4] },
                                { id:'bad',  k:'dodecahedron', p:[0,.5,0] }] }),
               /unknown shape/, 'an unknown shape must warn');
  assert.match(warnsOn({ parts:[{ id:'a', k:'box', p:[0,0,0], h:[.3,.3,.3], parent:'ghost' }] }),
               /unknown parent/, 'an unknown parent must warn');
  assert.match(warnsOn({ parts: Array.from({length:65}, (_,i) =>
                 ({ id:'p'+i, k:'box', p:[0, i/65-0.5, 0], h:[.2,.02,.2] })) }),
               /only the first 64/, 'exceeding the part cap must warn');
});
