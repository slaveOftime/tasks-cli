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

    const started = await startServer(storeRoot);
    server = started.server;
    const baseUrl = started.baseUrl;

    const index = await text(`${baseUrl}/`);
    assert.match(index, /tli Kanban/);
    assert.match(index, /\/assets\/app\.css/);
    assert.match(index, /\/assets\/htmx\.js/);
    assert.match(index, /data-dialog-open="create-task-dialog"/);
    assert.doesNotMatch(index, /workspace board/i);

    const appJs = await text(`${baseUrl}/assets/app.js`);
    assert.match(appJs, /data-dialog-open/);
    assert.match(appJs, /showModal\(\)/);
    assert.match(appJs, /data-ready-submit/);
    assert.match(appJs, /data-schedule-form/);
    assert.match(appJs, /data-schedule-ready-at/);

    const htmx = await text(`${baseUrl}/assets/htmx.js`);
    assert.match(htmx, /new URLSearchParams\(\)/);
    assert.match(htmx, /tli:content-updated/);
    assert.doesNotMatch(htmx, /request\('POST'[^;]+new FormData/s);

    const css = await text(`${baseUrl}/assets/app.css`);
    assertResponsiveCss(css);
    assert.match(css, /scrollbar-color:\s*hsl\(var\(--accent\)\)\s*hsl\(var\(--background\)\)/);
    assert.match(css, /\.dialog-card\s*{[\s\S]*overflow-x:\s*hidden/s);
    assert.match(css, /\.dialog-close\s*{[\s\S]*position:\s*absolute;[\s\S]*right:\s*8px/s);
    assert.match(css, /\.schedule-panel\[hidden\]\s*{[\s\S]*display:\s*none/s);
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
    assert.equal(countOccurrences(seedDialog, 'placeholder="optional ready at"'), 1);
    assert.match(seedDialog, /name="ready_at" placeholder="optional ready at" value="2026-05-12T01:55:46\+00:00" data-schedule-ready-at/);

    const scheduledDialog = sliceFrom(board, '<dialog id="manage-scheduled-task"', '</dialog>');
    assert.match(scheduledDialog, /value="clear"/);
    assert.match(scheduledDialog, /name="schedule_mode" value="interval" checked/);
    assert.match(scheduledDialog, /name="every_minutes" type="number" min="1" placeholder="every minutes" value="5"/);
    assert.match(scheduledDialog, /data-schedule-panel="cron" hidden/);
    assert.equal(countOccurrences(scheduledDialog, 'placeholder="optional ready at"'), 1);
    assert.match(scheduledDialog, /name="ready_at" placeholder="optional ready at" value="[^"]*" data-schedule-ready-at/);

    const cronDialog = sliceFrom(board, '<dialog id="manage-cron-task"', '</dialog>');
    assert.match(cronDialog, /value="clear"/);
    assert.match(cronDialog, /name="schedule_mode" value="cron" checked/);
    assert.match(cronDialog, /data-schedule-panel="interval" hidden/);
    assert.match(cronDialog, /name="every_minutes" type="number" min="1" placeholder="every minutes" value="" disabled/);
    assert.match(cronDialog, /data-schedule-panel="cron"/);
    assert.match(cronDialog, /name="cron" placeholder="cron expression" value="0 22 \* \* \*"/);
    assert.equal(countOccurrences(cronDialog, 'placeholder="optional ready at"'), 1);
    assert.match(cronDialog, /name="ready_at" placeholder="optional ready at" value="[^"]*" data-schedule-ready-at/);
    assert.match(board, /class="task-card__head"[\s\S]*<h3>Seed task<\/h3>[\s\S]*class="labels"/);
    assert.match(board, /<time class="task-time" datetime="2026-05-12T01:55:46\+00:00">[^<]+<\/time>/);
    assert.doesNotMatch(board, />2026-05-12T01:55:46\+00:00<\/time>/);
    assert.doesNotMatch(board, /class="status"/);
    assert.doesNotMatch(board, /<details>/);
    assert.doesNotMatch(board, /workspace board/i);

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
    assert.doesNotMatch(uiStarted, /class="status"/);

    const uiReview = await postFormText(`${baseUrl}/ui/tasks/htmx-created/review`, {});
    assert.match(uiReview, /column-review[\s\S]*htmx-created/);
    assert.match(uiReview, /column-review[\s\S]*htmx-created[\s\S]*<button type="submit" disabled aria-disabled="true">Review<\/button>/);

    const uiDone = await postFormText(`${baseUrl}/ui/tasks/htmx-created/done`, {});
    assert.match(uiDone, /column-done[\s\S]*htmx-created/);
    assert.match(uiDone, /column-done[\s\S]*htmx-created[\s\S]*<button type="submit" disabled aria-disabled="true">Done<\/button>/);

    const startedTask = await postJson<TaskRecord>(`${baseUrl}/api/tasks/created-from-e2e/start`, {
      note: 'Picked up in E2E',
    });
    assert.equal(startedTask.status, 'active');

    const state = await json<StateResponse>(`${baseUrl}/api/state`);
    assert.ok(state.counts.ready >= 1);
    assert.ok(state.counts.active >= 1);

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
  assert.match(css, /@media \(min-width: 1280px\)/);
  assert.match(css, /\.kanban\s*{[^}]*grid-template-columns:\s*repeat\(auto-fit,\s*minmax\(260px,\s*1fr\)\)/s);
  assert.match(css, /@media \(max-width: 640px\)[\s\S]*\.kanban\s*{[\s\S]*flex-direction:\s*column/s);
  assert.match(css, /@media \(min-width: 1280px\)[\s\S]*grid-template-columns:\s*repeat\(7,\s*minmax\(0,\s*1fr\)\)/s);
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
