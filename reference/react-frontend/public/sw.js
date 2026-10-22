// sw.js - Service Worker for range-safe media streaming
console.log('SW v5 - range-safe media streaming active');

const STREAM_URL_PREFIX = '/stream_media/';
const RANGE_CHUNK_SIZE = 512 * 1024;
const pendingRequests = new Map();

function swLog(level, ...args) {
  const message = args.map(arg => {
    if (arg instanceof Error) return `${arg.message}\n${arg.stack}`;
    if (typeof arg === 'object') {
      try { return JSON.stringify(arg); } catch (_) { return String(arg); }
    }
    return String(arg);
  }).join(' ');

  fetch('http://127.0.0.1:1425/log', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      time: new Date().toLocaleTimeString(),
      level: `SW_${level}`,
      message
    })
  }).catch(() => {});
}

self.addEventListener('install', () => {
  swLog('INFO', 'Install event triggered');
  self.skipWaiting();
});

self.addEventListener('activate', event => {
  swLog('INFO', 'Activate event triggered');
  event.waitUntil(self.clients.claim().then(() => {
    swLog('INFO', 'Clients claimed successfully');
  }));
});

self.addEventListener('message', event => {
  const { type, requestId, error } = event.data || {};
  swLog('DEBUG', `Message received in SW: type=${type}, requestId=${requestId}, hasError=${!!error}`);

  if ((type === 'range_response' || type === 'stream_response') && pendingRequests.has(requestId)) {
    const { resolve, reject } = pendingRequests.get(requestId);
    pendingRequests.delete(requestId);

    if (error) {
      swLog('ERROR', `Rejecting request ${requestId} due to error: ${error}`);
      reject(new Error(error));
    } else {
      resolve(event.data);
    }
  }
});

self.addEventListener('fetch', event => {
  const url = new URL(event.request.url);

  if (url.pathname.startsWith(STREAM_URL_PREFIX)) {
    const parts = url.pathname.split('/');
    const chatId = parts[2];
    const messageId = parts[3];

    if (!chatId || !messageId) {
      return event.respondWith(new Response('Invalid URL', { status: 400 }));
    }

    event.respondWith(handleStreamRequest(event.request, chatId, messageId));
  }
});

function parseRange(rangeHeader, totalSize) {
  if (!rangeHeader) return { start: 0, end: totalSize - 1, hasRange: false };

  const match = rangeHeader.match(/bytes=(\d+)-(\d*)/);
  if (!match) return { start: 0, end: totalSize - 1, hasRange: false };

  const start = Number.parseInt(match[1], 10);
  const requestedEnd = match[2] ? Number.parseInt(match[2], 10) : totalSize - 1;
  const end = Math.min(requestedEnd, totalSize - 1);
  return { start, end, hasRange: true };
}

async function handleStreamRequest(request, chatId, messageId) {
  const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  const client = clients[0];
  if (!client) return new Response('No active client found', { status: 500 });

  const streamId = Math.random().toString(36).slice(2);
  const prepareRes = await requestFromMain(client, {
    type: 'prepare_stream',
    chatId,
    messageId,
    streamId,
  });

  if (!prepareRes) {
    return new Response('Failed to prepare stream', { status: 500 });
  }
  if (prepareRes.error) {
    return new Response(`Failed to prepare stream: ${prepareRes.error}`, { status: 500 });
  }

  const totalSize = Number(prepareRes.totalSize);
  const mimeType = prepareRes.mimeType || 'video/mp4';
  if (!Number.isFinite(totalSize) || totalSize <= 0) {
    postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
    return new Response('Invalid stream metadata', { status: 500 });
  }

  const { start, end, hasRange } = parseRange(request.headers.get('Range'), totalSize);
  if (!Number.isFinite(start) || start < 0 || start >= totalSize || end < start) {
    postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
    return new Response('Range Not Satisfiable', {
      status: 416,
      headers: {
        'Content-Range': `bytes */${totalSize}`,
        'Accept-Ranges': 'bytes',
      },
    });
  }

  const contentLength = end - start + 1;
  let cursor = start;

  const stream = new ReadableStream({
    async pull(controller) {
      if (cursor > end) {
        controller.close();
        postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
        return;
      }

      const length = Math.min(RANGE_CHUNK_SIZE, end - cursor + 1);
      try {
        const res = await requestFromMain(client, {
          type: 'get_range',
          chatId,
          messageId,
          streamId,
          offset: cursor,
          length,
        });

        if (!res) {
          controller.error(new Error('Range retrieval timed out'));
          return;
        }
        if (res.error) {
          controller.error(new Error(res.error));
          return;
        }
        if (!res.chunk) {
          controller.close();
          postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
          return;
        }

        const chunk = new Uint8Array(res.chunk);
        if (chunk.byteLength === 0) {
          controller.close();
          postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
          return;
        }

        const safeChunk = chunk.byteLength > length ? chunk.slice(0, length) : chunk;
        controller.enqueue(safeChunk);
        cursor += safeChunk.byteLength;

        if (cursor > end) {
          controller.close();
          postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
        }
      } catch (error) {
        swLog('ERROR', `Exception during get_range for streamId=${streamId}: ${error.message}`);
        controller.error(error);
      }
    },
    cancel(reason) {
      swLog('INFO', `Stream cancelled for streamId=${streamId}: ${reason}`);
      postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
    },
  });

  const headers = new Headers({
    'Content-Type': mimeType,
    'Accept-Ranges': 'bytes',
    'Content-Length': String(contentLength),
    'Cache-Control': 'no-store',
  });
  if (hasRange) headers.set('Content-Range', `bytes ${start}-${end}/${totalSize}`);

  return new Response(stream, {
    status: hasRange ? 206 : 200,
    headers,
  });
}

function requestFromMain(client, payload) {
  return new Promise((resolve, reject) => {
    const requestId = Math.random().toString(36).slice(2);
    const timeout = setTimeout(() => {
      if (pendingRequests.has(requestId)) {
        pendingRequests.delete(requestId);
        swLog('ERROR', `Timeout waiting for main thread response to ${payload.type} (${requestId})`);
        resolve(null);
      }
    }, 12000);

    pendingRequests.set(requestId, {
      resolve: value => {
        clearTimeout(timeout);
        resolve(value);
      },
      reject: error => {
        clearTimeout(timeout);
        reject(error);
      },
    });

    client.postMessage({ ...payload, requestId });
  });
}

function postToMain(client, payload) {
  try {
    client.postMessage(payload);
  } catch (error) {
    swLog('WARN', `Error posting ${payload.type} to main thread: ${error.message}`);
  }
}
