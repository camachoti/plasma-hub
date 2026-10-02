import React from 'react';
import { IconChevronDown, IconCircleX, IconX } from "../../design-system/icons";
import { Virtuoso } from 'react-virtuoso';
import type { ForumTopic } from './TelegramDashboardTypes';
import { formatBytes } from './DashboardHelpers';
import { IconMagic } from './DashboardIcons';
import { LoadingIndicator, TextField } from '../../design-system';

interface DownloadItem {
  name: string;
  status: 'pending' | 'downloading' | 'completed' | 'skipped' | 'failed' | 'stopped';
  progress: number;
  size: number;
}

interface DownloadProgress {
  total: number;
  downloaded: number;
  currentFile: string;
  topicTitle?: string | null;
  isScanning?: boolean;
  items?: DownloadItem[];
}

interface DashboardMassDownloadPanelProps {
  albumSplitMode: 'separator' | 'comment';
  downloading: boolean;
  filteredTopics: ForumTopic[];
  folderPath: string;
  forumTopics: ForumTopic[];
  hasTopics: boolean;
  isTopicDropdownOpen: boolean;
  loadingTopics: boolean;
  progress: DownloadProgress | null;
  progressDetailsListRef: React.RefObject<HTMLDivElement | null>;
  selectedTopicId: string;
  showDetailedProgress: boolean;
  splitByAlbum: boolean;
  splitByUser: boolean;
  stopping: boolean;
  topicSearch: string;
  handleSelectFolder: () => void;
  handleStartDownload: () => void;
  handleStopDownload: () => void;
  setAlbumSplitMode: React.Dispatch<React.SetStateAction<'separator' | 'comment'>>;
  setIsDownloadModalOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setIsTopicDropdownOpen: React.Dispatch<React.SetStateAction<boolean>>;
  setSelectedTopicId: React.Dispatch<React.SetStateAction<string>>;
  setShowDetailedProgress: React.Dispatch<React.SetStateAction<boolean>>;
  setSplitByAlbum: React.Dispatch<React.SetStateAction<boolean>>;
  setSplitByUser: React.Dispatch<React.SetStateAction<boolean>>;
  setTopicSearch: React.Dispatch<React.SetStateAction<string>>;
}

const ProgressItemRow = React.memo(({ item }: { item: DownloadItem }) => (
  <div className={`progress-item-row ${item.status}`}>
    <div className="progress-item-info">
      <span className="progress-item-name" title={item.name}>{item.name}</span>
      {item.size > 0 && (
        <span className="progress-item-size">
          {formatBytes(item.size)}
        </span>
      )}
    </div>
    <div className="progress-item-status-col">
      {item.status === 'pending' && (
        <span className="item-badge">Na fila</span>
      )}
      {item.status === 'downloading' && (
        <span className="item-badge downloading">
          <LoadingIndicator size="xs" style={{ marginRight: 4 }} />
          {item.progress}%
        </span>
      )}
      {item.status === 'completed' && (
        <span className="item-badge completed">✓ Salvo</span>
      )}
      {item.status === 'skipped' && (
        <span className="item-badge skipped">⌥ Já existe</span>
      )}
      {item.status === 'failed' && (
        <span className="item-badge failed"><IconCircleX size={14} stroke={2} aria-hidden="true" /> Falhou</span>
      )}
      {item.status === 'stopped' && (
        <span className="item-badge stopped">Interrompido</span>
      )}
    </div>
  </div>
));

const DashboardMassDownloadPanelComponent: React.FC<DashboardMassDownloadPanelProps> = ({
  albumSplitMode,
  downloading,
  filteredTopics,
  folderPath,
  forumTopics,
  hasTopics,
  isTopicDropdownOpen,
  loadingTopics,
  progress,
  progressDetailsListRef,
  selectedTopicId,
  showDetailedProgress,
  splitByAlbum,
  splitByUser,
  stopping,
  topicSearch,
  handleSelectFolder,
  handleStartDownload,
  handleStopDownload,
  setAlbumSplitMode,
  setIsDownloadModalOpen,
  setIsTopicDropdownOpen,
  setSelectedTopicId,
  setShowDetailedProgress,
  setSplitByAlbum,
  setSplitByUser,
  setTopicSearch,
}) => {
  const itemSummary = (progress?.items || []).reduce(
    (summary, item) => {
      if (item.status === 'completed') summary.completed += 1;
      else if (item.status === 'skipped') summary.skipped += 1;
      else if (item.status === 'failed') summary.failed += 1;
      else if (item.status === 'stopped') summary.stopped += 1;
      else if (item.status === 'pending') summary.pending += 1;
      else if (item.status === 'downloading') summary.downloading += 1;
      return summary;
    },
    { completed: 0, skipped: 0, failed: 0, stopped: 0, pending: 0, downloading: 0 },
  );
  const processedCount = itemSummary.completed + itemSummary.skipped + itemSummary.failed + itemSummary.stopped;

  return (
  <div className="inline-download-panel">
    <div className="inline-panel-header">
      <div className="inline-panel-title">
        <IconMagic />
        <h3>Mass Download</h3>
      </div>
      <button className="icon-btn" onClick={() => setIsDownloadModalOpen(false)} aria-label="Fechar download em massa"><IconX size={18} stroke={2} /></button>
    </div>
    <div className="inline-panel-body">
      <div className="mass-download-main-row">
        <div className="inline-folder">
          <div className="folder-selection">
            <TextField appearance="inline" readOnly value={folderPath} placeholder="Selecionar pasta de destino..." />
            <button className="browse-btn" onClick={handleSelectFolder}>Procurar</button>
          </div>
        </div>
        {hasTopics && (
          <div className="topic-selection mass-download-topic-selection">
            <div className={`custom-select ${isTopicDropdownOpen ? 'open' : ''} ${loadingTopics || downloading ? 'disabled' : ''}`}>
              <button
                type="button"
                className="custom-select-trigger"
                onClick={event => {
                  event.stopPropagation();
                  if (!loadingTopics && !downloading) setIsTopicDropdownOpen(value => !value);
                }}
                disabled={loadingTopics || downloading}
                aria-label={loadingTopics ? 'Carregando tópicos' : undefined}
              >
                <span>
                  {loadingTopics ? (
                    <LoadingIndicator size="sm" />
                  ) : selectedTopicId === 'all' ? 'Todos os tópicos' : forumTopics.find(topic => String(topic.id) === selectedTopicId)?.title || 'Todos os tópicos'}
                </span>
                <IconChevronDown size={14} stroke={2} aria-hidden="true" />
              </button>
              {isTopicDropdownOpen && (
                <div className="custom-select-options">
                  <div className="custom-select-search" onClick={event => event.stopPropagation()}>
                    <TextField type="text" placeholder="Pesquisar tópicos..." value={topicSearch} onChange={event => setTopicSearch(event.target.value)} autoFocus />
                  </div>
                  <button type="button" className={`custom-select-option ${selectedTopicId === 'all' ? 'selected' : ''}`} onClick={event => { event.stopPropagation(); setSelectedTopicId('all'); setIsTopicDropdownOpen(false); setTopicSearch(''); }}>
                    Todos os tópicos
                  </button>
                  {filteredTopics.map(topic => (
                    <button key={topic.id} type="button" className={`custom-select-option ${String(topic.id) === selectedTopicId ? 'selected' : ''}`} onClick={event => { event.stopPropagation(); setSelectedTopicId(String(topic.id)); setIsTopicDropdownOpen(false); setTopicSearch(''); }}>
                      {topic.pinned && <span className="option-pin">📌</span>}
                      {topic.title}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
        {downloading ? (
          <button className={`stop-btn ${stopping ? 'disabled' : ''}`} onClick={handleStopDownload} disabled={stopping}>
            {stopping ? '⏳ Parando...' : '⏹ Parar'}
          </button>
        ) : (
          <button className="start-btn" onClick={handleStartDownload} disabled={!folderPath || loadingTopics}>
            Iniciar
          </button>
        )}
      </div>

      <div className="mass-download-options-row">
          <div className="split-user-selection">
            <label className="switch-label">
              <input
                type="checkbox"
                checked={splitByUser}
                onChange={event => setSplitByUser(event.target.checked)}
                disabled={downloading}
              />
              <span className="switch-custom" />
              <span className="switch-text">Dividir mídias por usuário</span>
            </label>
          </div>
          <div className="split-album-selection">
            <label className="switch-label">
              <input
                type="checkbox"
                checked={splitByAlbum}
                onChange={event => setSplitByAlbum(event.target.checked)}
                disabled={downloading}
              />
              <span className="switch-custom" />
              <span className="switch-text">Dividir mídias por álbum</span>
            </label>
            {splitByAlbum && (
              <div className="album-mode-toggle" role="group" aria-label="Modo de divisão por álbum">
                <button
                  type="button"
                  className={albumSplitMode === 'separator' ? 'active' : ''}
                  onClick={() => setAlbumSplitMode('separator')}
                  disabled={downloading}
                >
                  Separador
                </button>
                <button
                  type="button"
                  className={albumSplitMode === 'comment' ? 'active' : ''}
                  onClick={() => setAlbumSplitMode('comment')}
                  disabled={downloading}
                >
                  Comentário
                </button>
              </div>
            )}
          </div>
      </div>
    </div>
    {progress && (
      <div className={`inline-progress-container ${showDetailedProgress ? 'expanded' : ''}`}>
        <div className="progress-header">
          <span className="progress-status">
            {progress.topicTitle ? `${progress.topicTitle} · ${progress.currentFile}` : progress.currentFile}
          </span>
          <button
            type="button"
            className="progress-details-toggle"
            onClick={() => setShowDetailedProgress(value => !value)}
            aria-expanded={showDetailedProgress}
            aria-controls="mass-download-details"
          >
            {progress.isScanning ? 'Escaneando...' : `${processedCount} de ${progress.total}`}
            <span aria-hidden="true">{showDetailedProgress ? '▲' : '▼'}</span>
          </button>
        </div>
        <div className="progress-bar">
          <div
            className={`progress-fill ${downloading ? 'animated-stripes' : ''} ${progress.isScanning ? 'scanning-fill' : ''}`}
            style={{ width: (progress.total > 0 && !progress.isScanning) ? `${Math.min(100, (progress.downloaded / progress.total) * 100)}%` : '100%' }}
          />
        </div>
        {!progress.isScanning && progress.total > 0 && (
          <div className="progress-summary" aria-label="Resumo do download">
            <span className="progress-summary-item completed">Baixados: {itemSummary.completed}</span>
            <span className="progress-summary-item skipped">Já existem: {itemSummary.skipped}</span>
            {itemSummary.failed > 0 && <span className="progress-summary-item failed">Falharam: {itemSummary.failed}</span>}
            {itemSummary.stopped > 0 && <span className="progress-summary-item stopped">Interrompidos: {itemSummary.stopped}</span>}
            {(itemSummary.pending + itemSummary.downloading) > 0 && <span className="progress-summary-item pending">Na fila: {itemSummary.pending + itemSummary.downloading}</span>}
          </div>
        )}
        {showDetailedProgress && progress.items && progress.items.length > 0 && (
          <div id="mass-download-details" ref={progressDetailsListRef} className="progress-details-list">
            <Virtuoso
              style={{ height: '100%' }}
              data={progress.items}
              computeItemKey={(index, item) => `${item.name}_${index}`}
              itemContent={(_, item) => <ProgressItemRow item={item} />}
            />
          </div>
        )}
      </div>
    )}
  </div>
  );
};

export const DashboardMassDownloadPanel = React.memo(DashboardMassDownloadPanelComponent);
