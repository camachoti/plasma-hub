import type { FormatOption, MediaInfo } from '../types';
import { apiFetch } from '../downloaderApi';
import { extractYoutubeId, formatBytes, formatDuration } from '../downloaderUtils';

interface YtFormat {
  itag: number;
  url?: string;
  signatureCipher?: string;
  mimeType: string;
  quality: string;
  qualityLabel?: string;
  bitrate?: number;
  contentLength?: string;
  width?: number;
  height?: number;
  audioSampleRate?: string;
  audioQuality?: string;
  approxDurationMs?: string;
}

interface YtPlayerResponse {
  videoDetails?: {
    videoId: string;
    title: string;
    author: string;
    lengthSeconds: string;
    viewCount: string;
    thumbnail?: { thumbnails: Array<{ url: string; width: number; height: number }> };
  };
  streamingData?: {
    formats: YtFormat[];
    adaptiveFormats: YtFormat[];
    expiresInSeconds?: string;
  };
  playabilityStatus?: { status: string; reason?: string };
}

export async function extractYoutube(url: string): Promise<MediaInfo> {
  const videoId = extractYoutubeId(url);
  if (!videoId) throw new Error('Cannot parse YouTube URL');

  const res = await apiFetch(`https://www.youtube.com/watch?v=${videoId}`, {
    headers: {
      'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
    },
    rawText: true
  });

  const startStr = 'ytInitialPlayerResponse = ';
  const startIdx = res.text.indexOf(startStr);
  if (startIdx === -1) {
    throw new Error('Não foi possível extrair os dados do vídeo do YouTube. Tente novamente mais tarde.');
  }

  let jsonStr = res.text.slice(startIdx + startStr.length);
  const endScriptIdx = jsonStr.indexOf(';</script>');
  if (endScriptIdx !== -1) jsonStr = jsonStr.slice(0, endScriptIdx);
  const endVarIdx = jsonStr.indexOf('};var ');
  if (endVarIdx !== -1) jsonStr = jsonStr.slice(0, endVarIdx + 1);

  let data: YtPlayerResponse;
  try {
    data = JSON.parse(jsonStr) as YtPlayerResponse;
  } catch (err) {
    throw new Error('Falha ao processar os dados do YouTube (JSON inválido).');
  }

  if (data.playabilityStatus?.status === 'ERROR' || data.playabilityStatus?.status === 'LOGIN_REQUIRED') {
    throw new Error(data.playabilityStatus.reason ?? 'Vídeo indisponível ou requer login.');
  }

  const details = data.videoDetails;
  const streaming = data.streamingData;

  const muxed = (streaming?.formats ?? []);
  const videoOnly = (streaming?.adaptiveFormats ?? []).filter(
    (f) => f.mimeType.startsWith('video/')
  );
  const allVideos = [...muxed, ...videoOnly].sort((a, b) => (b.height ?? 0) - (a.height ?? 0));

  const uniqueVideos: FormatOption[] = [];
  const seenQualities = new Set<string>();

  for (let i = 0; i < allVideos.length; i++) {
    const f = allVideos[i];
    const isAdaptive = !muxed.includes(f);
    const label = f.qualityLabel ?? f.quality ?? 'Desconhecido';

    if (!seenQualities.has(label)) {
      seenQualities.add(label);
      uniqueVideos.push({
        id: String(f.itag),
        label,
        sub: `MP4 · ${f.quality}`,
        size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
        best: uniqueVideos.length === 0 && !isAdaptive,
        url: f.url,
        hasAudio: !isAdaptive,
      });
    } else if (!isAdaptive) {
      const existingIdx = uniqueVideos.findIndex(v => v.label === label);
      if (existingIdx !== -1 && !uniqueVideos[existingIdx].hasAudio) {
        uniqueVideos[existingIdx] = {
          id: String(f.itag),
          label,
          sub: `MP4 · ${f.quality}`,
          size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
          best: existingIdx === 0,
          url: f.url,
          hasAudio: true,
        };
      }
    }
  }

  const videoFormats: FormatOption[] = uniqueVideos;
  if (videoFormats.length === 0) {
    videoFormats.push({ id: 'best', label: 'Melhor Qualidade', sub: 'Muxed stream', size: '—' });
  }

  const audioOnly = (streaming?.adaptiveFormats ?? []).filter(
    (f) => f.mimeType.startsWith('audio/')
  );

  const audioFormats: FormatOption[] = audioOnly.slice(0, 3).map((f, i) => ({
    id: String(f.itag),
    label: f.audioQuality === 'AUDIO_QUALITY_HIGH' ? 'Alta qualidade' : f.audioQuality === 'AUDIO_QUALITY_MEDIUM' ? 'Qualidade média' : 'Compacto',
    sub: f.mimeType.split(';')[0].replace('audio/', '').toUpperCase() + (f.audioSampleRate ? ` · ${Math.round(parseInt(f.audioSampleRate) / 1000)}kHz` : ''),
    size: f.contentLength ? formatBytes(parseInt(f.contentLength)) : '—',
    best: i === 0,
    url: f.url,
  }));

  if (audioFormats.length === 0) {
    audioFormats.push({ id: 'mp3-mock', label: 'MP3 256kbps', sub: 'Extraído do vídeo', size: '—' });
  }

  const thumb = details?.thumbnail?.thumbnails;
  const thumbnailUrl = thumb ? thumb[thumb.length - 1]?.url : undefined;

  return {
    platform: 'youtube',
    title: details?.title ?? 'YouTube Video',
    author: details?.author ?? 'Unknown',
    duration: details?.lengthSeconds ? formatDuration(parseInt(details.lengthSeconds)) : '—',
    views: details?.viewCount
      ? parseInt(details.viewCount) > 1_000_000
        ? `${(parseInt(details.viewCount) / 1_000_000).toFixed(1)}M`
        : `${(parseInt(details.viewCount) / 1000).toFixed(0)}K`
      : undefined,
    thumbHue: 0,
    thumbHue2: 20,
    thumbnailUrl,
    originalUrl: url,
    formats: { video: videoFormats, audio: audioFormats },
  };
}
