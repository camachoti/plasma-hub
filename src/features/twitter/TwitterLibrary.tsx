import { useEffect, useState } from 'react';
import { WarningCircle, DownloadSimple, Link, Spinner, ChatCircle, MagnifyingGlass, TwitterLogo } from '@phosphor-icons/react';
import { invokeCommand as invoke, listenEvent as listen } from '../../shared/platform/tauri';
import { runtimeCapabilities } from '../../shared/platform/runtime';
import { analyzeUrl, downloadMedia } from '../downloader/downloader';
import { downloadService, type DownloadItem } from '../downloader/DownloadService';
import type { MediaInfo } from '../downloader/types';
import { createTwitterProfileChat } from '../telegram/TwitterFakeChatStore';
import { getStoredTwitterCookies, onTwitterSettingsChanged } from './TwitterSettingsStore';
import '../../styles/TwitterLibrary.css';

interface TwitterProfileInfo {
  platform: 'twitter';
  username: string;
  displayName?: string;
  avatarUrl?: string;
  thumbnailUrl?: string;
  mediaCount: number;
  mediaUrls: string[];
  mediaItems?: Array<{
    url: string;
    thumbnailUrl?: string | null;
    isVideo: boolean;
  }>;
  originalUrl: string;
}

function getTwitterProfileUsername(input: string): string | null {
  try {
    const parsed = new URL(input);
    const host = parsed.hostname.toLowerCase().replace(/^www\./, '');
    if (host !== 'x.com' && host !== 'twitter.com') return null;
    const parts = parsed.pathname.split('/').filter(Boolean);
    if (parts.length !== 1) return null;
    return /^[A-Za-z0-9_]{1,15}$/.test(parts[0]) ? parts[0] : null;
  } catch {
    const match = input.match(/(?:x\.com|twitter\.com)\/([A-Za-z0-9_]{1,15})\/?$/);
    return match?.[1] ?? null;
  }
}

export function TwitterLibrary() {
  const [url, setUrl] = useState('');
  const [cookies, setCookies] = useState(getStoredTwitterCookies);
  const [media, setMedia] = useState<MediaInfo | null>(null);
  const [profile, setProfile] = useState<TwitterProfileInfo | null>(null);
  const [selectedFormat, setSelectedFormat] = useState('');
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [analyzing, setAnalyzing] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [creatingChat, setCreatingChat] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDownloads(Array.from(downloadService.activeDownloads.values()).filter(item => item.platform === 'twitter').reverse());
    const unsubscribe = downloadService.onDownloadsChange((list: DownloadItem[]) => {
      setDownloads(list.filter(item => item.platform === 'twitter').reverse());
    });
    return () => unsubscribe();
  }, []);

  useEffect(() => onTwitterSettingsChanged(() => setCookies(getStoredTwitterCookies())), []);

  async function handleAnalyze() {
    const cleanUrl = url.trim();
    if (!cleanUrl) return;

    setAnalyzing(true);
    setError(null);
    setMedia(null);
    setProfile(null);
    setSelectedFormat('');

    try {
      if (getTwitterProfileUsername(cleanUrl)) {
        const info = await invoke<TwitterProfileInfo>('analyze_twitter_profile_native', {
          url: cleanUrl,
          cookies: cookies.trim() || null,
        });
        setProfile(info);
        return;
      }

      const info = await analyzeUrl(cleanUrl, { twitterCookies: cookies });
      if (info.platform !== 'twitter') {
        throw new Error('Cole uma URL do Twitter/X.');
      }

      setMedia(info);
      const firstPlayable = info.formats.video.find(format => format.url);
      if (firstPlayable) setSelectedFormat(firstPlayable.id);
    } catch (err: any) {
      setError(err?.message || 'Nao foi possivel analisar o tweet.');
    } finally {
      setAnalyzing(false);
    }
  }

  async function handleDownload() {
    if (!media || !selectedFormat) return;

    setDownloading(true);
    setError(null);
    try {
      await downloadMedia(media, 'video', selectedFormat, { twitterCookies: cookies });
    } catch (err: any) {
      setError(err?.message || 'Falha ao baixar mídia.');
    } finally {
      setDownloading(false);
    }
  }

  async function handleProfileDownload() {
    if (!profile || profile.mediaUrls.length === 0) return;
    if (!runtimeCapabilities.supportsNativeTwitter) {
      setError('Download nativo do Twitter/X ainda não está disponível no Android.');
      return;
    }

    const downloadId = `twitter_profile_${profile.username}_${Date.now()}`;
    const fileName = `plasma_twitter_${profile.username}`;
    setDownloading(true);
    setError(null);
    downloadService.addDownload({
      id: downloadId,
      fileName,
      progress: 0,
      status: 'downloading',
      platform: 'twitter',
      thumbnailUrl: profile.thumbnailUrl || profile.avatarUrl,
    });

    try {
      const unlistenProgress = await listen<{id: string, progress: number}>('twitter-download-progress', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { progress: e.payload.progress });
        }
      });
      const unlistenDone = await listen<{id: string}>('twitter-download-done', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'completed', progress: 100 });
        }
      });
      const unlistenError = await listen<{id: string, error: string}>('twitter-download-error', (e) => {
        if (e.payload.id === downloadId) {
          downloadService.updateDownload(downloadId, { status: 'failed', error: e.payload.error });
        }
      });

      await invoke('download_twitter_profile_native', {
        id: downloadId,
        username: profile.username,
        mediaUrls: profile.mediaUrls,
        cookies: cookies.trim() || null,
      });

      unlistenProgress();
      unlistenDone();
      unlistenError();
    } catch (err: any) {
      downloadService.updateDownload(downloadId, { status: 'failed', error: err?.message || String(err) });
      setError(err?.message || 'Falha ao baixar mídias do perfil.');
    } finally {
      setDownloading(false);
    }
  }

  async function handleCreateProfileChat() {
    if (!profile || profile.mediaUrls.length === 0) return;

    setCreatingChat(true);
    setError(null);
    try {
      const res = createTwitterProfileChat(profile);
      if (!res.success) {
        throw new Error(res.error || 'Falha ao criar chat do perfil.');
      }
    } catch (err: any) {
      setError(err?.message || 'Falha ao criar chat do perfil.');
    } finally {
      setCreatingChat(false);
    }
  }

  const selected = media?.formats.video.find(format => format.id === selectedFormat);
  const canDownload = Boolean(media && selected?.url);
  const canDownloadProfile = Boolean(profile && profile.mediaUrls.length > 0);
  const runningCount = downloads.filter(item => item.status === 'downloading').length;

  return (
    <div className="twitter-library twitter-library-single">
      <main className="twitter-main">
        <header className="twitter-topbar">
          <div className="twitter-title-block">
            <span className="twitter-topbar-icon">
              <TwitterLogo size={18} weight="fill" />
            </span>
            <h1>Twitter / X</h1>
            <span>{downloads.length} downloads</span>
          </div>
        </header>

        <div className="twitter-content">
          <section className="twitter-panel twitter-tweet-tool">
            <div className="twitter-tool-header">
              <label htmlFor="twitter-url">URL do tweet/status</label>
              {cookies.trim() && (
                <span className="twitter-cookies-saved">Cookies configurados</span>
              )}
            </div>
            <div className="twitter-url-row">
              <div className="twitter-input-row">
                <Link size={18} />
                <input
                  id="twitter-url"
                  value={url}
                  onChange={event => setUrl(event.target.value)}
                  onKeyDown={event => event.key === 'Enter' && handleAnalyze()}
                  placeholder="https://x.com/usuario/status/123456789"
                />
              </div>
              <button className="twitter-primary-btn" onClick={handleAnalyze} disabled={analyzing || !url.trim()}>
                {analyzing ? <Spinner className="spin" size={18} /> : <MagnifyingGlass size={18} />}
                <span>{analyzing ? 'Analisando' : 'Analisar'}</span>
              </button>
            </div>
            {error && (
              <div className="twitter-error">
                <WarningCircle size={17} />
                <span>{error}</span>
              </div>
            )}
          </section>

          {media && (
            <section className="twitter-preview">
              {media.thumbnailUrl && (
                <img src={media.thumbnailUrl} alt="" className="twitter-preview-thumb" />
              )}
              <div className="twitter-preview-info">
                <p>{media.author} {media.duration !== '—' ? `· ${media.duration}` : ''}</p>
                <h3>{media.title}</h3>
                <div className="twitter-format-row">
                  <select value={selectedFormat} onChange={event => setSelectedFormat(event.target.value)}>
                    {media.formats.video.map(format => (
                      <option key={format.id} value={format.id} disabled={!format.url}>
                        {format.label} {format.size !== '—' ? `(${format.size})` : ''}
                      </option>
                    ))}
                  </select>
                  <button className="twitter-primary-btn" onClick={handleDownload} disabled={!canDownload || downloading}>
                    {downloading ? <Spinner className="spin" size={18} /> : <DownloadSimple size={18} />}
                    <span>{downloading ? 'Baixando' : 'Baixar'}</span>
                  </button>
                </div>
              </div>
            </section>
          )}

          {profile && (
            <section className="twitter-preview">
              {(profile.thumbnailUrl || profile.avatarUrl) && (
                <img
                  src={profile.thumbnailUrl || profile.avatarUrl}
                  alt=""
                  className={`twitter-preview-thumb ${profile.thumbnailUrl ? '' : 'twitter-profile-avatar'}`}
                />
              )}
              <div className="twitter-preview-info">
                <p>@{profile.username}</p>
                <h3>{profile.displayName || `Perfil @${profile.username}`}</h3>
                <div className="twitter-profile-stats">
                  <strong>{profile.mediaCount}</strong>
                  <span>{profile.mediaCount === 1 ? 'mídia encontrada' : 'mídias encontradas'}</span>
                </div>
                <div className="twitter-format-row">
                  <button className="twitter-primary-btn" onClick={handleProfileDownload} disabled={!canDownloadProfile || downloading}>
                    {downloading ? <Spinner className="spin" size={18} /> : <DownloadSimple size={18} />}
                    <span>{downloading ? 'Baixando' : 'Baixar mídias'}</span>
                  </button>
                  <button className="twitter-secondary-btn" onClick={handleCreateProfileChat} disabled={!canDownloadProfile || creatingChat}>
                    {creatingChat ? <Spinner className="spin" size={18} /> : <ChatCircle size={18} />}
                    <span>{creatingChat ? 'Criando' : 'Criar chat'}</span>
                  </button>
                </div>
              </div>
            </section>
          )}

          <section className="twitter-activity">
            <div className="twitter-activity-header">
              <h3>
                Downloads
                {runningCount > 0 && <span className="twitter-running-badge">{runningCount} RUNNING</span>}
              </h3>
            </div>
            <div className="twitter-feed">
              {downloads.length === 0 ? (
                <div className="twitter-feed-empty">Nenhum download do Twitter/X nesta sessão.</div>
              ) : (
                downloads.map(item => (
                  <div className={`twitter-feed-item ${item.status === 'failed' ? 'error' : item.status === 'completed' ? 'success' : 'info'}`} key={item.id}>
                    <span>{Math.round(item.progress)}%</span>
                    <p>{item.fileName} · {item.status}{item.error ? ` · ${item.error}` : ''}</p>
                  </div>
                ))
              )}
            </div>
          </section>
        </div>
      </main>
    </div>
  );
}
