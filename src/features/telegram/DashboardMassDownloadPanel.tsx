import React from 'react';
import type { ForumTopic } from './TelegramDashboardTypes';
import { formatBytes } from './DashboardHelpers';
import { IconMagic } from './DashboardIcons';

interface DownloadItem {
  name: string;
  status: 'pending' | 'downloading' | 'completed' | 'skipped' | 'failed';
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

export const DashboardMassDownloadPanel: React.FC<DashboardMassDownloadPanelProps> = ({
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
}) => (
  <div className="inline-download-panel">
    <div className="inline-panel-header">
      <div className="inline-panel-title">
        <IconMagic />
        <h3>Mass Download</h3>
      </div>
      <button className="icon-btn" onClick={() => setIsDownloadModalOpen(false)}>✕</button>
    </div>
    <div className="inline-panel-body">
      <div className="mass-download-main-row">
        <div className="inline-folder">
          <div className="folder-selection">
            <input readOnly value={folderPath} placeholder="Selecionar pasta de destino..." />
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
                    <span className="modern-loader small" aria-hidden="true" />
                  ) : selectedTopicId === 'all' ? 'Todos os tópicos' : forumTopics.find(topic => String(topic.id) === selectedTopicId)?.title || 'Todos os tópicos'}
                </span>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><polyline points="6 9 12 15 18 9" /></svg>
              </button>
              {isTopicDropdownOpen && (
                <div className="custom-select-options">
                  <div className="custom-select-search" onClick={event => event.stopPropagation()}>
                    <input type="text" placeholder="Pesquisar tópicos..." value={topicSearch} onChange={event => setTopicSearch(event.target.value)} autoFocus />
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

      {!hasTopics && (
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
      )}
    </div>
    {progress && (
      <div
        className={`inline-progress-container ${showDetailedProgress ? 'expanded' : ''}`}
        onClick={() => setShowDetailedProgress(!showDetailedProgress)}
        style={{ cursor: 'pointer', userSelect: 'none' }}
      >
        <div className="progress-header">
          <span className="progress-status">
            {progress.topicTitle ? `${progress.topicTitle} · ${progress.currentFile}` : progress.currentFile}
          </span>
          <span className="progress-count" style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
            {progress.isScanning ? 'Escaneando...' : `${Math.floor(progress.downloaded)} / ${progress.total}`}
            <span style={{ fontSize: '10px', transition: 'transform 0.2s', transform: showDetailedProgress ? 'rotate(180deg)' : 'rotate(0deg)' }}>
              ▼
            </span>
          </span>
        </div>
        <div className="progress-bar">
          <div
            className={`progress-fill ${downloading ? 'animated-stripes' : ''} ${progress.isScanning ? 'scanning-fill' : ''}`}
            style={{ width: (progress.total > 0 && !progress.isScanning) ? `${Math.min(100, (progress.downloaded / progress.total) * 100)}%` : '100%' }}
          />
        </div>
        {showDetailedProgress && progress.items && progress.items.length > 0 && (
          <div ref={progressDetailsListRef} className="progress-details-list" onClick={event => event.stopPropagation()}>
            {progress.items.map((item, index) => (
              <div key={index} className={`progress-item-row ${item.status}`}>
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
                      <span className="spinner small-spinner inline-spinner" style={{ width: 10, height: 10, borderWidth: 1.5, display: 'inline-block', marginRight: 4 }} />
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
                    <span className="item-badge failed">✕ Falhou</span>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    )}
  </div>
);
