import { useEffect, useRef, useState } from 'react';
import { IconFolderOpen, IconLoader2, IconX } from '../../design-system/icons';
import { Dialog } from '../../design-system';
import { revealSystemItem } from '../../shared/platform/files';
import { invokeCommand, listenEvent } from '../../shared/platform/tauri';
import { telegramService } from './TelegramService';

export interface DuplicateSearchMedia {
  id: string;
  chatId: string;
  messageId: number;
  name: string;
  isVideo: boolean;
}

interface SearchMatch { sourceId: string; path: string }
interface SearchProgress { runId: string; scanned: number; analyzed: number; matches: SearchMatch[]; unreadable: number }
interface SearchResult { scanned: number; analyzed: number; matches: SearchMatch[]; unreadable: number; canceled: boolean }
interface SearchSource { id: string; name: string; isVideo: boolean; path?: string; bytes?: number[] }

interface Props {
  folderPath: string;
  media: DuplicateSearchMedia[];
  onClose: () => void;
}

export function DashboardDuplicateSearch({ folderPath, media, onClose }: Props) {
  const activeRunId = useRef('');
  const [phase, setPhase] = useState<'preparing' | 'searching' | 'done' | 'canceled' | 'error'>('preparing');
  const [progress, setProgress] = useState<SearchProgress>({ runId: '', scanned: 0, analyzed: 0, matches: [], unreadable: 0 });
  const [error, setError] = useState('');
  const [canceling, setCanceling] = useState(false);
  const canceled = useRef(false);
  const preparedSources = useRef<SearchSource[] | null>(null);
  const [mode, setMode] = useState<'exact' | 'visual'>('exact');

  useEffect(() => {
    const runId = crypto.randomUUID();
    activeRunId.current = runId;
    canceled.current = false;
    setPhase('preparing');
    setCanceling(false);
    setProgress({ runId, scanned: 0, analyzed: 0, matches: [], unreadable: 0 });
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const start = async () => {
      try {
        unlisten = await listenEvent<SearchProgress>('duplicate-media-search-progress', event => {
          if (event.payload.runId === runId && !disposed) {
            setProgress(previous => ({
              ...event.payload,
              matches: [...previous.matches, ...event.payload.matches],
            }));
          }
        });
        if (disposed) { unlisten(); return; }
        if (!preparedSources.current) {
          const sources: SearchSource[] = [];
          for (const item of media) {
            if (disposed || canceled.current) return;
            const source = await telegramService.getDuplicateSearchSource(item);
            sources.push({ id: item.id, name: item.name, isVideo: item.isVideo, ...source });
          }
          preparedSources.current = sources;
        }
        if (disposed || canceled.current) return;
        setPhase('searching');
        const result = await invokeCommand<SearchResult>('search_duplicate_media', {
          runId, folderPath, sources: preparedSources.current, mode,
        });
        if (!disposed) {
          setProgress({ runId, scanned: result.scanned, analyzed: result.analyzed, matches: result.matches, unreadable: result.unreadable });
          setPhase(result.canceled ? 'canceled' : 'done');
        }
      } catch (caught) {
        if (!disposed) {
          setError(caught instanceof Error ? caught.message : String(caught));
          setPhase('error');
        }
      } finally {
        unlisten?.();
      }
    };
    void start();
    return () => {
      disposed = true;
      canceled.current = true;
      unlisten?.();
      void invokeCommand<boolean>('cancel_native_download', { id: `duplicate-search-${runId}` });
    };
  }, [folderPath, media, mode]);

  const cancel = async () => {
    canceled.current = true;
    setCanceling(true);
    if (phase === 'preparing') {
      setPhase('canceled');
    } else {
      // The scanner may still be registering while the invoke request starts.
      for (let attempt = 0; attempt < 20; attempt += 1) {
        if (await invokeCommand<boolean>('cancel_native_download', { id: `duplicate-search-${activeRunId.current}` })) return;
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      setCanceling(false);
    }
  };

  const busy = phase === 'preparing' || phase === 'searching';
  const mediaType = media.every(item => item.isVideo) ? 'vídeos' : media.every(item => !item.isVideo) ? 'fotos' : 'mídias';
  const searchCount = mode === 'visual'
    ? `${progress.analyzed} ${mediaType} em ${progress.scanned} arquivos examinados`
    : `${progress.scanned} arquivos verificados`;
  return (
    <Dialog label="Procurar arquivo no sistema" onClose={onClose} overlayClassName="duplicate-search-overlay" className="duplicate-search-modal">
        <header className="duplicate-search-header">
          <div>
            <h3 id="duplicate-search-title">Procurar arquivo no sistema</h3>
            <p title={folderPath}>{folderPath}</p>
          </div>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Fechar busca"><IconX size={18} /></button>
        </header>
        <div className="duplicate-search-body">
          <div className="duplicate-search-status" role="status">
            {busy && <IconLoader2 size={18} className="duplicate-search-spinner" aria-hidden="true" />}
            <span>{phase === 'preparing' ? 'Preparando mídia...' : phase === 'searching' ? `${mode === 'exact' ? 'Busca exata' : 'Varredura profunda'} nas subpastas · ${searchCount}` : phase === 'done' ? `${mode === 'exact' ? 'Busca exata' : 'Varredura profunda'} concluída · ${searchCount}` : phase === 'canceled' ? 'Busca cancelada' : error}</span>
          </div>
          {progress.unreadable > 0 && <p className="duplicate-search-note">{progress.unreadable} arquivos ou pastas não puderam ser lidos.</p>}
          {media.map(item => {
            const matches = progress.matches.filter(match => match.sourceId === item.id);
            return <section key={item.id} className="duplicate-search-item">
              <h4>{item.name}</h4>
              {matches.length > 0 ? <>{mode === 'visual' && <p className="duplicate-search-note">Possíveis correspondências visuais</p>}<ul>{matches.map(match => <li key={match.path}>
                <span title={match.path}>{match.path}</span>
                <button type="button" className="icon-btn" onClick={() => void revealSystemItem(match.path)} title="Mostrar no gerenciador de arquivos" aria-label={`Mostrar ${match.path} no gerenciador de arquivos`}><IconFolderOpen size={18} /></button>
              </li>)}</ul></> : <p>{phase === 'done' ? mode === 'exact' ? 'Nenhuma cópia exata encontrada nesta pasta.' : 'Nenhuma mídia semelhante encontrada nesta pasta.' : 'Nenhuma correspondência encontrada até agora.'}</p>}
            </section>;
          })}
        </div>
        <footer className="duplicate-search-footer">
          {phase === 'done' && mode === 'exact' && progress.matches.length === 0 && (
            <button type="button" className="confirm-modal-confirm" onClick={() => { setPhase('preparing'); setMode('visual'); }} title="Procurar fotos redimensionadas ou recomprimidas e vídeos com duração e quadros semelhantes">Varredura mais profunda</button>
          )}
          {busy && <button type="button" className="confirm-modal-cancel" disabled={canceling} onClick={() => void cancel()}>{canceling ? 'Cancelando...' : 'Cancelar busca'}</button>}
          <button type="button" className="confirm-modal-cancel" onClick={onClose}>Fechar</button>
        </footer>
    </Dialog>
  );
}
