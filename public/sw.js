// sw.js - Service Worker for Media Streaming
console.log('SW v4 - Real-time debug logging active');

const STREAM_URL_PREFIX = '/stream_media/';
const pendingRequests = new Map();

function swLog(level, ...args) {
  const message = args.map(arg => {
    if (arg instanceof Error) {
      return `${arg.message}\n${arg.stack}`;
    }
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

self.addEventListener('install', (event) => {
  swLog('INFO', 'Install event triggered');
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  swLog('INFO', 'Activate event triggered');
  event.waitUntil(self.clients.claim().then(() => {
    swLog('INFO', 'Clients claimed successfully');
  }));
});

self.addEventListener('message', (event) => {
  const { type, requestId, error } = event.data;
  const { chunk, done } = event.data;
  swLog('DEBUG', `Message received in SW: type=${type}, requestId=${requestId}, done=${done}, hasChunk=${!!chunk}, hasError=${!!error}`);
  
  if (type === 'chunk_response' && pendingRequests.has(requestId)) {
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

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);

  if (url.pathname.startsWith(STREAM_URL_PREFIX)) {
    const parts = url.pathname.split('/');
    const chatId = parts[2];
    const messageId = parts[3];

    swLog('INFO', `Intercepted stream request: ${url.pathname} (chatId=${chatId}, msgId=${messageId})`);

    if (!chatId || !messageId) {
      swLog('WARN', 'Invalid stream URL format');
      return event.respondWith(new Response('Invalid URL', { status: 400 }));
    }

    event.respondWith(handleStreamRequest(event.request, chatId, messageId));
  }
});

async function handleStreamRequest(request, chatId, messageId) {
  swLog('INFO', `Starting stream request handler for ${chatId}/${messageId}`);
  const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
  swLog('DEBUG', `Found window clients: count=${clients.length}`);
  const client = clients[0];
  
  if (!client) {
    swLog('ERROR', 'No active client window found to retrieve chunks from!');
    return new Response('No active client found', { status: 500 });
  }

  const rangeHeader = request.headers.get('Range');
  swLog('INFO', `Range header: ${rangeHeader}`);
  
  let requestedStart = 0;
  let requestedEnd = null;
  if (rangeHeader) {
    const match = rangeHeader.match(/bytes=(\d+)-(\d*)/);
    if (match) {
      requestedStart = parseInt(match[1], 10);
      if (match[2]) {
        requestedEnd = parseInt(match[2], 10);
      }
      swLog('DEBUG', `Parsed range bytes: start=${requestedStart}, end=${requestedEnd}`);
    }
  }

  // Generate a unique ID for this specific video stream instance
  const streamId = Math.random().toString(36).substring(7);
  swLog('DEBUG', `Generated streamId=${streamId} for range request starting at offset=${requestedStart}`);

  // Initialize stream on main thread with offset
  swLog('DEBUG', `Requesting init_stream from main thread for streamId=${streamId}`);
  const initRes = await requestFromMain(client, { 
    type: 'init_stream', 
    chatId, 
    messageId, 
    offset: requestedStart, 
    streamId 
  });
  
  if (!initRes) {
    swLog('ERROR', `No response received from main thread for init_stream request ${streamId}`);
    return new Response('Failed to init stream (timeout/no response)', { status: 500 });
  }
  
  if (initRes.error) {
    swLog('ERROR', `Failed to init stream on main thread for streamId=${streamId}: ${initRes.error}`);
    return new Response(`Failed to init stream: ${initRes.error}`, { status: 500 });
  }

  const totalSize = Number(initRes.totalSize);
  const { mimeType } = initRes;
  swLog('INFO', `Main thread initialized stream: totalSize=${totalSize}, mimeType=${mimeType}`);

  if (!Number.isFinite(totalSize) || totalSize <= 0) {
    swLog('ERROR', `Invalid stream metadata received for streamId=${streamId}: totalSize=${totalSize}, mimeType=${mimeType}`);
    postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
    return new Response('Invalid stream metadata', { status: 500 });
  }
  
  const actualEnd = requestedEnd !== null ? requestedEnd : totalSize - 1;
  const contentLength = actualEnd - requestedStart + 1;
  swLog('INFO', `Stream dimensions: actualEnd=${actualEnd}, contentLength=${contentLength}`);

  let bytesWritten = 0;

  const stream = new ReadableStream({
    async pull(controller) {
      try {
        swLog('DEBUG', `Stream pull requested for streamId=${streamId}, bytesWritten=${bytesWritten}/${contentLength}`);
        const res = await requestFromMain(client, { type: 'get_chunk', chatId, messageId, streamId });
        
        if (!res) {
          swLog('ERROR', `No response received from main thread for get_chunk streamId=${streamId}`);
          controller.error(new Error('Chunk retrieval timed out/failed'));
          return;
        }
        
        if (res.error) {
          swLog('ERROR', `Main thread returned error for get_chunk streamId=${streamId}: ${res.error}`);
          controller.error(new Error(res.error));
          return;
        }

        if (res.done) {
          swLog('INFO', `Stream completed (done=true) from main thread for streamId=${streamId}`);
          controller.close();
        } else if (res.chunk) {
          const chunk = new Uint8Array(res.chunk);
          const bytesLeft = contentLength - bytesWritten;
          swLog('DEBUG', `Received chunk: size=${chunk.length} bytes, bytesLeft=${bytesLeft} bytes`);
          
          if (chunk.length <= bytesLeft) {
            controller.enqueue(chunk);
            bytesWritten += chunk.length;
            swLog('DEBUG', `Enqueued full chunk. Total written=${bytesWritten}`);
          } else {
            // Slice the chunk to exactly match the requested contentLength
            const sliced = chunk.slice(0, bytesLeft);
            controller.enqueue(sliced);
            bytesWritten += bytesLeft;
            swLog('INFO', `Enqueued sliced chunk (bytesLeft reached). Total written=${bytesWritten}. Closing stream.`);
            controller.close();
            // Cancel the stream on main thread since we reached the end of the Range
            postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
          }
        } else {
          swLog('WARN', 'get_chunk returned neither done nor chunk! Closing stream.');
          controller.close();
        }
      } catch (err) {
        swLog('ERROR', `Exception during stream pull for streamId=${streamId}: ${err.message}`);
        controller.error(err);
      }
    },
    cancel(reason) {
      swLog('INFO', `Stream cancelled by browser/consumer for streamId=${streamId}, reason=${reason}`);
      postToMain(client, { type: 'cancel_stream', chatId, messageId, streamId });
    }
  });

  const headers = new Headers({
    'Content-Type': mimeType || 'video/mp4',
    'Accept-Ranges': 'bytes',
    'Content-Length': `${contentLength}`
  });

  if (rangeHeader) {
    headers.set('Content-Range', `bytes ${requestedStart}-${actualEnd}/${totalSize}`);
  }

  swLog('INFO', `Responding with status ${rangeHeader ? 206 : 200}, headers: Content-Range=${headers.get('Content-Range')}, Content-Length=${headers.get('Content-Length')}`);

  return new Response(stream, {
    status: rangeHeader ? 206 : 200,
    headers
  });
}

function requestFromMain(client, payload) {
  return new Promise((resolve, reject) => {
    const requestId = Math.random().toString(36).substring(7);
    
    // Set a timeout of 10 seconds to avoid hanging indefinitely if main thread doesn't reply
    const timeout = setTimeout(() => {
      if (pendingRequests.has(requestId)) {
        pendingRequests.delete(requestId);
        swLog('ERROR', `Timeout waiting for main thread response to ${payload.type} (requestId=${requestId})`);
        resolve(null); // Resolve with null to let the handler handle it
      }
    }, 10000);

    const resolveWithCleanup = (val) => {
      clearTimeout(timeout);
      resolve(val);
    };

    const rejectWithCleanup = (err) => {
      clearTimeout(timeout);
      reject(err);
    };

    pendingRequests.set(requestId, {
      resolve: resolveWithCleanup,
      reject: rejectWithCleanup
    });

    client.postMessage({ ...payload, requestId });
  });
}

function postToMain(client, payload) {
  try {
    client.postMessage(payload);
  } catch (e) {
    swLog('WARN', `Error posting ${payload.type} to main thread: ${e.message}`);
  }
}
