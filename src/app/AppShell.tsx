import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import { IconAt, IconDownload, IconMessageCircle, IconSettings } from "../design-system/icons";
import { runtimeCapabilities } from "../shared/platform/runtime";
import { DOWNLOAD_STATUS_EVENT, type DownloadItem } from "../features/downloader/DownloadService";
import { Dialog, IconButton } from "../design-system";

const loadDashboard = () => import("../features/telegram/Dashboard");
const loadDownloads = () => import("../features/telegram/Downloads");
const loadTelegramSettings = () => import("../features/telegram/Settings");
const loadTwitterLibrary = () => import("../features/twitter/TwitterLibrary");

const Dashboard = lazy(() => loadDashboard().then(module => ({ default: module.Dashboard })));
const Downloads = lazy(() => loadDownloads().then(module => ({ default: module.Downloads })));
const TelegramSettings = lazy(() => loadTelegramSettings().then(module => ({ default: module.Settings })));
const TwitterLibrary = lazy(() => loadTwitterLibrary().then(module => ({ default: module.TwitterLibrary })));

export type AppTab = "telegram" | "downloads" | "twitter";

interface AppShellProps {
  activeTab: AppTab;
  isSettingsOpen: boolean;
  palette: string;
  density: string;
  skipLogin: boolean;
  telegramConnectionState: string;
  onActiveTabChange: (tab: AppTab) => void;
  onSettingsOpen: () => void;
  onSettingsClose: () => void;
  onTelegramLoginRequest: () => void;
}

export function AppShell({
  activeTab,
  isSettingsOpen,
  palette,
  density,
  skipLogin,
  telegramConnectionState,
  onActiveTabChange,
  onSettingsOpen,
  onSettingsClose,
  onTelegramLoginRequest,
}: AppShellProps) {
  const [downloadToast, setDownloadToast] = useState<DownloadItem | null>(null);
  const [visitedTabs, setVisitedTabs] = useState<Set<AppTab>>(() => new Set(["telegram"]));
  const [isCommandPaletteOpen, setIsCommandPaletteOpen] = useState(false);
  const [commandQuery, setCommandQuery] = useState("");
  const [activeCommandIndex, setActiveCommandIndex] = useState(0);

  const activateTab = (tab: AppTab) => {
    setVisitedTabs(current => current.has(tab) ? current : new Set(current).add(tab));
    onActiveTabChange(tab);
  };

  const preloadTab = (tab: AppTab) => {
    if (tab === "downloads") void loadDownloads();
    if (tab === "twitter") void loadTwitterLibrary();
  };

  const preloadSettings = () => {
    void loadTelegramSettings();
  };

  useEffect(() => {
    let timeoutId: number | undefined;
    const handleDownloadStatus = (event: Event) => {
      const item = (event as CustomEvent<DownloadItem>).detail;
      setDownloadToast(item);
      window.clearTimeout(timeoutId);
      timeoutId = window.setTimeout(() => setDownloadToast(null), 4200);
    };
    window.addEventListener(DOWNLOAD_STATUS_EVENT, handleDownloadStatus);
    return () => {
      window.removeEventListener(DOWNLOAD_STATUS_EVENT, handleDownloadStatus);
      window.clearTimeout(timeoutId);
    };
  }, []);

  useEffect(() => {
    setVisitedTabs(current => current.has(activeTab) ? current : new Set(current).add(activeTab));
  }, [activeTab]);

  useEffect(() => {
    const handleShortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setCommandQuery("");
        setActiveCommandIndex(0);
        setIsCommandPaletteOpen(true);
      }
      if (event.key === "Escape") setIsCommandPaletteOpen(false);
    };
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  }, []);

  const commands = useMemo(() => [
    { id: "telegram", label: "Abrir conversas", hint: "Telegram", run: () => activateTab("telegram") },
    { id: "downloads", label: "Abrir downloads", hint: "Downloads", run: () => activateTab("downloads") },
    { id: "twitter", label: "Abrir biblioteca Twitter / X", hint: "Twitter / X", run: () => activateTab("twitter") },
    { id: "settings", label: "Abrir configurações", hint: "Preferências", run: () => { preloadSettings(); onSettingsOpen(); } },
  ], [onSettingsOpen]);
  const visibleCommands = commands.filter(command => {
    const query = commandQuery.trim().toLocaleLowerCase("pt-BR");
    return !query || `${command.label} ${command.hint}`.toLocaleLowerCase("pt-BR").includes(query);
  });
  const fallback = (
    <div className="app-loading">
      <div className="loader-surface" role="status" aria-label="Carregando">
        <span className="modern-loader" />
      </div>
    </div>
  );
  const connectionNotice = !skipLogin && telegramConnectionState !== "ready"
    ? telegramConnectionState === "waiting_for_network"
      ? "Sem rede. O Telegram será reconectado automaticamente."
      : telegramConnectionState === "updating"
        ? "Telegram conectado. Atualizando mensagens…"
        : "Reconectando ao Telegram…"
    : null;

  return (
    <div
      className="app-container"
      data-palette={palette}
      data-density={density}
      data-runtime={runtimeCapabilities.kind}
    >
      {connectionNotice && (
        <div className="telegram-connection-notice" role="status" aria-live="polite">
          <span className="telegram-connection-dot" />
          {connectionNotice}
        </div>
      )}
      {downloadToast && (
        <button
          type="button"
          className={`app-toast ${downloadToast.status}`}
          onClick={() => setDownloadToast(null)}
          aria-live="polite"
        >
          <strong>{downloadToast.status === 'completed' ? 'Download concluído' : downloadToast.status === 'canceled' ? 'Download cancelado' : 'Falha no download'}</strong>
          <span>{downloadToast.fileName}</span>
        </button>
      )}
      <div className="sidebar">
        <IconButton
          className={`sidebar-item ${activeTab === "telegram" ? "active" : ""}`}
          onClick={() => activateTab("telegram")}
          title="Telegram"
          aria-label="Abrir Telegram"
        >
          <IconMessageCircle size={22} stroke={2} />
        </IconButton>
        <IconButton
          className={`sidebar-item ${activeTab === "downloads" ? "active" : ""}`}
          onClick={() => activateTab("downloads")}
          onPointerEnter={() => preloadTab("downloads")}
          onFocus={() => preloadTab("downloads")}
          title="Downloads"
          aria-label="Abrir downloads"
        >
          <IconDownload size={22} stroke={2} />
        </IconButton>
        <IconButton
          className={`sidebar-item ${activeTab === "twitter" ? "active" : ""}`}
          onClick={() => activateTab("twitter")}
          onPointerEnter={() => preloadTab("twitter")}
          onFocus={() => preloadTab("twitter")}
          title="Twitter / X"
          aria-label="Abrir Twitter / X"
        >
          <IconAt size={22} stroke={2} />
        </IconButton>
        <div style={{ flex: 1 }} />
        <IconButton
          className={`sidebar-item ${isSettingsOpen ? "active" : ""}`}
          onClick={onSettingsOpen}
          onPointerEnter={preloadSettings}
          onFocus={preloadSettings}
          onPointerDown={preloadSettings}
          title="Configurações"
          aria-label="Abrir configurações"
        >
          <IconSettings size={22} stroke={2} />
        </IconButton>
      </div>

      <div className="main-content" style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        {visitedTabs.has("telegram") && (
          <div className={`app-tab-panel ${activeTab === "telegram" ? "active" : "inactive"}`} aria-hidden={activeTab !== "telegram"}>
            <Suspense fallback={fallback}>
              <Dashboard skipLogin={skipLogin} onTelegramLoginRequest={onTelegramLoginRequest} />
            </Suspense>
          </div>
        )}
        {visitedTabs.has("downloads") && (
          <div className={`app-tab-panel ${activeTab === "downloads" ? "active" : "inactive"}`} aria-hidden={activeTab !== "downloads"}>
            <Suspense fallback={activeTab === "downloads" ? fallback : null}>
              <Downloads />
            </Suspense>
          </div>
        )}
        {visitedTabs.has("twitter") && (
          <div className={`app-tab-panel ${activeTab === "twitter" ? "active" : "inactive"}`} aria-hidden={activeTab !== "twitter"}>
            <Suspense fallback={activeTab === "twitter" ? fallback : null}>
              <TwitterLibrary />
            </Suspense>
          </div>
        )}
        {isSettingsOpen && (
          <Suspense fallback={null}>
            <TelegramSettings onClose={onSettingsClose} />
          </Suspense>
        )}
      </div>
      {isCommandPaletteOpen && (
        <Dialog className="command-palette ds-surface" label="Comandos rápidos" onClose={() => setIsCommandPaletteOpen(false)} overlayClassName="command-palette-overlay">
            <div className="command-palette-search">
              <span aria-hidden="true">⌕</span>
              <input
                autoFocus
                value={commandQuery}
                onChange={event => {
                  setCommandQuery(event.target.value);
                  setActiveCommandIndex(0);
                }}
                placeholder="Buscar ação..."
                onKeyDown={event => {
                  if (event.key === "ArrowDown") {
                    event.preventDefault();
                    setActiveCommandIndex(current => Math.min(current + 1, Math.max(visibleCommands.length - 1, 0)));
                  }
                  if (event.key === "ArrowUp") {
                    event.preventDefault();
                    setActiveCommandIndex(current => Math.max(current - 1, 0));
                  }
                  if (event.key === "Enter" && visibleCommands[activeCommandIndex]) {
                    visibleCommands[activeCommandIndex].run();
                    setIsCommandPaletteOpen(false);
                  }
                }}
              />
              <kbd>Esc</kbd>
            </div>
            <div className="command-palette-list">
              {visibleCommands.map((command, index) => (
                <button
                  key={command.id}
                  type="button"
                  className={`command-palette-item ${index === activeCommandIndex ? "active" : ""}`}
                  onMouseEnter={() => setActiveCommandIndex(index)}
                  onClick={() => {
                    command.run();
                    setIsCommandPaletteOpen(false);
                  }}
                >
                  <span>{command.label}</span>
                  <small>{command.hint}</small>
                </button>
              ))}
              {!visibleCommands.length && <div className="command-palette-empty">Nenhuma ação encontrada.</div>}
            </div>
            <div className="command-palette-footer"><kbd>Ctrl</kbd><span>+</span><kbd>K</kbd><span> para abrir</span></div>
        </Dialog>
      )}
    </div>
  );
}
