import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/downloader/downloaderUtils.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } }).outputText;
const { getDownloadExtension } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('audio keeps its actual container instead of being renamed to MP3', () => {
  assert.equal(getDownloadExtension('youtube', 'audio', 'https://example.com/playback', 'MP4 · 44kHz'), 'm4a');
  assert.equal(getDownloadExtension('youtube', 'audio', 'https://example.com/playback', 'WEBM · 48kHz'), 'webm');
  assert.equal(getDownloadExtension('reddit', 'audio', 'https://v.redd.it/abc/DASH_audio.mp4'), 'mp4');
});

test('an image URL keeps its image extension', () => {
  assert.equal(getDownloadExtension('twitter', 'video', 'https://pbs.twimg.com/media/abc?format=jpg&name=large'), 'jpg');
  assert.equal(getDownloadExtension('youtube', 'video', undefined, 'WEBM · 1080p'), 'webm');
});
