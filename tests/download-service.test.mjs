import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { createServer } from 'vite';

const stored = new Map();
globalThis.window = new EventTarget();
globalThis.localStorage = {
  getItem: key => stored.get(key) ?? null,
  setItem: (key, value) => stored.set(key, value),
  removeItem: key => stored.delete(key),
};

let server;
let downloads;

before(async () => {
  server = await createServer({
    configFile: false,
    appType: 'custom',
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true, hmr: false, ws: false },
  });
  ({ downloadService: downloads } = await server.ssrLoadModule('/src/features/downloader/DownloadService.ts'));
});

after(async () => {
  await server?.close();
});

test('retry keeps one item and finishes on the same ID', async () => {
  const id = 'retry-completes';
  downloads.addDownload({ id, fileName: 'one.mp4', status: 'failed', progress: 0 }, {
    retry: async () => downloads.updateDownload(id, { status: 'completed', progress: 100 }),
  });

  assert.equal(await downloads.retryDownload(id), true);
  assert.equal(downloads.getDownload(id)?.status, 'completed');
  assert.equal(downloads.activeDownloads.has(id), true);
  assert.equal(downloads.activeDownloads.size, 1);
  downloads.removeDownload(id);
});

test('retry that never reports completion becomes a failure', async () => {
  const id = 'retry-stalls';
  downloads.addDownload({ id, fileName: 'two.mp4', status: 'failed', progress: 0 }, {
    retry: async () => {},
  });

  assert.equal(await downloads.retryDownload(id), false);
  assert.equal(downloads.getDownload(id)?.status, 'failed');
  downloads.removeDownload(id);
});

test('late progress and completion cannot revive a canceled item', () => {
  const id = 'cancel-late-event';
  downloads.addDownload({ id, fileName: 'three.mp4', status: 'downloading', progress: 0 });
  downloads.updateDownload(id, { status: 'canceled', progress: 0 });
  downloads.updateDownload(id, { progress: 80 });
  downloads.updateDownload(id, { status: 'completed', progress: 100 });

  assert.equal(downloads.getDownload(id)?.status, 'canceled');
  assert.equal(downloads.getDownload(id)?.progress, 0);
  downloads.removeDownload(id);
});

test('a full localStorage does not interrupt download state updates', () => {
  const id = 'storage-full';
  const originalSetItem = globalThis.localStorage.setItem;
  globalThis.localStorage.setItem = () => { throw new Error('QuotaExceededError'); };
  try {
    downloads.addDownload({ id, fileName: 'four.mp4', status: 'downloading', progress: 0 });
    assert.doesNotThrow(() => downloads.updateDownload(id, { status: 'completed', progress: 100 }, true));
    assert.equal(downloads.getDownload(id)?.status, 'completed');
  } finally {
    globalThis.localStorage.setItem = originalSetItem;
    downloads.removeDownload(id);
  }
});
