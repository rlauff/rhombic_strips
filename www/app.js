// Rhombic Strips — browser frontend.
// The editor state lives here; combinatorics (generators, lattice files,
// strip search) is Rust compiled to wasm. Long-running searches run in
// worker.js; this file mirrors the LatticeApp in gui.rs.

import init, {
  poset_ranks,
  to_lattice_file,
  from_lattice_file,
  gen_grid,
  gen_cube,
  gen_simplex,
  infer_digit_relations,
  gen_distributive,
  gen_graph,
  gen_tube_poset,
  gen_graph_associahedron,
} from './pkg/rhombic_strips.js';

// ---------------------------------------------------------------------------
// Constants & palette (Okabe-Ito, as in the papers; vermillion/sky as gui.rs)
// ---------------------------------------------------------------------------

const NODE_R = 18;
const X_STEP = 90, Y_STEP = 90, STRIP_Y_STEP = 100;
const INK = '#23262e';
const FAINT = '#c9c5b8';
const VERMILLION = '#e66100';
const SKY = '#56b4e9';
const LAYER_COLORS = [
  '#0072b2', '#e69f00', '#009e73', '#cc79a7',
  '#56b4e9', '#d55e00', '#8a7d1f', '#6c62c9',
];

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const state = {
  mode: 'poset',           // 'poset' | 'graph'
  nodes: [],               // {id, label, x, y}
  edges: [],               // [idA, idB]; poset: (lower, upper)
  nextId: 0,
  undoStack: [],

  view: { scale: 1, ox: 0, oy: 0 },

  edgeStart: null,         // node id
  hovered: null,           // node id

  // computation
  worker: null,
  job: null,               // {kind, started, liveCount}
  strips: [],              // {layers:[[id]], edges:[[id,id]], cyclicEdges:[[id,id]]}
  cursor: 0,
  totalStrips: null,
  viewing: false,
};

// ---------------------------------------------------------------------------
// DOM
// ---------------------------------------------------------------------------

const $ = (id) => document.getElementById(id);
const canvas = $('canvas');
const ctx = canvas.getContext('2d');
const logEl = $('log');
const statsEl = $('stats');
const renameInput = $('rename-input');

function log(msg, isError = false) {
  logEl.textContent = msg;
  logEl.classList.toggle('error', isError);
}

function updateStats() {
  const kind = state.mode === 'poset' ? 'relations' : 'edges';
  statsEl.textContent = `${state.nodes.length} nodes · ${state.edges.length} ${kind}`;
}

// ---------------------------------------------------------------------------
// Model helpers
// ---------------------------------------------------------------------------

const nodeById = (id) => state.nodes.find((n) => n.id === id);
const labelOf = (id) => nodeById(id)?.label ?? '?';

function addNode(label, x, y) {
  const id = state.nextId++;
  state.nodes.push({ id, label: label || String(id), x, y });
  return id;
}

function removeNode(id) {
  state.nodes = state.nodes.filter((n) => n.id !== id);
  state.edges = state.edges.filter(([a, b]) => a !== id && b !== id);
}

/// Toggle relation/edge (a, b); returns true if added.
function toggleEdge(a, b) {
  const i = state.edges.findIndex(
    ([x, y]) => (x === a && y === b) || (x === b && y === a)
  );
  if (i >= 0) {
    state.edges.splice(i, 1);
    return false;
  }
  state.edges.push([a, b]);
  return true;
}

/// Wire format for wasm: labels + index edges, plus id map (index -> id).
function toWire() {
  const idx = new Map(state.nodes.map((n, i) => [n.id, i]));
  return {
    wire: {
      labels: state.nodes.map((n) => n.label),
      edges: state.edges
        .filter(([a, b]) => idx.has(a) && idx.has(b))
        .map(([a, b]) => [idx.get(a), idx.get(b)]),
    },
    idMap: state.nodes.map((n) => n.id),
  };
}

// ---------------------------------------------------------------------------
// Undo & structural changes
// ---------------------------------------------------------------------------

function pushUndo() {
  state.undoStack.push({
    mode: state.mode,
    nextId: state.nextId,
    nodes: state.nodes.map((n) => ({ ...n })),
    edges: state.edges.map((e) => [...e]),
  });
  if (state.undoStack.length > 50) state.undoStack.shift();
}

function undo() {
  const s = state.undoStack.pop();
  if (!s) {
    log('Nothing to undo.');
    return;
  }
  invalidateResults();
  setMode(s.mode);
  state.nodes = s.nodes;
  state.edges = s.edges;
  state.nextId = s.nextId;
  log('Undone.');
  refresh();
}

/// Any structural change invalidates running jobs and cached strips.
function invalidateResults() {
  if (state.worker && state.job) state.worker.postMessage({ cmd: 'cancel' });
  stopTicker();
  state.job = null;
  state.strips = [];
  state.cursor = 0;
  state.totalStrips = null;
  state.viewing = false;
  state.edgeStart = null;
  updateJobUi();
}

function structuralChange() {
  pushUndo();
  invalidateResults();
}

/// Replace the whole diagram (generators, load).
function replaceGraph(wire, mode, msg, layout = 'rank') {
  structuralChange();
  setMode(mode);
  state.nodes = [];
  state.edges = [];
  state.nextId = 0;
  const ids = wire.labels.map((l) => addNode(l, 0, 0));
  for (const [a, b] of wire.edges) state.edges.push([ids[a], ids[b]]);
  if (layout === 'rank' && wire.ranks) layoutByRank(wire.ranks);
  else if (layout === 'line') layoutLine();
  else layoutCircle();
  fitView();
  log(msg);
  refresh();
}

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

function layoutByRank(ranks) {
  // ranks in node order; rows centered, higher rank higher on screen
  const rows = new Map();
  state.nodes.forEach((n, i) => {
    const r = ranks[i];
    if (!rows.has(r)) rows.set(r, []);
    rows.get(r).push(n);
  });
  for (const [r, row] of rows) {
    const w = (row.length - 1) * X_STEP;
    row.forEach((n, k) => {
      n.x = -w / 2 + k * X_STEP;
      n.y = -r * Y_STEP;
    });
  }
}

function layoutCircle() {
  const n = state.nodes.length;
  if (n === 0) return;
  const radius = 40 + 18 * n;
  state.nodes.forEach((node, i) => {
    const a = (Math.PI * 2 * i) / n - Math.PI / 2;
    node.x = radius * Math.cos(a);
    node.y = radius * Math.sin(a);
  });
}

function layoutLine() {
  const n = state.nodes.length;
  state.nodes.forEach((node, i) => {
    node.x = i * 80 - (n - 1) * 40;
    node.y = 0;
  });
}

function arrangeByRank() {
  const { wire } = toWire();
  try {
    const ranks = JSON.parse(poset_ranks(JSON.stringify(wire)));
    layoutByRank(ranks);
    fitView();
    refresh();
  } catch (e) {
    log(String(e), true);
  }
}

/// Layered layout of the displayed strip (like gui.rs arrange_as_strip).
function arrangeAsStrip(idx) {
  const view = state.strips[idx];
  if (!view) return;
  view.layers.forEach((layer, li) => {
    layer.forEach((id, i) => {
      const n = nodeById(id);
      if (!n) return;
      n.x = (i - (layer.length - 1) / 2) * X_STEP;
      n.y = -li * STRIP_Y_STEP;
    });
  });
  fitView();
}

// ---------------------------------------------------------------------------
// Viewport
// ---------------------------------------------------------------------------

const toScreen = (x, y) => [
  x * state.view.scale + state.view.ox,
  y * state.view.scale + state.view.oy,
];
const toWorld = (sx, sy) => [
  (sx - state.view.ox) / state.view.scale,
  (sy - state.view.oy) / state.view.scale,
];

function canvasSize() {
  const r = canvas.getBoundingClientRect();
  return [r.width, r.height];
}

function fitView() {
  if (state.nodes.length === 0) return;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const n of state.nodes) {
    minX = Math.min(minX, n.x); maxX = Math.max(maxX, n.x);
    minY = Math.min(minY, n.y); maxY = Math.max(maxY, n.y);
  }
  const [w, h] = canvasSize();
  const bw = Math.max(maxX - minX, 1), bh = Math.max(maxY - minY, 1);
  const margin = 120;
  state.view.scale = Math.min(
    Math.max(Math.min((w - margin) / bw, (h - margin) / bh), 0.05),
    2.5
  );
  state.view.ox = w / 2 - ((minX + maxX) / 2) * state.view.scale;
  state.view.oy = h / 2 - ((minY + maxY) / 2) * state.view.scale;
}

function zoomAt(sx, sy, factor) {
  const [wx, wy] = toWorld(sx, sy);
  state.view.scale = Math.min(Math.max(state.view.scale * factor, 0.05), 10);
  state.view.ox = sx - wx * state.view.scale;
  state.view.oy = sy - wy * state.view.scale;
  refresh();
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function resizeCanvas() {
  const dpr = window.devicePixelRatio || 1;
  const [w, h] = canvasSize();
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  render();
}

function currentStrip() {
  return state.viewing ? state.strips[state.cursor] : null;
}

function render() {
  const [w, h] = canvasSize();
  ctx.clearRect(0, 0, w, h);

  // dotted worksheet grid, in world coordinates
  const s = state.view.scale;
  const step = X_STEP * s >= 24 ? X_STEP : X_STEP * 4;
  const [x0, y0] = toWorld(0, 0);
  const [x1, y1] = toWorld(w, h);
  ctx.fillStyle = '#d9d5c9';
  for (let gx = Math.floor(x0 / step) * step; gx <= x1; gx += step) {
    for (let gy = Math.floor(y0 / step) * step; gy <= y1; gy += step) {
      const [px, py] = toScreen(gx, gy);
      ctx.fillRect(px - 1, py - 1, 2, 2);
    }
  }

  const strip = currentStrip();
  const inStrip = new Map(); // id -> layer index
  if (strip) {
    strip.layers.forEach((layer, li) =>
      layer.forEach((id) => inStrip.set(id, li))
    );
  }

  // base edges
  for (const [a, b] of state.edges) {
    const na = nodeById(a), nb = nodeById(b);
    if (!na || !nb) continue;
    const hovered =
      !strip && (state.hovered === a || state.hovered === b);
    ctx.strokeStyle = hovered ? SKY : strip ? FAINT : INK;
    ctx.lineWidth = hovered ? 2.5 : 1.5;
    line(na, nb);
  }

  // strip overlay edges
  if (strip) {
    ctx.strokeStyle = VERMILLION;
    ctx.lineWidth = 3;
    for (const [a, b] of strip.edges) {
      const na = nodeById(a), nb = nodeById(b);
      if (na && nb) line(na, nb);
    }
    ctx.setLineDash([7, 5]);
    for (const [a, b] of strip.cyclicEdges) {
      const na = nodeById(a), nb = nodeById(b);
      if (na && nb) line(na, nb);
    }
    ctx.setLineDash([]);
  }

  // pending relation preview
  if (state.edgeStart != null && pointer.inside) {
    const na = nodeById(state.edgeStart);
    if (na) {
      const [ax, ay] = toScreen(na.x, na.y);
      ctx.strokeStyle = SKY;
      ctx.lineWidth = 1.5;
      ctx.setLineDash([5, 4]);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(pointer.sx, pointer.sy);
      ctx.stroke();
      ctx.setLineDash([]);
    }
  }

  // nodes
  const r = NODE_R * s;
  for (const n of state.nodes) {
    const [px, py] = toScreen(n.x, n.y);
    const layer = inStrip.get(n.id);
    const dimmed = strip && layer === undefined;

    ctx.beginPath();
    ctx.arc(px, py, r, 0, Math.PI * 2);
    ctx.fillStyle = dimmed ? '#efede6' : '#ffffff';
    ctx.fill();
    if (n.id === state.edgeStart) {
      ctx.strokeStyle = SKY;
      ctx.lineWidth = 3;
    } else if (layer !== undefined) {
      ctx.strokeStyle = LAYER_COLORS[layer % LAYER_COLORS.length];
      ctx.lineWidth = 3;
    } else if (n.id === state.hovered) {
      ctx.strokeStyle = SKY;
      ctx.lineWidth = 2.5;
    } else {
      ctx.strokeStyle = dimmed ? FAINT : INK;
      ctx.lineWidth = 1.5;
    }
    ctx.stroke();

    // label, shrunk to fit, middle-truncated beyond hope
    let text = n.label;
    let size = Math.min(13, (2.6 * r) / Math.max(text.length * 0.62, 1));
    if (size < 6.5 && text.length > 12) {
      text = text.slice(0, 5) + '…' + text.slice(-5);
      size = Math.min(13, (2.6 * r) / (text.length * 0.62));
    }
    if (size >= 3) {
      ctx.fillStyle = dimmed ? '#9a968a' : INK;
      ctx.font = `${size}px "IBM Plex Mono", monospace`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(text, px, py);
    }
  }

  // full label of the hovered node, as a floating caption
  if (state.hovered != null) {
    const n = nodeById(state.hovered);
    if (n && n.label.length > 12) {
      const [px, py] = toScreen(n.x, n.y);
      ctx.font = '12px "IBM Plex Mono", monospace';
      const tw = ctx.measureText(n.label).width;
      ctx.fillStyle = 'rgba(20, 22, 28, 0.92)';
      ctx.fillRect(px - tw / 2 - 7, py - r - 30, tw + 14, 22);
      ctx.fillStyle = '#e9e7e0';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(n.label, px, py - r - 19);
    }
  }

  function line(na, nb) {
    const [ax, ay] = toScreen(na.x, na.y);
    const [bx, by] = toScreen(nb.x, nb.y);
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.stroke();
  }
}

function refresh() {
  updateStats();
  updateStripUi();
  render();
}

// ---------------------------------------------------------------------------
// Pointer interaction: drag / pan / click-click relations / rename / delete
// ---------------------------------------------------------------------------

const pointer = { sx: 0, sy: 0, inside: false };
let drag = null; // {kind:'node'|'pan', id?, startSx, startSy, moved, ...}

function nodeAt(sx, sy) {
  const [wx, wy] = toWorld(sx, sy);
  const r = NODE_R;
  for (let i = state.nodes.length - 1; i >= 0; i--) {
    const n = state.nodes[i];
    if ((n.x - wx) ** 2 + (n.y - wy) ** 2 <= r * r) return n;
  }
  return null;
}

function localXY(e) {
  const r = canvas.getBoundingClientRect();
  return [e.clientX - r.left, e.clientY - r.top];
}

canvas.addEventListener('pointerdown', (e) => {
  if (e.button === 2) return; // context menu handles deletion
  finishRename(false);
  const [sx, sy] = localXY(e);
  const n = nodeAt(sx, sy);
  canvas.setPointerCapture(e.pointerId);
  if (n) {
    drag = {
      kind: 'node', id: n.id, startSx: sx, startSy: sy,
      origX: n.x, origY: n.y, moved: false, undoPushed: false,
    };
  } else {
    drag = {
      kind: 'pan', startSx: sx, startSy: sy,
      origOx: state.view.ox, origOy: state.view.oy, moved: false,
    };
  }
});

canvas.addEventListener('pointermove', (e) => {
  const [sx, sy] = localXY(e);
  pointer.sx = sx; pointer.sy = sy; pointer.inside = true;

  if (drag) {
    const dx = sx - drag.startSx, dy = sy - drag.startSy;
    if (Math.hypot(dx, dy) > 4) drag.moved = true;
    if (drag.kind === 'node' && drag.moved) {
      if (!drag.undoPushed) { pushUndo(); drag.undoPushed = true; }
      const n = nodeById(drag.id);
      if (n) {
        n.x = drag.origX + dx / state.view.scale;
        n.y = drag.origY + dy / state.view.scale;
      }
    } else if (drag.kind === 'pan' && drag.moved) {
      state.view.ox = drag.origOx + dx;
      state.view.oy = drag.origOy + dy;
    }
    render();
    return;
  }

  const hovered = nodeAt(sx, sy)?.id ?? null;
  if (hovered !== state.hovered) {
    state.hovered = hovered;
    render();
  }
});

canvas.addEventListener('pointerleave', () => {
  pointer.inside = false;
  if (state.hovered !== null) { state.hovered = null; render(); }
});

canvas.addEventListener('pointerup', (e) => {
  if (!drag) return;
  const wasClick = !drag.moved;
  const d = drag;
  drag = null;
  if (!wasClick) return;

  if (d.kind === 'node') {
    // click-click relation building
    if (state.edgeStart == null) {
      state.edgeStart = d.id;
      canvas.classList.add('linking');
      log(
        state.mode === 'poset'
          ? `Relation from “${labelOf(d.id)}” — click the covering node.`
          : `Edge from “${labelOf(d.id)}” — click the other endpoint.`
      );
    } else if (state.edgeStart === d.id) {
      cancelLink();
    } else {
      structuralChange();
      const added = toggleEdge(state.edgeStart, d.id);
      const verb = added ? 'Added' : 'Removed';
      const kind = state.mode === 'poset' ? 'relation' : 'edge';
      log(`${verb} ${kind} ${labelOf(state.edgeStart)} → ${labelOf(d.id)}.`);
      state.edgeStart = null;
      canvas.classList.remove('linking');
    }
  } else {
    cancelLink();
  }
  refresh();
});

canvas.addEventListener('dblclick', (e) => {
  const [sx, sy] = localXY(e);
  const n = nodeAt(sx, sy);
  if (n) {
    startRename(n.id);
  } else {
    structuralChange();
    const [wx, wy] = toWorld(sx, sy);
    const id = addNode($('label-input').value.trim(), wx, wy);
    $('label-input').value = '';
    log(`Added node ${labelOf(id)}.`);
    refresh();
  }
});

canvas.addEventListener('contextmenu', (e) => {
  e.preventDefault();
  const [sx, sy] = localXY(e);
  const n = nodeAt(sx, sy);
  if (n) {
    structuralChange();
    removeNode(n.id);
    log(`Deleted node “${n.label}”.`);
    refresh();
  } else {
    cancelLink();
    render();
  }
});

canvas.addEventListener('wheel', (e) => {
  e.preventDefault();
  const [sx, sy] = localXY(e);
  zoomAt(sx, sy, e.deltaY < 0 ? 1.1 : 1 / 1.1);
}, { passive: false });

function cancelLink() {
  state.edgeStart = null;
  canvas.classList.remove('linking');
}

// ---- rename ----------------------------------------------------------------

let renaming = null; // node id

function startRename(id) {
  const n = nodeById(id);
  if (!n) return;
  renaming = id;
  const [px, py] = toScreen(n.x, n.y);
  renameInput.value = n.label;
  renameInput.style.left = `${px}px`;
  renameInput.style.top = `${py}px`;
  renameInput.hidden = false;
  renameInput.focus();
  renameInput.select();
}

function finishRename(apply) {
  if (renaming == null) return;
  const n = nodeById(renaming);
  if (apply && n) {
    const label = renameInput.value.trim();
    if (label && label !== n.label) {
      structuralChange();
      n.label = label;
      log(`Renamed to “${label}”.`);
    }
  }
  renaming = null;
  renameInput.hidden = true;
  refresh();
}

renameInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') finishRename(true);
  if (e.key === 'Escape') finishRename(false);
});
renameInput.addEventListener('blur', () => finishRename(true));

// ---- global shortcuts --------------------------------------------------------

window.addEventListener('keydown', (e) => {
  if (e.target instanceof HTMLInputElement) return;
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
    e.preventDefault();
    undo();
  }
  if (e.key === 'Escape') {
    cancelLink();
    finishRename(false);
    render();
  }
});

// ---------------------------------------------------------------------------
// Mode switch
// ---------------------------------------------------------------------------

function setMode(mode) {
  if (state.mode !== mode) invalidateResults();
  state.mode = mode;
  $('mode-poset').classList.toggle('active', mode === 'poset');
  $('mode-graph').classList.toggle('active', mode === 'graph');
  $('mode-poset').setAttribute('aria-selected', mode === 'poset');
  $('mode-graph').setAttribute('aria-selected', mode === 'graph');
  document.querySelectorAll('[data-mode]').forEach((el) => {
    el.hidden = el.dataset.mode !== mode;
  });
  $('btn-arrange').textContent =
    mode === 'poset' ? 'Arrange by rank' : 'Circle layout';
  updateStats();
}

$('mode-poset').addEventListener('click', () => { setMode('poset'); refresh(); });
$('mode-graph').addEventListener('click', () => { setMode('graph'); refresh(); });

// ---------------------------------------------------------------------------
// Sidebar: edit
// ---------------------------------------------------------------------------

$('btn-add').addEventListener('click', () => {
  structuralChange();
  const [w, h] = canvasSize();
  const [wx, wy] = toWorld(w / 2, h / 2);
  const id = addNode($('label-input').value.trim(), wx, wy);
  $('label-input').value = '';
  log(`Added node ${labelOf(id)}.`);
  refresh();
});

$('btn-arrange').addEventListener('click', () => {
  if (state.mode === 'poset') arrangeByRank();
  else { layoutCircle(); fitView(); refresh(); }
});

$('btn-fit').addEventListener('click', () => { fitView(); render(); });
$('btn-undo').addEventListener('click', undo);
$('btn-clear').addEventListener('click', () => {
  if (state.nodes.length === 0) return;
  structuralChange();
  state.nodes = [];
  state.edges = [];
  log('Cleared.');
  refresh();
});

// ---------------------------------------------------------------------------
// Sidebar: examples & generators
// ---------------------------------------------------------------------------

const exampleN = () =>
  Math.min(12, Math.max(1, parseInt($('example-n').value, 10) || 3));

function runGen(fn, mode, describe, layout = 'rank') {
  try {
    const wire = JSON.parse(fn());
    replaceGraph(wire, mode, describe(wire), layout);
  } catch (e) {
    log(String(e), true);
  }
}

document.querySelectorAll('[data-gen]').forEach((btn) =>
  btn.addEventListener('click', () => {
    const n = exampleN();
    const kind = btn.dataset.gen;
    runGen(
      () => (kind === 'cube' ? gen_cube(n) : gen_simplex(n)),
      'poset',
      (w) => `${n}-${kind} face lattice: ${w.labels.length} faces.`
    );
  })
);

const ASSOC_NAMES = {
  complete: 'Permutahedron (tubings of K_n)',
  path: 'Associahedron (tubings of a path)',
  cycle: 'Cyclohedron (tubings of a cycle)',
  star: 'Stellahedron (tubings of a star)',
};

document.querySelectorAll('[data-assoc]').forEach((btn) =>
  btn.addEventListener('click', () => {
    const kind = btn.dataset.assoc;
    runGen(
      () => gen_graph_associahedron(gen_graph(kind, exampleN())),
      'poset',
      (w) => `${ASSOC_NAMES[kind]}: ${w.labels.length} faces.`
    );
  })
);

document.querySelectorAll('[data-graph]').forEach((btn) =>
  btn.addEventListener('click', () => {
    const kind = btn.dataset.graph;
    const n = exampleN();
    runGen(
      () => gen_graph(kind, n),
      'graph',
      () =>
        kind === 'complete' ? `K_${n}.`
        : kind === 'star' ? `Star K_(1,${Math.max(n - 1, 0)}).`
        : `${kind[0].toUpperCase() + kind.slice(1)} on ${n} vertices.`,
      kind === 'path' ? 'line' : 'circle'
    );
  })
);

$('btn-grid').addEventListener('click', () => {
  const spec = $('grid-input').value.trim();
  runGen(
    () => gen_grid(spec),
    'poset',
    (w) => `Product of chains ${spec}: ${w.labels.length} elements.`
  );
});

$('btn-distributive').addEventListener('click', () => {
  const { wire } = toWire();
  runGen(
    () => gen_distributive(JSON.stringify(wire)),
    'poset',
    (w) => `J(P) has ${w.labels.length} elements.`
  );
});

$('btn-infer').addEventListener('click', () => {
  const { wire } = toWire();
  try {
    const res = JSON.parse(infer_digit_relations(JSON.stringify(wire)));
    replaceGraph(res.graph, 'poset', `Inferred ${res.added} relations.`);
  } catch (e) {
    log(String(e), true);
  }
});

$('btn-tube-poset').addEventListener('click', () => {
  const { wire } = toWire();
  runGen(
    () => gen_tube_poset(JSON.stringify(wire)),
    'poset',
    (w) => `Tube poset: ${w.labels.length} tubes.`
  );
});

$('btn-graph-assoc').addEventListener('click', () => {
  const { wire } = toWire();
  runGen(
    () => gen_graph_associahedron(JSON.stringify(wire)),
    'poset',
    (w) => `Graph associahedron face lattice: ${w.labels.length} faces.`
  );
});

// ---------------------------------------------------------------------------
// Sidebar: file
// ---------------------------------------------------------------------------

function download(name, content, type = 'text/plain') {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([content], { type }));
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}

$('btn-save').addEventListener('click', () => {
  const { wire } = toWire();
  try {
    download('lattice.txt', to_lattice_file(JSON.stringify(wire)));
    log('Saved lattice.txt.');
  } catch (e) {
    log(String(e), true);
  }
});

$('btn-load').addEventListener('click', () => $('file-input').click());
$('file-input').addEventListener('change', async (e) => {
  const file = e.target.files[0];
  e.target.value = '';
  if (!file) return;
  try {
    const wire = JSON.parse(from_lattice_file(await file.text()));
    replaceGraph(wire, 'poset', `Loaded ${wire.labels.length} faces from ${file.name}.`);
  } catch (err) {
    log(String(err), true);
  }
});

// ---- TikZ export (port of gui.rs export_tikz) --------------------------------

$('btn-tikz').addEventListener('click', () => {
  const strip = currentStrip();
  const nodeIds = strip
    ? strip.layers.flat()
    : state.nodes.map((n) => n.id);
  const edges = strip
    ? [...strip.edges, ...strip.cyclicEdges]
    : state.edges;

  const safe = (s) =>
    s.replace(/[{}]/g, '')
      .replace(/[,|]/g, '_')
      .replace(/\*/g, 'top')
      .replace(/\?/g, 'empty');

  const scale = 0.02;
  let tex = '\\documentclass[tikz, border=1cm]{standalone}\n\\begin{document}\n';
  tex += '\\begin{tikzpicture}[y=-1cm]\n\n% Coordinates\n';
  for (const id of nodeIds) {
    const n = nodeById(id);
    if (!n) continue;
    tex += `\\coordinate (${safe(n.label)}) at (${(n.x * scale).toFixed(2)}, ${(n.y * scale).toFixed(2)});\n`;
  }
  tex += '\n% Edges\n\\foreach \\a/\\b in {';
  tex += edges
    .map(([a, b]) => {
      const na = nodeById(a), nb = nodeById(b);
      return na && nb ? `${safe(na.label)}/${safe(nb.label)}` : null;
    })
    .filter(Boolean)
    .join(', ');
  tex += '} {\n    \\draw (\\a) -- (\\b);\n}\n\n% Nodes\n\\foreach \\v/\\l in {';
  tex += nodeIds
    .map((id) => {
      const n = nodeById(id);
      return n ? `${safe(n.label)}/{${n.label.replace(/\|/g, '$|$')}}` : null;
    })
    .filter(Boolean)
    .join(', ');
  tex +=
    '} {\n    \\node[draw, circle, fill=white, inner sep=2pt] at (\\v) {\\footnotesize \\l};\n}\n';
  tex += '\\end{tikzpicture}\n\\end{document}\n';

  download('lattice_output.tex', tex, 'application/x-tex');
  log('Exported lattice_output.tex.');
});

// ---------------------------------------------------------------------------
// Strip search: worker orchestration (browser twin of gui.rs poll_job)
// ---------------------------------------------------------------------------

const LOOKAHEAD = 8;
let ticker = null;

function stopTicker() {
  if (ticker) { clearInterval(ticker); ticker = null; }
}

function startJob(kind) {
  invalidateResults();
  const { wire, idMap } = toWire();
  if (wire.labels.length === 0) {
    log('The diagram is empty.', true);
    return;
  }
  try {
    poset_ranks(JSON.stringify(wire)); // early cycle check with a clear error
  } catch (e) {
    log(String(e), true);
    return;
  }

  state.job = { kind, started: performance.now(), liveCount: 0, idMap };
  state.worker.postMessage({
    cmd: 'start',
    graph: wire,
    cyclic: $('cyclic').checked,
    mode: kind,
    wanted: LOOKAHEAD,
  });
  log(
    kind === 'exists' ? 'Checking existence…'
    : kind === 'count' ? 'Counting strips…'
    : 'Enumerating strips…'
  );
  ticker = setInterval(updateJobUi, 250);
  updateJobUi();
}

function elapsed(job) {
  return `${((performance.now() - job.started) / 1000).toFixed(1)}s`;
}

function updateJobUi() {
  const row = $('job-row');
  if (!state.job) {
    row.hidden = true;
    return;
  }
  row.hidden = false;
  const j = state.job;
  const status =
    j.kind === 'count' ? `counted ${j.liveCount} …`
    : j.kind === 'enumerate' ? `found ${state.strips.length} …`
    : 'searching …';
  $('job-status').textContent = `${status} (${elapsed(j)})`;
}

function onWorkerMessage(e) {
  const msg = e.data;
  const job = state.job;
  if (!job) return; // stale message from a cancelled job

  if (msg.type === 'strips') {
    const map = (f) => job.idMap[f];
    for (const s of msg.strips) {
      state.strips.push({
        layers: s.layers.map((l) => l.map(map).filter((x) => x !== undefined)),
        edges: s.edges
          .map(([a, b]) => [map(a), map(b)])
          .filter(([a, b]) => a !== undefined && b !== undefined),
        cyclicEdges: s.cyclicEdges
          .map(([a, b]) => [map(a), map(b)])
          .filter(([a, b]) => a !== undefined && b !== undefined),
      });
    }
    job.liveCount = msg.count;
    if (state.strips.length === msg.strips.length) {
      // first arrivals: show immediately
      state.cursor = 0;
      state.viewing = true;
      arrangeAsStrip(0);
      if (job.kind === 'exists') {
        log(`A rhombic strip EXISTS (${elapsed(job)}) — shown on the canvas.`);
      }
    }
    refresh();
  } else if (msg.type === 'progress') {
    job.liveCount = msg.count;
  } else if (msg.type === 'done') {
    state.totalStrips = msg.count;
    if (job.kind === 'count') {
      log(`${msg.count} rhombic strips (${elapsed(job)}).`);
    } else if (job.kind === 'enumerate') {
      log(`Enumeration finished: ${msg.count} strips (${elapsed(job)}).`);
    } else if (msg.count === 0) {
      log(`No rhombic strip exists (${elapsed(job)}).`);
    }
    state.job = null;
    stopTicker();
    updateJobUi();
    refresh();
  } else if (msg.type === 'error') {
    log(msg.message, true);
    state.job = null;
    stopTicker();
    updateJobUi();
  }
}

$('btn-exists').addEventListener('click', () => startJob('exists'));
$('btn-count').addEventListener('click', () => startJob('count'));
$('btn-enumerate').addEventListener('click', () => startJob('enumerate'));
$('btn-cancel').addEventListener('click', () => {
  if (state.worker) state.worker.postMessage({ cmd: 'cancel' });
  state.job = null;
  stopTicker();
  updateJobUi();
  log('Cancelled.');
});

// ---- strip navigation ---------------------------------------------------------

function updateStripUi() {
  const nav = $('strip-nav');
  if (state.strips.length === 0) {
    nav.hidden = true;
    return;
  }
  nav.hidden = false;
  const total =
    state.totalStrips != null ? `${state.totalStrips}` : `≥${state.strips.length}`;
  $('strip-counter').textContent = `Strip ${state.cursor + 1} of ${total}`;
  $('btn-prev').disabled = state.cursor === 0;
  $('btn-next').disabled =
    state.cursor + 1 >= state.strips.length && !state.job;
  $('btn-toggle-strip').textContent = state.viewing ? 'Hide' : 'Show';

  // layer readout (top layer first — the list is `reversed`)
  const list = $('layers-list');
  list.innerHTML = '';
  const view = state.strips[state.cursor];
  view.layers.forEach((layer, li) => {
    const item = document.createElement('li');
    const chip = document.createElement('span');
    chip.className = 'chip';
    chip.style.background = LAYER_COLORS[li % LAYER_COLORS.length];
    item.appendChild(chip);
    item.appendChild(
      document.createTextNode(`[${layer.map(labelOf).join(', ')}]`)
    );
    list.prepend(item);
  });
}

$('btn-prev').addEventListener('click', () => {
  if (state.cursor > 0) {
    state.cursor--;
    state.viewing = true;
    arrangeAsStrip(state.cursor);
    refresh();
  }
});

$('btn-next').addEventListener('click', () => {
  if (state.cursor + 1 < state.strips.length) {
    state.cursor++;
    state.viewing = true;
    arrangeAsStrip(state.cursor);
    refresh();
  }
  // keep the worker one lookahead ahead of the cursor
  if (state.worker && state.job) {
    state.worker.postMessage({
      cmd: 'advance',
      wanted: state.cursor + 1 + LOOKAHEAD,
    });
  }
});

$('btn-arrange-strip').addEventListener('click', () => {
  arrangeAsStrip(state.cursor);
  refresh();
});

$('btn-toggle-strip').addEventListener('click', () => {
  state.viewing = !state.viewing;
  refresh();
});

$('btn-copy-layers').addEventListener('click', async () => {
  const view = state.strips[state.cursor];
  if (!view) return;
  const text = view.layers
    .map((layer) => `[${layer.map(labelOf).join(', ')}]`)
    .join('\n');
  try {
    await navigator.clipboard.writeText(text);
    log('Layers copied to clipboard.');
  } catch {
    log('Clipboard unavailable — select the layers manually.', true);
  }
});

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

new ResizeObserver(resizeCanvas).observe(canvas);
window.addEventListener('resize', resizeCanvas);

init()
  .then(() => {
    state.worker = new Worker(new URL('./worker.js', import.meta.url), {
      type: 'module',
    });
    state.worker.onmessage = onWorkerMessage;
    setMode('poset');
    const [w, h] = canvasSize();
    state.view.ox = w / 2;
    state.view.oy = h * 0.66;
    log('Welcome. Double-click the canvas to add nodes, click two nodes to relate them.');
    refresh();
  })
  .catch((e) => log(`Failed to load the WebAssembly module: ${e}`, true));
