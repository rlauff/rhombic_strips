// Strip-search worker: the browser twin of the native worker thread in
// gui.rs. The main thread posts {cmd: 'start' | 'advance' | 'cancel'};
// the worker streams {type: 'strips' | 'progress' | 'done' | 'error'}.
//
// Backpressure mirrors the bounded channel: in enumerate mode the worker
// only searches ahead of the browsing cursor by the lookahead the main
// thread asked for ('advance' raises it), so memory stays proportional to
// how far the user has browsed.
//
// The same worker also runs the Scripts panel's batch jobs ({cmd: 'survey' |
// 'bounds'}, streaming {type: 'survey' | 'bounds'}); app.js uses a separate
// Worker instance for those, so a script and a strip search never share one.

import init, {
  StripEnumerator,
  GraphSurvey,
  BoundaryEnumerator,
} from './pkg/rhombic_strips.js';

const ready = init();

const BUDGET_MS = 30; // per slice, keeps the worker responsive to 'cancel'
const BATCH = 8;      // max strips per slice

let en = null;
let mode = null;
let sent = 0;
let wanted = BATCH;
let running = false;

let script = null;     // GraphSurvey | BoundaryEnumerator
let scriptType = null; // 'survey' | 'bounds'

function stop() {
  if (en) {
    en.free();
    en = null;
  }
  if (script) {
    script.free();
    script = null;
  }
  scriptType = null;
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

// One slice of a Scripts job. Each slice's result (with its `done` flag) is
// forwarded verbatim under the job's own message type; app.js accumulates.
function pumpScript() {
  if (!script) return;
  let res;
  try {
    res = JSON.parse(script.step(BUDGET_MS));
  } catch (err) {
    postMessage({ type: 'error', message: String(err) });
    stop();
    return;
  }
  postMessage({ type: scriptType, ...res });
  if (res.done) {
    stop();
    return;
  }
  setTimeout(pumpScript, 0);
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
  } else if (msg.cmd === 'survey') {
    await ready;
    stop();
    try {
      script = new GraphSurvey(msg.maxN, msg.linear, msg.cyclic);
    } catch (err) {
      postMessage({ type: 'error', message: String(err) });
      return;
    }
    scriptType = 'survey';
    pumpScript();
  } else if (msg.cmd === 'bounds') {
    await ready;
    stop();
    try {
      script = new BoundaryEnumerator(JSON.stringify(msg.graph));
    } catch (err) {
      postMessage({ type: 'error', message: String(err) });
      return;
    }
    scriptType = 'bounds';
    pumpScript();
  } else if (msg.cmd === 'cancel') {
    stop();
  }
};
