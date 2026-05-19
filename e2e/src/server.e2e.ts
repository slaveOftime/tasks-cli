import assert from 'node:assert/strict';
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, mkdir, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

type StateResponse = {
  counts: {
    ready: number;
    todo: number;
    active: number;
    blocked: number;
    review: number;
    done: number;
  };
};

type TaskRecord = {
  id: string;
  title: string;
  status: string;
  labels: string[];
};

const repoRoot = resolve(import.meta.dirname, '..', '..');
const binary = resolve(repoRoot, 'target', 'debug', process.platform === 'win32' ? 'tli.exe' : 'tli');
const cssPath = resolve(repoRoot, 'src', 'server_assets', 'app.css');

async function main(): Promise<void> {
  const temp = await mkdtemp(join(tmpdir(), 'tli-e2e-'));
  const storeRoot = join(temp, '.tli');
  let server: ChildProcess | undefined;

  try {
    await mkdir(storeRoot);
    await run(binary, [
      '--root',
      storeRoot,
      'add',
      'Seed task',
      '--id',
      'seed-task',
      '--summary',
      'Seeded from TypeScript E2E',
      '--ready-at',
      '2026-05-12T01:55:46Z',
      '--label',
      'e2e',
    ]);
    await run(binary, [
      '--root',
      storeRoot,
      'add',
      'Scheduled task',
      '--id',
      'scheduled-task',
      '--every-minutes',
      '5',
    ]);
    await run(binary, [
      '--root',
      storeRoot,
      'add',
      'Cron task',
      '--id',
      'cron-task',
      '--cron',
      '0 22 * * *',
    ]);
    await run(binary, ['--root', storeRoot, 'add', 'Depends task', '--id', 'depends-task']);
    await run(binary, ['--root', storeRoot, 'dep', 'add', 'depends-task', 'seed-task']);
    for (let index = 1; index <= 16; index += 1) {
      const id = `done-overflow-${String(index).padStart(2, '0')}`;
      await run(binary, ['--root', storeRoot, 'add', `Done overflow ${index}`, '--id', id]);
      await run(binary, ['--root', storeRoot, 'start', id]);
      await run(binary, ['--root', storeRoot, 'done', id, '--note', `C:\\repo\\very\\long\\path\\${'segment\\'.repeat(8)}file-${index}.ts`]);
    }

    const started = await startServer(storeRoot);
    server = started.server;
    const baseUrl = started.baseUrl;

    const index = await text(`${baseUrl}/`);
    assert.match(index, /Tasks Kanban/);
    assert.match(index, /href="assets\/app\.css"/);
    assert.match(index, /src="assets\/htmx\.js"/);
    assert.match(index, /data-dialog-open="create-task-dialog"/);
    assert.match(index, /<form class="topbar-search board-search" role="search" hx-get="ui\/board" hx-target="#board" hx-swap="outerHTML">/);
    assert.match(index, /<input type="search" name="query" value="" placeholder="Search titles, ids, labels" aria-label="Search tasks" autocomplete="off">/);
    assert.doesNotMatch(index, /repo-local task management/);
    assert.doesNotMatch(index, /workspace board/i);

    const appJs = await text(`${baseUrl}/assets/app.js`);
    assert.match(appJs, /data-dialog-open/);
    assert.match(appJs, /showModal\(\)/);
    assert.match(appJs, /data-ready-submit/);
    assert.match(appJs, /data-schedule-form/);
    assert.match(appJs, /data-scroll-top/);
    assert.match(appJs, /document\.querySelector\('\.kanban'\)/);
    assert.match(appJs, /scrollContainer\.addEventListener\('scroll', syncScrollTopButton, \{ passive: true \}\)/);
    assert.match(appJs, /scrollContainer\.scrollTo\(\{ top: 0, left: 0, behavior: 'smooth' \}\)/);
    assert.match(appJs, /window\.scrollTo\(\{ top: 0, behavior: 'smooth' }/);
    assert.doesNotMatch(appJs, /document\.getElementById\('board'\)\.addEventListener\('scroll'/);

    const htmx = await text(`${baseUrl}/assets/htmx.js`);
    assert.match(htmx, /new URLSearchParams\(\)/);
    assert.match(htmx, /tli:content-updated/);
    assert.match(htmx, /form\[hx-post], form\[hx-get]/);
    assert.match(htmx, /document\.addEventListener\('input'/);
    assert.match(htmx, /document\.addEventListener\('search'/);
    assert.match(htmx, /var pendingRequests = new Map\(\)/);
    assert.match(htmx, /prior && prior\.controller\) prior\.controller\.abort\(\)/);
    assert.match(htmx, /!current \|\| current\.token !== token/);
    assert.match(htmx, /var nextTarget = document\.querySelector\(selector\)/);
    assert.match(htmx, /if \(error && error\.name === 'AbortError'\) return;/);
    assert.match(htmx, /\[hx-get\]:not\(form\):not\(\[hx-trigger~="load"\]\)/);
    assert.match(htmx, /isFormControl\(event\.target\)/);
    assert.doesNotMatch(htmx, /request\('POST'[^;]+new FormData/s);

    const css = await text(`${baseUrl}/assets/app.css`);
    assertResponsiveCss(css);
    assert.match(css, /scrollbar-color:\s*hsl\(var\(--accent\)\)\s*hsl\(var\(--background\)\)/);
    assert.match(css, /\.app-dialog\s*{[\s\S]*max-height:\s*min\(780px,\s*calc\(100vh - 24px\)\)[\s\S]*overflow:\s*visible/s);
    assert.match(css, /^\.dialog-card\s*{[\s\S]*max-height:\s*min\(780px,\s*calc\(100vh - 24px\)\)[\s\S]*overflow:\s*hidden/sm);
    assert.match(css, /\.dialog-content\s*{[\s\S]*overflow-y:\s*auto/s);
    assert.match(css, /^\.dialog-close\s*{[\s\S]*position:\s*absolute;[\s\S]*right:\s*8px;[\s\S]*background:\s*transparent;[\s\S]*border:\s*1px solid transparent/sm);
    assert.doesNotMatch(css, /@media \(max-width: 640px\)[\s\S]*\.dialog-card > header\s*{[\s\S]*flex-direction:\s*column/s);
    assert.match(css, /input,\s*textarea\s*{[\s\S]*font-size:\s*16px/s);
    assert.match(css, /\.schedule-panel\[hidden\]\s*{[\s\S]*display:\s*none/s);
    assert.match(css, /\.schedule-clear-form\s*{[\s\S]*margin-top:\s*8px/s);
    assert.match(css, /\.task-card__head\s*{[\s\S]*flex-direction:\s*column/s);
    assert.match(css, /\.task-card__id\s*{[\s\S]*text-align:\s*left;[\s\S]*overflow-wrap:\s*anywhere/s);
    assert.match(css, /\.task-time\s*{[\s\S]*text-align:\s*center/s);
    assert.match(css, /\.event-time\s*{[\s\S]*font-family:\s*ui-monospace/s);
    assert.match(css, /\.event-message\s*{[\s\S]*text-align:\s*left;[\s\S]*overflow-wrap:\s*anywhere/s);
    assert.match(css, /\.column-pagination\s*{[\s\S]*justify-content:\s*space-between/s);
    assert.match(css, /\.metrics\s*{[\s\S]*display:\s*none/s);
    assert.match(css, /\.metrics__link\s*{[\s\S]*text-decoration:\s*none[\s\S]*text-transform:\s*uppercase[\s\S]*font-family:\s*ui-monospace/s);
    assert.match(css, /\.metrics__link span\s*{[\s\S]*justify-content:\s*space-between[\s\S]*width:\s*100%/s);
    assert.match(css, /\.metrics__link strong\s*{[\s\S]*border-left:\s*1px solid hsl\(var\(--status-color,\s*var\(--border\)\)\s*\/\s*\.22\)[\s\S]*font-variant-numeric:\s*tabular-nums/s);
    assert.match(css, /--status-ready:\s*196 88% 62%/);
    assert.match(css, /\.metrics__link--ready,\s*\.column-ready,\s*\.task-card--ready\s*{[\s\S]*--status-color:\s*var\(--status-ready\)/s);
    assert.match(css, /\.metrics__link\[class\*="metrics__link--"\]::before\s*{[\s\S]*width:\s*2px[\s\S]*background:\s*linear-gradient\(180deg,\s*hsl\(var\(--status-color\)\),\s*hsl\(var\(--status-color\)\s*\/\s*\.2\)\)/s);
    assert.match(css, /\.column\s*{[\s\S]*border-top-width:\s*3px[\s\S]*border-top-color:\s*hsl\(var\(--status-color\)\s*\/\s*\.72\)/s);
    assert.doesNotMatch(css, /\.status-chip\s*{/);
    assert.match(css, /\.scroll-top\s*{[\s\S]*position:\s*fixed;[\s\S]*border-radius:\s*999px/s);
    assert.match(css, /\.scroll-top\[data-visible="true"\]\s*{[\s\S]*pointer-events:\s*auto/s);
    assert.match(css, /\.board-search\s*{[\s\S]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s*auto/s);
    assert.match(css, /\.topbar-search\s*{[\s\S]*flex:\s*1 1 500px[\s\S]*max-width:\s*560px/s);
    assert.match(css, /@media \(max-width: 920px\)[\s\S]*\.topbar-search\s*{[\s\S]*display:\s*none/s);
    assert.match(css, /@media \(min-width: 921px\)[\s\S]*\.board-toolbar\s*{[\s\S]*display:\s*none/s);
    assert.match(css, /\.board-toolbar\s*{[\s\S]*margin-bottom:\s*12px/s);
    assert.doesNotMatch(css, /@media \(max-width: 920px\)\s*{[^@]*\.board-toolbar\s*{[^}]*padding:/s);
    assert.match(css, /\.board-search__actions\s*{[\s\S]*display:\s*inline-flex[\s\S]*flex-wrap:\s*nowrap/s);
    assert.match(css, /\.board-search__field input\s*{[\s\S]*(?:min-)?height:\s*32px[\s\S]*font-size:\s*16px/s);
    assert.match(css, /\.board-search__actions button\s*{[\s\S]*(?:min-)?height:\s*32px/s);
    assert.match(css, /\.board-search__summary\s*{[\s\S]*grid-column:\s*1 \/ -1/s);
    assert.match(css, /\.task-card\s*{[\s\S]*--task-detail-label-width:\s*clamp\(9ch,\s*28%,\s*16ch\)/s);
    assert.match(css, /\.detail-row,\s*\.events li\s*{[\s\S]*display:\s*flex[\s\S]*flex-wrap:\s*wrap/s);
    assert.match(css, /\.detail-row__label\s*{[\s\S]*color:\s*hsl\(var\(--muted-foreground\)\)[\s\S]*letter-spacing:\s*\.06em[\s\S]*text-transform:\s*uppercase/s);
    assert.match(css, /\.detail-row__label--warning\s*{[\s\S]*color:\s*hsl\(var\(--status-checkpoint\)\)/s);
    assert.match(css, /\.detail-row__value\s*{[\s\S]*text-align:\s*left;[\s\S]*overflow-wrap:\s*anywhere/s);
    assert.match(css, /\.event-kind\s*{[\s\S]*color:\s*hsl\(var\(--muted-foreground\)\)[\s\S]*letter-spacing:\s*\.06em[\s\S]*text-transform:\s*uppercase/s);
    assert.equal(css, await readFile(cssPath, 'utf8'));

    const board = await text(`${baseUrl}/ui/board`);
    assert.match(board, /Seed task/);
    assert.match(board, /Seeded from TypeScript E2E/);
    assert.match(board, /Scheduled task/);
    assert.match(board, /Cron task/);
    assert.match(board, /<dialog id="create-task-dialog" class="app-dialog">/);
    assert.match(board, /class="dialog-close" data-dialog-close aria-label="Close dialog">&times;<\/button>/);
    assert.doesNotMatch(board, />Close<\/button>/);
    assert.match(board, /data-dialog-open="manage-seed-task"/);
    assert.match(board, /<dialog id="manage-seed-task" class="app-dialog">/);
    assert.match(board, /data-ready-submit hidden/);
    assert.match(board, /class="toggle-group" role="radiogroup" aria-label="Schedule mode"/);
    assert.match(board, /name="schedule_mode" value="interval" checked/);
    const seedDialog = sliceFrom(board, '<dialog id="manage-seed-task"', '</dialog>');
    assert.doesNotMatch(seedDialog, /value="clear"/);
    assert.match(seedDialog, /data-schedule-panel="interval"/);
    assert.match(seedDialog, /data-schedule-panel="cron" hidden/);
    assert.match(seedDialog, /name="cron" placeholder="cron expression" value="" disabled/);
    assert.match(seedDialog, /<textarea name="note" placeholder="checkpoint note"><\/textarea>/);
    assert.match(seedDialog, /<textarea name="next_step" placeholder="next step"><\/textarea>/);
    assert.match(seedDialog, /<textarea name="reason" required placeholder="blocked reason"><\/textarea>/);
    assert.match(seedDialog, /<textarea name="text" required placeholder="note"><\/textarea>/);
    assert.match(seedDialog, /<input name="next_task" placeholder="next task id">/);
    assert.match(seedDialog, /<input name="dependency" required placeholder="dependency id">/);
    assert.equal(countOccurrences(seedDialog, 'placeholder="optional ready at"'), 1);
    assert.match(seedDialog, /name="ready_at" type="datetime-local" step="1" placeholder="optional ready at" aria-label="Next ready at" value="\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}" data-schedule-ready-at/);

    const scheduledDialog = sliceFrom(board, '<dialog id="manage-scheduled-task"', '</dialog>');
    assert.match(scheduledDialog, /class="schedule-clear-form"/);
    assert.match(scheduledDialog, /<input type="hidden" name="clear" value="true">/);
    assert.match(scheduledDialog, />Clear schedule<\/button>/);
    assert.match(scheduledDialog, /name="schedule_mode" value="interval" checked/);
    assert.match(scheduledDialog, /name="every_minutes" type="number" min="1" placeholder="every minutes" value="5"/);
    assert.match(scheduledDialog, /data-schedule-panel="cron" hidden/);
    assert.equal(countOccurrences(scheduledDialog, 'placeholder="optional ready at"'), 1);
    assert.match(scheduledDialog, /name="ready_at" type="datetime-local" step="1" placeholder="optional ready at" aria-label="Next ready at" value="[^"]*" data-schedule-ready-at/);

    const cronDialog = sliceFrom(board, '<dialog id="manage-cron-task"', '</dialog>');
    assert.match(cronDialog, /class="schedule-clear-form"/);
    assert.match(cronDialog, /name="schedule_mode" value="cron" checked/);
    assert.match(cronDialog, /data-schedule-panel="interval" hidden/);
    assert.match(cronDialog, /name="every_minutes" type="number" min="1" placeholder="every minutes" value="" disabled/);
    assert.match(cronDialog, /data-schedule-panel="cron"/);
    assert.match(cronDialog, /name="cron" placeholder="cron expression" value="0 22 \* \* \*"/);
    assert.equal(countOccurrences(cronDialog, 'placeholder="optional ready at"'), 1);
    assert.match(cronDialog, /name="ready_at" type="datetime-local" step="1" placeholder="optional ready at" aria-label="Next ready at" value="[^"]*" data-schedule-ready-at/);
    assert.match(board, /class="task-card__head"[\s\S]*<h3>Seed task<\/h3>[\s\S]*class="labels"/);
    assert.match(board, /<time class="task-time" datetime="2026-05-12T01:55:46\+00:00">[^<]+<\/time>/);
    assert.doesNotMatch(board, />2026-05-12T01:55:46\+00:00<\/time>/);
    assert.match(board, /<code class="task-card__id">seed-task<\/code>/);
    assert.match(board, /<nav class="metrics" aria-label="Task status summary">/);
    assert.match(board, /href="#status-ready" aria-label="Jump to ready tasks"/);
    assert.match(board, /href="#status-checkpoint" aria-label="Jump to checkpoint tasks"/);
    assert.match(board, /<a class="metrics__link metrics__link--ready" href="#status-ready" aria-label="Jump to ready tasks"><span>Ready <strong>\d+<\/strong><\/span><\/a>/);
    assert.match(board, /<section id="status-ready" class="column column-ready">/);
    assert.match(board, /<section id="status-done" class="column column-done">/);
    assert.match(board, /<article class="task-card task-card--ready">/);
    assert.doesNotMatch(board, /status-chip/);
    assert.match(board, /data-scroll-top data-visible="false" aria-label="Scroll to top"/);
    assert.match(board, /class="scroll-top__icon" aria-hidden="true">&uarr;<\/span>/);
    assert.match(board, /class="board-search__actions"/);
    assert.match(board, /<section class="board-toolbar">/);
    assert.doesNotMatch(board, /<section class="panel board-toolbar">/);
    assert.match(board, /<form class="board-search" role="search" hx-get="ui\/board" hx-target="#board" hx-swap="outerHTML">/);
    assert.match(board, /type="search" name="query" value="" placeholder="Search titles, ids, labels" aria-label="Search tasks"/);
    assert.doesNotMatch(board, /type="search"[^>]*hx-trigger=/);
    assert.match(board, /<p class="meta detail-row"><span class="detail-row__label">schedule<\/span><span class="detail-row__value">every 5m<\/span><\/p>/);
    assert.match(board, /<p class="meta detail-row"><span class="detail-row__label detail-row__label--warning">depends on<\/span><span class="detail-row__value">seed-task<\/span><\/p>/);
    assert.doesNotMatch(board, /<span class="eyebrow">Search<\/span>/);
    assert.doesNotMatch(board, /Filter tasks across every column\./);
    assert.match(board, /<li><div class="event-kind">completed<time class="event-time" datetime="[^"]+">\([^)]+\)<\/time><\/div>/);
    assert.match(board, /column-pagination/);
    assert.match(board, /hx-get="ui\/board\?done_page=2"/);
    assert.match(board, /Page 1 of 2 · 1-15 of 16/);
    assert.doesNotMatch(board, /class="status"/);
    assert.doesNotMatch(board, /<details>/);
    assert.doesNotMatch(board, /workspace board/i);

    const donePageTwo = await text(`${baseUrl}/ui/board?done_page=2`);
    assert.match(donePageTwo, /Page 2 of 2 · 16-16 of 16/);
    assert.match(donePageTwo, /done-overflow-01/);
    assert.doesNotMatch(donePageTwo, /done-overflow-16/);

    const created = await postJson<TaskRecord>(`${baseUrl}/api/tasks`, {
      title: 'Created from JSON-compatible form',
      id: 'created-from-e2e',
      summary: 'Created through the server API',
      labels: 'web,e2e',
    });
    assert.equal(created.id, 'created-from-e2e');
    assert.deepEqual(created.labels, ['e2e', 'web']);

    const addResponse = await postFormText(`${baseUrl}/ui/tasks`, {
      title: 'Created through HTMX form',
      id: 'htmx-created',
      summary: 'Created through the UI fragment endpoint',
      labels: 'mobile',
    });
    assert.match(addResponse, /htmx-created/);
    assert.match(addResponse, /Created through HTMX form/);

    const uiStarted = await postFormText(`${baseUrl}/ui/tasks/htmx-created/start`, {});
    assert.match(uiStarted, /column-active[\s\S]*htmx-created/);
    assert.match(uiStarted, /column-active[\s\S]*htmx-created[\s\S]*<button type="submit" disabled aria-disabled="true">Start<\/button>/);
    assert.doesNotMatch(uiStarted, /status-chip/);
    assert.doesNotMatch(uiStarted, /class="status"/);

    const uiReview = await postFormText(`${baseUrl}/ui/tasks/htmx-created/review`, {});
    assert.match(uiReview, /column-review[\s\S]*htmx-created/);
    assert.match(uiReview, /column-review[\s\S]*htmx-created[\s\S]*<button type="submit" disabled aria-disabled="true">Review<\/button>/);
    assert.doesNotMatch(uiReview, /status-chip/);

    const uiDone = await postFormText(`${baseUrl}/ui/tasks/htmx-created/done`, {});
    assert.match(uiDone, /column-done[\s\S]*htmx-created/);
    const doneCard = sliceTaskCard(uiDone, 'htmx-created');
    assert.doesNotMatch(doneCard, /<div class="actions">/);
    assert.doesNotMatch(doneCard, /<button[\s\S]*>Start<\/button>/);
    assert.doesNotMatch(doneCard, /<button[\s\S]*>Review<\/button>/);
    assert.doesNotMatch(doneCard, /<button[\s\S]*>Done<\/button>/);
    assert.doesNotMatch(doneCard, /data-dialog-open="manage-htmx-created"/);
    assert.doesNotMatch(uiDone, /status-chip/);

    const startedTask = await postJson<TaskRecord>(`${baseUrl}/api/tasks/created-from-e2e/start`, {
      note: 'Picked up in E2E',
    });
    assert.equal(startedTask.status, 'active');

    const state = await json<StateResponse>(`${baseUrl}/api/state`);
    assert.ok(state.counts.ready >= 1);
    assert.ok(state.counts.active >= 1);

    const searchResults = await text(`${baseUrl}/ui/board?query=seed`);
    assert.match(searchResults, /class="board-search__summary">1 matching task for &quot;seed&quot;\./);
    assert.doesNotMatch(searchResults, /Filter tasks across every column\./);

    const events = await json<Array<{ message: string }>>(`${baseUrl}/api/events`);
    assert.ok(events.some((event) => event.message.includes('task created')));
  } finally {
    if (server) {
      server.kill();
      await onceExit(server);
    }
    await rm(temp, { recursive: true, force: true });
  }
}

function assertResponsiveCss(css: string): void {
  assert.match(css, /@media \(max-width: 640px\)/);
  assert.match(css, /\.kanban\s*{[^}]*grid-template-columns:\s*repeat\(7,\s*minmax\(280px,\s*1fr\)\)[^}]*overflow:\s*auto/s);
  assert.match(css, /@media \(max-width: 920px\)[\s\S]*\.kanban\s*{[\s\S]*max-height:\s*calc\(100vh - 176px\)/s);
  assert.match(css, /@media \(max-width: 640px\)[\s\S]*\.kanban\s*{[\s\S]*flex-direction:\s*column[\s\S]*max-height:\s*none[\s\S]*overflow:\s*visible/s);
  assert.match(css, /@media \(max-width: 640px\)[\s\S]*\.metrics\s*{[\s\S]*grid-template-columns:\s*repeat\(3,\s*minmax\(0,\s*1fr\)\)/s);
}

function sliceFrom(value: string, start: string, end: string): string {
  const startIndex = value.indexOf(start);
  assert.notEqual(startIndex, -1, `missing start marker ${start}`);
  const endIndex = value.indexOf(end, startIndex);
  assert.notEqual(endIndex, -1, `missing end marker ${end}`);
  return value.slice(startIndex, endIndex + end.length);
}

function countOccurrences(value: string, needle: string): number {
  return value.split(needle).length - 1;
}

function sliceTaskCard(value: string, taskId: string): string {
  const idMarker = `<code class="task-card__id">${taskId}</code>`;
  const idIndex = value.indexOf(idMarker);
  assert.notEqual(idIndex, -1, `missing task card id ${taskId}`);
  const startIndex = value.lastIndexOf('<article ', idIndex);
  assert.notEqual(startIndex, -1, `missing task card start ${taskId}`);
  const endIndex = value.indexOf('</article>', idIndex);
  assert.notEqual(endIndex, -1, `missing task card end ${taskId}`);
  return value.slice(startIndex, endIndex + '</article>'.length);
}

async function startServer(storeRoot: string): Promise<{
  server: ChildProcess;
  baseUrl: string;
}> {
  const server = spawn(binary, ['--root', storeRoot, 'server', 'start', '--port', '0'], {
    cwd: repoRoot,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  let stderr = '';
  server.stderr?.on('data', (chunk: Buffer) => {
    stderr += chunk.toString();
  });

  const baseUrl = await new Promise<string>((resolveBaseUrl, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error(`server did not start; stderr=${stderr}`));
    }, 10_000);

    server.stdout?.on('data', (chunk: Buffer) => {
      const output = chunk.toString();
      const match = output.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (!match) return;
      clearTimeout(timeout);
      resolveBaseUrl(match[0]);
    });

    server.once('exit', (code) => {
      clearTimeout(timeout);
      reject(new Error(`server exited before listening with code ${code}; stderr=${stderr}`));
    });
  });

  return { server, baseUrl };
}

async function run(command: string, args: string[]): Promise<void> {
  await new Promise<void>((resolveRun, reject) => {
    const child = spawn(command, args, { cwd: repoRoot, stdio: ['ignore', 'pipe', 'pipe'] });
    let stderr = '';
    child.stderr.on('data', (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.once('exit', (code) => {
      if (code === 0) {
        resolveRun();
      } else {
        reject(new Error(`${command} ${args.join(' ')} failed with ${code}: ${stderr}`));
      }
    });
  });
}

async function json<T>(url: string): Promise<T> {
  const response = await fetch(url);
  await assertOk(response, url);
  return (await response.json()) as T;
}

async function text(url: string): Promise<string> {
  const response = await fetch(url);
  await assertOk(response, url);
  return response.text();
}

async function postJson<T>(url: string, values: Record<string, string>): Promise<T> {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(values),
  });
  await assertOk(response, url);
  return (await response.json()) as T;
}

async function postFormText(url: string, values: Record<string, string>): Promise<string> {
  const response = await fetch(url, { method: 'POST', body: form(values) });
  await assertOk(response, url);
  return response.text();
}

async function assertOk(response: Response, url: string): Promise<void> {
  if (!response.ok) {
    assert.fail(`${url} returned ${response.status}: ${await response.text()}`);
  }
}

function form(values: Record<string, string>): URLSearchParams {
  const body = new URLSearchParams();
  for (const [key, value] of Object.entries(values)) {
    body.set(key, value);
  }
  return body;
}

async function onceExit(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise<void>((resolveExit) => {
    child.once('exit', () => resolveExit());
  });
}

await main();
