import http from 'http';
import fs from 'fs';
import path from 'path';

const logFile = path.join(process.cwd(), 'client-debug.log');

// Clear existing log file
try {
  fs.writeFileSync(logFile, '');
} catch (e) {}

const server = http.createServer((req, res) => {
  // Set CORS headers
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'POST, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type');

  if (req.method === 'OPTIONS') {
    res.writeHead(200);
    res.end();
    return;
  }

  if (req.method === 'POST' && req.url === '/log') {
    let body = '';
    req.on('data', chunk => {
      body += chunk.toString();
    });
    req.on('end', () => {
      try {
        const data = JSON.parse(body);
        const logLine = `[${data.time}] [${data.level}] ${data.message}\n`;
        fs.appendFileSync(logFile, logLine);
        console.log(logLine.trim());
      } catch (e) {
        fs.appendFileSync(logFile, `[RAW] ${body}\n`);
        console.log('[RAW]', body);
      }
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      res.end('OK');
    });
  } else {
    res.writeHead(404);
    res.end();
  }
});

server.listen(1425, '127.0.0.1', () => {
  console.log('Log server running on http://127.0.0.1:1425');
  fs.appendFileSync(logFile, `=== LOG SERVER STARTED AT ${new Date().toISOString()} ===\n`);
});
