// Strip-search worker: the browser twin of the native worker thread in
// gui.rs. The main thread posts {cmd: 'start' | 'advance' | 'cancel'};
// the worker streams {type: 'strips' | 'progress' | 'done' | 'error'}.
//
// Backpressure mirrors the bounded channel: in enumerate mode the worker
// only searches ahead of the browsing cursor by the lookahead the main
// thread asked for ('advance' raises it), so memory stays proportional to
// how far the user has browsed.

import init, { StripEnumerator } from './pkg/rhombic_strips.js';

const ready = init();

const BUDGET_MS = 30; // per slice, keeps the worker responsive to 'cancel'
const BATCH = 8;      // max strips per slice

let en = null;
let mode = null;
let sent = 0;
let wanted = BATCH;
let running = false;

function stop() {
  if (en) {
    en.free();
    en = null;
  }
  running = false;
}

function pump() {
  if (!en) {
    running = false;
    return;
  }
  if (mode === 'enumerate' && sent >= wanted) {
    running = false; // paused; an 'advance' message resumes
    return;
  }

  const res = JSON.parse(en.step(BUDGET_MS, BATCH));
  if (res.strips.length > 0) {
    sent += res.strips.length;
    postMessage({ type: 'strips', strips: res.strips, count: res.count });
  } else {
    postMessage({ type: 'progress', count: res.count });
  }

  if (res.done) {
    postMessage({ type: 'done', count: res.count });
    stop();
    return;
  }
  setTimeout(pump, 0); // yield so 'cancel' / 'advance' can arrive
}

onmessage = async (e) => {
  const msg = e.data;
  if (msg.cmd === 'start') {
    await ready;
    stop();
    sent = 0;
    wanted = msg.wanted ?? BATCH;
    mode = msg.mode;
    try {
      en = new StripEnumerator(JSON.stringify(msg.graph), msg.cyclic, msg.mode);
    } catch (err) {
      postMessage({ type: 'error', message: String(err) });
      return;
    }
    running = true;
    pump();
  } else if (msg.cmd === 'advance') {
    wanted = Math.max(wanted, msg.wanted);
    if (en && !running) {
      running = true;
      pump();
    }
  } else if (msg.cmd === 'cancel') {
    stop();
  }
};
