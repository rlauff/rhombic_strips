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

function restartWorker() {
  if (state.worker) {
    state.worker.terminate(); // Instantly kills the thread & WASM memory
  }
  state.worker = new Worker(new URL('./worker.js', import.meta.url), {
    type: 'module',
  });
  state.worker.onmessage = onWorkerMessage;
}

function invalidateResults() {
  if (state.worker && state.job && !state.job.remote) restartWorker();
  if (remote.abort) {
    remote.abort.abort(); // the helper kills the process group on disconnect
    remote.abort = null;
  }
  
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

  // empty worksheet: a quiet hint instead of a blank page
  if (state.nodes.length === 0) {
    ctx.fillStyle = '#aaa598';
    ctx.textAlign = 'center';
    ctx.font = '13.5px "IBM Plex Sans", sans-serif';
    ctx.textBaseline = 'bottom';
    ctx.fillText('Double-click to add a node, or pick an example on the left.', w / 2, h / 2 - 6);
    ctx.textBaseline = 'top';
    ctx.font = 'italic 12.5px "IBM Plex Sans", sans-serif';
    ctx.fillText('Press ? for keyboard & mouse reference.', w / 2, h / 2 + 6);
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
      const bx = pointer.sx;
      const by = pointer.sy;

      ctx.strokeStyle = SKY;
      ctx.lineWidth = 1.5;
      
      // Draw the dashed preview line
      ctx.setLineDash([5, 4]);
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
      ctx.setLineDash([]); // Reset immediately for the arrow

      // Draw the preview arrow
      if (state.mode === 'poset') {
        const mx = (ax + bx) / 2;
        const my = (ay + by) / 2;
        const angle = Math.atan2(by - ay, bx - ax);
        const len = 8;
        
        ctx.beginPath();
        ctx.moveTo(mx, my);
        ctx.lineTo(mx - len * Math.cos(angle - Math.PI / 6), my - len * Math.sin(angle - Math.PI / 6));
        ctx.moveTo(mx, my);
        ctx.lineTo(mx - len * Math.cos(angle + Math.PI / 6), my - len * Math.sin(angle + Math.PI / 6));
        ctx.stroke();
      }
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
    
    // Draw the main line
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.stroke();

    // Draw the arrow chevron (only for directed posets)
    if (state.mode === 'poset') {
      const mx = (ax + bx) / 2;
      const my = (ay + by) / 2;
      const angle = Math.atan2(by - ay, bx - ax);
      const len = 8; // Size of the arrowhead
      
      // Save dash state and enforce a solid stroke for the arrowhead
      // (This prevents the arrow from looking broken if the edge is a dashed cyclic edge)
      const dash = ctx.getLineDash();
      ctx.setLineDash([]);
      
      ctx.beginPath();
      // Draw top wing of the arrow
      ctx.moveTo(mx, my);
      ctx.lineTo(mx - len * Math.cos(angle - Math.PI / 6), my - len * Math.sin(angle - Math.PI / 6));
      // Draw bottom wing of the arrow
      ctx.moveTo(mx, my);
      ctx.lineTo(mx - len * Math.cos(angle + Math.PI / 6), my - len * Math.sin(angle + Math.PI / 6));
      ctx.stroke();
      
      // Restore the dash state
      ctx.setLineDash(dash);
    }
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
      // FIX: Cache the starting node ID before structuralChange() clears it
      const startId = state.edgeStart; 
      
      structuralChange();
      
      // Use the cached startId
      const added = toggleEdge(startId, d.id);
      const verb = added ? 'Added' : 'Removed';
      const kind = state.mode === 'poset' ? 'relation' : 'edge';
      log(`${verb} ${kind} ${labelOf(startId)} → ${labelOf(d.id)}.`);
      
      // state.edgeStart is already null from structuralChange, but we keep this clean
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

// ---- shortcut sheet ------------------------------------------------------------

const helpOverlay = $('help-overlay');

function showHelp(show) {
  helpOverlay.hidden = !show;
}

$('btn-help').addEventListener('click', () => showHelp(true));
$('zoom-help').addEventListener('click', () => showHelp(true));
$('help-close').addEventListener('click', () => showHelp(false));
helpOverlay.addEventListener('click', (e) => {
  if (e.target === helpOverlay) showHelp(false); // click outside the card
});

// ---- global shortcuts --------------------------------------------------------

function zoomCentered(factor) {
  const [w, h] = canvasSize();
  zoomAt(w / 2, h / 2, factor);
}

$('zoom-in').addEventListener('click', () => zoomCentered(1.2));
$('zoom-out').addEventListener('click', () => zoomCentered(1 / 1.2));

window.addEventListener('keydown', (e) => {
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) {
    return; // typing in a field
  }

  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
    e.preventDefault();
    undo();
    return;
  }

  if (e.key === 'Escape') {
    if (!remoteOverlay.hidden) {
      remoteOverlay.hidden = true;
    } else if (!helpOverlay.hidden) {
      showHelp(false);
    } else {
      cancelLink();
      finishRename(false);
      render();
    }
    return;
  }

  if (e.key === '?') {
    showHelp(helpOverlay.hidden);
    return;
  }

  // single-key shortcuts: never steal browser/system chords
  if (e.ctrlKey || e.metaKey || e.altKey) return;

  const poset = state.mode === 'poset';
  switch (e.key) {
    case 'n':
      if (pointer.inside) {
        addNodeAt(...toWorld(pointer.sx, pointer.sy));
      } else {
        $('btn-add').click();
      }
      break;
    case 'Delete':
    case 'Backspace': {
      e.preventDefault();
      const n = state.hovered != null ? nodeById(state.hovered) : null;
      if (n) {
        structuralChange();
        removeNode(n.id);
        log(`Deleted node “${n.label}”.`);
        refresh();
      }
      break;
    }
    case 'a':
      $('btn-arrange').click();
      break;
    case 'f':
      fitView();
      render();
      break;
    case 'm':
      setMode(poset ? 'graph' : 'poset');
      refresh();
      break;
    case '+':
    case '=':
      zoomCentered(1.2);
      break;
    case '-':
    case '_':
      zoomCentered(1 / 1.2);
      break;
    case 'x':
      if (poset) startJob('exists');
      break;
    case 'c':
      if (poset) startJob('count');
      break;
    case 'e':
      if (poset) startJob('enumerate');
      break;
    case 's':
      if (state.strips.length > 0) $('btn-toggle-strip').click();
      break;
    case 't':
      if (poset) $('btn-tikz').click();
      break;
    case 'ArrowLeft':
      if (state.strips.length > 0) {
        e.preventDefault();
        $('btn-prev').click();
      }
      break;
    case 'ArrowRight':
      if (state.strips.length > 0) {
        e.preventDefault();
        $('btn-next').click();
      }
      break;
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
  $('btn-arrange').innerHTML =
    mode === 'poset' ? 'Arrange by rank <kbd>A</kbd>' : 'Circle layout <kbd>A</kbd>';
  updateStats();
}

$('mode-poset').addEventListener('click', () => { setMode('poset'); refresh(); });
$('mode-graph').addEventListener('click', () => { setMode('graph'); refresh(); });

// ---------------------------------------------------------------------------
// Sidebar: edit
// ---------------------------------------------------------------------------

/// Add a node near (wx, wy), nudging right past occupied spots; used by the
/// "Add" button (view center) and the N shortcut (pointer position).
function addNodeAt(wx, wy) {
  structuralChange();
  while (state.nodes.some((n) => Math.abs(n.x - wx) < 10 && Math.abs(n.y - wy) < 10)) {
    wx += 60;
  }
  const id = addNode($('label-input').value.trim(), wx, wy);
  $('label-input').value = '';
  log(`Added node ${labelOf(id)}.`);
  refresh();
}

$('btn-add').addEventListener('click', () => {
  const [w, h] = canvasSize();
  const [wx, wy] = toWorld(w / 2, h / 2);
  addNodeAt(wx, wy);
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
// Remote compute: a native helper on 127.0.0.1 — either strip_stream running
// on this machine, or srun on the cluster reached through the user's own
// `ssh -L` tunnel (cluster/serve.sh). The page only ever talks to loopback;
// ssh authentication and Slurm accounting stay in the user's terminal, so
// nobody can compute on someone else's allocation. A pairing code (printed
// by the helper, entered once, kept in localStorage) stops other users on a
// shared login node — and random web pages probing localhost — from
// submitting jobs through the relay.
// ---------------------------------------------------------------------------

const RELAY = 'http://127.0.0.1:8642';
const ENUM_CAP = 512; // one-shot stream: enumerate stops here (log says so)
const SERVE_URL =
  'https://raw.githubusercontent.com/rlauff/rhombic_strips/main/cluster/serve.sh';

const remote = {
  backend: 'wasm', // 'wasm' | 'local' | 'cluster'
  token: localStorage.getItem('rhombic.token') || '',
  host: localStorage.getItem('rhombic.host') || '',
  partition: localStorage.getItem('rhombic.partition') || '',
  time: localStorage.getItem('rhombic.time') || '',
  info: null,   // last successful /ping payload
  abort: null,  // AbortController of the running remote job
};

const remoteOverlay = $('remote-overlay');
const backendStatusEl = $('backend-status');

function saveRemoteSettings() {
  localStorage.setItem('rhombic.backend', remote.backend);
  localStorage.setItem('rhombic.token', remote.token);
  localStorage.setItem('rhombic.host', remote.host);
  localStorage.setItem('rhombic.partition', remote.partition);
  localStorage.setItem('rhombic.time', remote.time);
}

async function pingRelay() {
  const res = await fetch(`${RELAY}/ping`, {
    headers: remote.token ? { 'X-Rhombic-Token': remote.token } : {},
    signal: AbortSignal.timeout(2500),
  });
  return await res.json();
}

function describeRelay(info) {
  const where = info.mode === 'slurm' ? `slurm @ ${info.host}` : `native @ ${info.host}`;
  return `${where} · ${info.threads} threads`;
}

/// Switch the compute backend. Pings the helper for 'local'/'cluster' and, if
/// interactive, opens the setup dialog when the helper is missing or unpaired.
async function setBackend(b, interactive = true) {
  remote.backend = b;
  saveRemoteSettings();
  for (const id of ['wasm', 'local', 'cluster']) {
    const el = $('backend-' + id);
    el.classList.toggle('active', id === b);
    el.setAttribute('aria-selected', id === b);
  }
  if (b === 'wasm') {
    backendStatusEl.hidden = true;
    remote.info = null;
    return;
  }
  backendStatusEl.hidden = false;
  backendStatusEl.className = 'backend-status';
  backendStatusEl.textContent = 'looking for the compute helper…';
  try {
    const info = await pingRelay();
    remote.info = info;
    if (info.paired) {
      backendStatusEl.className = 'backend-status ok';
      backendStatusEl.textContent = describeRelay(info);
    } else {
      backendStatusEl.className = 'backend-status error';
      backendStatusEl.textContent = 'helper found — pairing code needed (click here)';
      if (interactive) openRemoteSetup();
    }
  } catch {
    remote.info = null;
    backendStatusEl.className = 'backend-status error';
    backendStatusEl.textContent = 'helper not running — click here for setup';
    if (interactive) openRemoteSetup();
  }
}

$('backend-wasm').addEventListener('click', () => setBackend('wasm'));
$('backend-local').addEventListener('click', () => setBackend('local'));
$('backend-cluster').addEventListener('click', () => setBackend('cluster'));
backendStatusEl.addEventListener('click', () => {
  if (remote.backend !== 'wasm') openRemoteSetup();
});

// ---- setup dialog -------------------------------------------------------------

/// The exact command to paste into a terminal. For the cluster this is one
/// ssh line: your usual login (keys/password/OTP prompt in the terminal) that
/// simultaneously tunnels the helper's loopback port back to this page.
function remoteCommand() {
  if (remote.backend === 'local') {
    return `curl -fsSL ${SERVE_URL} | bash -s -- --local`;
  }
  const host = remote.host.trim() || 'you@sshgate.math.tu-berlin.de';
  let opts = '';
  if (remote.partition.trim()) opts += ` --partition=${remote.partition.trim()}`;
  if (remote.time.trim()) opts += ` --time=${remote.time.trim()}`;
  return (
    `ssh -t -L 8642:127.0.0.1:8642 ${host} ` +
    `'curl -fsSL ${SERVE_URL} | bash -s --${opts}'`
  );
}

function renderRemoteSetup() {
  const cluster = remote.backend === 'cluster';
  $('remote-title').textContent = cluster ? 'Cluster compute' : 'Compute on this machine';
  $('remote-cluster-fields').hidden = !cluster;
  $('remote-explain-cmd').textContent = cluster
    ? 'Paste this into a terminal. It is your normal ssh login (password or keys stay ' +
      'in the terminal); on first use it builds the search binary on the cluster, then ' +
      'runs jobs via srun for as long as the terminal stays open:'
    : 'Paste this into a terminal on this machine. On first use it builds the native ' +
      'search binary (all cores, unlike the browser build), then serves it to this ' +
      'page for as long as the terminal stays open:';
  $('remote-cmd').textContent = remoteCommand();
}

function openRemoteSetup() {
  renderRemoteSetup();
  $('remote-host').value = remote.host;
  $('remote-partition').value = remote.partition;
  $('remote-time').value = remote.time;
  $('remote-code').value = remote.token;
  $('remote-status').className = 'remote-status';
  $('remote-status').textContent = '';
  remoteOverlay.hidden = false;
}

for (const [id, key] of [
  ['remote-host', 'host'],
  ['remote-partition', 'partition'],
  ['remote-time', 'time'],
]) {
  $(id).addEventListener('input', (e) => {
    remote[key] = e.target.value;
    saveRemoteSettings();
    renderRemoteSetup();
  });
}

$('remote-copy').addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(remoteCommand());
    $('remote-copy').textContent = 'Copied';
    setTimeout(() => { $('remote-copy').textContent = 'Copy'; }, 1200);
  } catch {
    $('remote-status').className = 'remote-status error';
    $('remote-status').textContent = 'Clipboard unavailable — select the command manually.';
  }
});

$('remote-connect').addEventListener('click', async () => {
  remote.token = $('remote-code').value.trim();
  saveRemoteSettings();
  const status = $('remote-status');
  status.className = 'remote-status';
  status.textContent = 'connecting…';
  try {
    const info = await pingRelay();
    remote.info = info;
    if (info.paired) {
      status.className = 'remote-status ok';
      status.textContent = `Connected: ${describeRelay(info)}`;
      setBackend(remote.backend, false);
      setTimeout(() => { remoteOverlay.hidden = true; }, 700);
    } else {
      status.className = 'remote-status error';
      status.textContent = remote.token
        ? 'Helper is running, but the pairing code does not match.'
        : 'Helper is running — enter the pairing code it printed.';
    }
  } catch {
    status.className = 'remote-status error';
    status.textContent = 'No helper on 127.0.0.1:8642 yet — run the command above first.';
  }
});

$('remote-close').addEventListener('click', () => { remoteOverlay.hidden = true; });
remoteOverlay.addEventListener('click', (e) => {
  if (e.target === remoteOverlay) remoteOverlay.hidden = true;
});

// ---- remote job runner ----------------------------------------------------------

/// The fetch-streaming twin of the Web Worker: POSTs the job, reads the
/// chunked NDJSON response line by line, and feeds each message into the same
/// applyJobMessage the worker path uses. Cancel = abort the fetch; the helper
/// kills the process group, which also releases a Slurm allocation.
async function startRemoteJob(kind, wire) {
  const job = state.job;
  const ctrl = new AbortController();
  remote.abort = ctrl;
  try {
    const res = await fetch(`${RELAY}/job`, {
      method: 'POST',
      signal: ctrl.signal,
      headers: {
        'Content-Type': 'application/json',
        'X-Rhombic-Token': remote.token,
      },
      body: JSON.stringify({
        graph: wire,
        cyclic: $('cyclic').checked,
        mode: kind,
        cap: kind === 'enumerate' ? ENUM_CAP : 0,
      }),
    });
    if (res.status === 401) {
      applyJobMessage({ type: 'error', message: 'Not paired with the compute helper.' });
      openRemoteSetup();
      return;
    }
    if (!res.ok) {
      applyJobMessage({ type: 'error', message: `Compute helper error: HTTP ${res.status}.` });
      return;
    }

    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buf = '';
    let sawDone = false;
    for (;;) {
      const { value, done } = await reader.read();
      if (state.job !== job) { ctrl.abort(); return; } // superseded / cancelled
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      let nl;
      while ((nl = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, nl).trim();
        buf = buf.slice(nl + 1);
        if (!line) continue;
        const msg = JSON.parse(line);
        if (msg.type === 'done' || msg.type === 'error') sawDone = true;
        applyJobMessage(msg);
      }
    }
    if (!sawDone && state.job === job) {
      applyJobMessage({
        type: 'error',
        message: 'The compute helper closed the stream unexpectedly.',
      });
    }
  } catch (err) {
    if (ctrl.signal.aborted || state.job !== job) return; // user cancelled
    applyJobMessage({
      type: 'error',
      message: `Compute helper unreachable: ${err.message}`,
    });
    setBackend(remote.backend, false); // refresh the status line
  } finally {
    if (remote.abort === ctrl) remote.abort = null;
  }
}

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

  const isRemote = remote.backend !== 'wasm';
  state.job = { kind, started: performance.now(), liveCount: 0, idMap, remote: isRemote };
  if (isRemote) {
    startRemoteJob(kind, wire);
  } else {
    state.worker.postMessage({
      cmd: 'start',
      graph: wire,
      cyclic: $('cyclic').checked,
      mode: kind,
      wanted: LOOKAHEAD,
    });
  }
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
    row.style.display = 'none'; // Force it to hide, overriding any CSS layout
    return;
  }
  
  row.hidden = false;
  row.style.display = ''; // Clear inline styles so your CSS can take over again
  
  const j = state.job;
  const status =
    j.kind === 'count' ? `counted ${j.liveCount} …`
    : j.kind === 'enumerate' ? `found ${state.strips.length} …`
    : 'searching …';
  $('job-status').textContent = `${status} (${elapsed(j)})`;
}

function onWorkerMessage(e) {
  applyJobMessage(e.data);
}

/// One handler for both transports: Web Worker messages and NDJSON lines
/// streamed from the native compute helper (see startRemoteJob) — the wire
/// shapes are identical by construction (src/bin/strip_stream.rs).
function applyJobMessage(msg) {
  const job = state.job;
  if (!job) return; // stale message from a cancelled job

  if (msg.type === 'note') {
    log(msg.message);
  } else if (msg.type === 'strips') {
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
      log(`Enumeration finished: ${msg.count} strips${msg.capped ? ' (capped)' : ''} (${elapsed(job)}).`);
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
  if (remote.abort) {
    remote.abort.abort();
    remote.abort = null;
  } else if (state.worker) {
    // terminate, don't post: a 'cancel' message can never reach a worker
    // that is stuck inside one long wasm step (the budget is only checked
    // between strips, and finding the next strip can take arbitrarily long).
    restartWorker();
  }
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
// Scripts panel: batch jobs in a dedicated worker (src/scripts.rs). Uses its
// own Worker instance so a running script never fights the strip search over
// one thread; cancellation terminates the worker (same rationale as strips).
// Script jobs survive diagram edits: the survey is independent of the editor,
// and the boundary job snapshots the poset when it starts.
// ---------------------------------------------------------------------------

const scripts = {
  worker: null,
  job: null, // {kind: 'survey' | 'bounds', started}
  survey: { results: [], opts: null, done: false },
  bounds: { pairs: [], count: 0, distinct: 0, done: false },
};

function scriptWorker() {
  if (!scripts.worker) {
    scripts.worker = new Worker(new URL('./worker.js', import.meta.url), {
      type: 'module',
    });
    scripts.worker.onmessage = (e) => onScriptMessage(e.data);
  }
  return scripts.worker;
}

function cancelScript(quiet = false) {
  if (scripts.worker) {
    // terminate, don't post: one script step can take arbitrarily long
    // (a single hard strip-existence search), so a 'cancel' message might
    // never be seen. A fresh worker is created lazily on the next run.
    scripts.worker.terminate();
    scripts.worker = null;
  }
  if (scripts.job) {
    scripts.job = null;
    updateScriptJobUi('');
    if (!quiet) log('Script cancelled.');
  }
}

function updateScriptJobUi(status) {
  $('script-job').hidden = !scripts.job;
  if (scripts.job && status !== undefined) {
    $('script-status').textContent = status;
  }
}

function scriptElapsed() {
  return `${((performance.now() - scripts.job.started) / 1000).toFixed(1)}s`;
}

// ---- tabs -----------------------------------------------------------------

function setScriptTab(kind) {
  const survey = kind === 'survey';
  $('script-tab-survey').classList.toggle('active', survey);
  $('script-tab-bounds').classList.toggle('active', !survey);
  $('script-tab-survey').setAttribute('aria-selected', survey);
  $('script-tab-bounds').setAttribute('aria-selected', !survey);
  $('script-survey-opts').hidden = !survey;
  $('script-bounds-opts').hidden = survey;
  renderSurvey();
  renderBounds();
}

$('script-tab-survey').addEventListener('click', () => setScriptTab('survey'));
$('script-tab-bounds').addEventListener('click', () => setScriptTab('bounds'));

// ---- run ------------------------------------------------------------------

$('btn-run-survey').addEventListener('click', () => {
  const n = Math.min(7, Math.max(2, parseInt($('survey-n').value, 10) || 5));
  $('survey-n').value = n;
  const linear = $('survey-linear').checked;
  const cyclic = $('survey-cyclic').checked;
  cancelScript(true);
  scripts.survey = { results: [], opts: { n, linear, cyclic }, done: false };
  scripts.job = { kind: 'survey', started: performance.now() };
  scriptWorker().postMessage({ cmd: 'survey', maxN: n, linear, cyclic });
  updateScriptJobUi('generating graphs …');
  renderSurvey();
  log(`Surveying all connected graphs on ≤ ${n} vertices…`);
});

$('btn-run-bounds').addEventListener('click', () => {
  if (state.mode !== 'poset') {
    log('Strip boundaries runs on a poset — switch to poset mode.', true);
    return;
  }
  const { wire } = toWire();
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
  cancelScript(true);
  scripts.bounds = { pairs: [], count: 0, distinct: 0, done: false };
  scripts.job = { kind: 'bounds', started: performance.now() };
  scriptWorker().postMessage({ cmd: 'bounds', graph: wire });
  updateScriptJobUi('enumerating strips …');
  renderBounds();
  log('Enumerating strip boundaries…');
});

$('btn-script-cancel').addEventListener('click', () => cancelScript());

// ---- messages -------------------------------------------------------------

function onScriptMessage(msg) {
  const job = scripts.job;
  if (!job) return; // stale message from a cancelled job

  if (msg.type === 'error') {
    log(msg.message, true);
    scripts.job = null;
    updateScriptJobUi('');
    return;
  }

  if (msg.type === 'survey' && job.kind === 'survey') {
    const st = scripts.survey;
    if (msg.results.length) st.results.push(...msg.results);
    st.done = msg.done;
    updateScriptJobUi(
      msg.phase === 'generate'
        ? `generating graphs (n=${msg.level}) … (${scriptElapsed()})`
        : `checked ${msg.checked}/${msg.total} … (${scriptElapsed()})`
    );
    if (msg.results.length || msg.done) renderSurvey();
    if (msg.done) {
      log(`Survey finished: ${st.results.length} graphs (${scriptElapsed()}).`);
      scripts.job = null;
      updateScriptJobUi('');
    }
  } else if (msg.type === 'bounds' && job.kind === 'bounds') {
    scripts.bounds = {
      pairs: msg.pairs,
      count: msg.count,
      distinct: msg.distinct,
      done: msg.done,
    };
    updateScriptJobUi(
      `${msg.count} strips · ${msg.distinct} pairs … (${scriptElapsed()})`
    );
    renderBounds();
    if (msg.done) {
      log(
        `Boundary enumeration finished: ${msg.count} strips, ` +
          `${msg.distinct} boundary pairs (${scriptElapsed()}).`
      );
      scripts.job = null;
      updateScriptJobUi('');
    }
  }
}

// ---- survey rendering -----------------------------------------------------

const mark = (v) => (v === true ? '✓' : v === false ? '✗' : '·');

function graphThumbSvg(r) {
  const s = 56;
  const cx = s / 2, cy = s / 2, rad = 19;
  const pts = Array.from({ length: r.n }, (_, i) => {
    const a = (2 * Math.PI * i) / r.n - Math.PI / 2;
    return [cx + rad * Math.cos(a), cy + rad * Math.sin(a)];
  });
  const f = (x) => x.toFixed(1);
  let svg = `<svg viewBox="0 0 ${s} ${s}" aria-hidden="true">`;
  for (const [a, b] of r.edges) {
    svg +=
      `<line x1="${f(pts[a][0])}" y1="${f(pts[a][1])}" ` +
      `x2="${f(pts[b][0])}" y2="${f(pts[b][1])}" ` +
      `stroke="currentColor" stroke-width="1.4"/>`;
  }
  for (const [x, y] of pts) {
    svg += `<circle cx="${f(x)}" cy="${f(y)}" r="2.6" fill="currentColor"/>`;
  }
  return svg + '</svg>';
}

function surveyCard(r) {
  const opts = scripts.survey.opts ?? {};
  const card = document.createElement('button');
  card.type = 'button';
  card.className = 'g-card';
  const cex = r.hamPath && r.strip === false; // conjecture counterexample
  if (cex) card.classList.add('cex');

  let dots = '';
  if (opts.linear) {
    dots += `<span class="g-dot ${r.strip ? 'ok' : 'bad'}" title="rhombic strip ${mark(r.strip)}"></span>`;
  }
  if (opts.cyclic) {
    dots += `<span class="g-dot cyc ${r.cyclicStrip ? 'ok' : 'bad'}" title="cyclic strip ${mark(r.cyclicStrip)}"></span>`;
  }

  card.title =
    `n=${r.n} · ${r.edges.length} edges · ${r.tubes} tubes · ` +
    `Ham path ${mark(r.hamPath)} · Ham cycle ${mark(r.hamCycle)}` +
    (opts.linear ? ` · strip ${mark(r.strip)}` : '') +
    (opts.cyclic ? ` · cyclic strip ${mark(r.cyclicStrip)}` : '') +
    (cex ? ' — COUNTEREXAMPLE' : '') +
    '\nClick to open in the graph editor.';
  card.innerHTML =
    graphThumbSvg(r) + (dots ? `<span class="g-dots">${dots}</span>` : '');
  card.addEventListener('click', () => {
    const wire = {
      labels: Array.from({ length: r.n }, (_, i) => String(i)),
      edges: r.edges,
    };
    replaceGraph(
      wire,
      'graph',
      `Survey graph: n=${r.n}, ${r.edges.length} edges — ` +
        `Ham path ${mark(r.hamPath)}, Ham cycle ${mark(r.hamCycle)}` +
        (opts.linear ? `, strip ${mark(r.strip)}` : '') +
        (opts.cyclic ? `, cyclic ${mark(r.cyclicStrip)}` : '') + '.',
      'circle'
    );
  });
  return card;
}

function surveyGroupStats(rs, opts) {
  const bits = [`${rs.length}`];
  if (opts.linear) {
    bits.push(`strip ${rs.filter((r) => r.strip === true).length}/${rs.length}`);
  }
  if (opts.cyclic) {
    bits.push(
      `cyclic ${rs.filter((r) => r.cyclicStrip === true).length}/${rs.length}`
    );
  }
  return bits.join(' · ');
}

function renderSurvey() {
  const el = $('survey-results');
  const st = scripts.survey;
  const active = !$('script-survey-opts').hidden;
  el.hidden = !active || st.results.length === 0;
  el.innerHTML = '';
  if (el.hidden) return;

  const opts = st.opts ?? {};
  const total = st.results.length;

  const sum = document.createElement('p');
  sum.className = 'survey-summary';
  sum.textContent =
    `${total} graph${total === 1 ? '' : 's'} on ≤ ${opts.n} vertices` +
    (st.done ? '' : ' so far') +
    (opts.linear || opts.cyclic ? ` · ${surveyGroupStats(st.results, opts)}` : '');
  el.appendChild(sum);

  // conjecture verdicts
  if (opts.linear) {
    const cex = st.results.filter((r) => r.hamPath && r.strip === false);
    const p = document.createElement('p');
    p.className = 'conj ' + (cex.length ? 'bad' : 'ok');
    p.textContent = cex.length
      ? `✗ ${cex.length} counterexample${cex.length === 1 ? '' : 's'}: ` +
        'Hamilton path but no rhombic strip!'
      : (st.done ? '✓ ' : '') +
        'Hamilton path ⇒ rhombic strip: no counterexample' +
        (st.done ? '.' : ' so far.');
    el.appendChild(p);
  }
  if (opts.cyclic) {
    const bad = st.results.filter((r) => r.hamCycle && r.cyclicStrip === false);
    const p = document.createElement('p');
    p.className = 'conj note';
    p.textContent =
      `Hamilton cycle without cyclic strip: ${bad.length} graph` +
      `${bad.length === 1 ? '' : 's'}` +
      (bad.length ? ' (the cyclic analogue fails, as known).' : '.');
    el.appendChild(p);
  }

  const groups = $('survey-ham').checked
    ? [
        ['Hamilton cycle', (r) => r.hamCycle],
        ['Hamilton path only', (r) => r.hamPath && !r.hamCycle],
        ['No Hamilton path', (r) => !r.hamPath],
      ]
    : [['All graphs', () => true]];

  const badness = (r) =>
    r.hamPath && r.strip === false ? 0 // counterexample first
    : r.strip === false ? 1
    : r.hamCycle && r.cyclicStrip === false ? 2
    : 3;

  for (const [name, pred] of groups) {
    const rs = st.results.filter(pred);
    if (rs.length === 0) continue;
    rs.sort(
      (a, b) =>
        a.n - b.n || badness(a) - badness(b) || a.edges.length - b.edges.length
    );
    const det = document.createElement('details');
    det.className = 'survey-group';
    det.open = true;
    const summary = document.createElement('summary');
    summary.textContent = `${name} — ${surveyGroupStats(rs, opts)}`;
    det.appendChild(summary);
    const grid = document.createElement('div');
    grid.className = 'g-grid';
    for (const r of rs) grid.appendChild(surveyCard(r));
    det.appendChild(grid);
    el.appendChild(det);
  }
}

$('survey-ham').addEventListener('change', renderSurvey);

// ---- boundary rendering ---------------------------------------------------

function renderBounds() {
  const el = $('bounds-results');
  const st = scripts.bounds;
  const active = !$('script-bounds-opts').hidden;
  el.hidden = !active || st.pairs.length === 0;
  el.innerHTML = '';
  if (el.hidden) return;

  const head = document.createElement('div');
  head.className = 'bounds-head';
  const sum = document.createElement('p');
  sum.className = 'survey-summary';
  sum.textContent =
    `${st.count} strip${st.count === 1 ? '' : 's'} · ` +
    `${st.distinct} boundary pair${st.distinct === 1 ? '' : 's'}` +
    (st.done ? '' : ' …');
  head.appendChild(sum);
  const copy = document.createElement('button');
  copy.className = 'small';
  copy.textContent = 'Copy';
  copy.addEventListener('click', async () => {
    const text = st.pairs
      .map((p) => `${p.left} -> ${p.right}  x${p.count}`)
      .join('\n');
    try {
      await navigator.clipboard.writeText(text);
      log('Boundary pairs copied to clipboard.');
    } catch {
      log('Clipboard unavailable — select the pairs manually.', true);
    }
  });
  head.appendChild(copy);
  el.appendChild(head);

  const list = document.createElement('div');
  list.className = 'pair-list';
  for (const p of st.pairs) {
    const row = document.createElement('div');
    row.className = 'pair-row';
    const code = document.createElement('code');
    code.textContent = `${p.left} → ${p.right}`;
    const count = document.createElement('span');
    count.className = 'pair-count';
    count.textContent = `×${p.count}`;
    row.append(code, count);
    list.appendChild(row);
  }
  el.appendChild(list);
}

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
    log('Ready. Double-click the canvas to add nodes; press ? for all shortcuts.');
    const savedBackend = localStorage.getItem('rhombic.backend');
    if (savedBackend === 'local' || savedBackend === 'cluster') {
      setBackend(savedBackend, false); // reconnects silently if the helper is up
    }
    refresh();
  })
  .catch((e) => log(`Failed to load the WebAssembly module: ${e}`, true));
