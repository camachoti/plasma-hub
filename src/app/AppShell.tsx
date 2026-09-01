import { lazy, Suspense, useEffect, useState } from "react";
import { At, ChatCircle, DownloadSimple, Gear as SettingsIcon } from "@phosphor-icons/react";
import { runtimeCapabilities } from "../shared/platform/runtime";
import { DOWNLOAD_STATUS_EVENT, type DownloadItem } from "../features/downloader/DownloadService";

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
        <button
          className={`sidebar-item ${activeTab === "telegram" ? "active" : ""}`}
          onClick={() => activateTab("telegram")}
          title="Telegram"
          aria-label="Abrir Telegram"
        >
          <ChatCircle size={22} />
        </button>
        <button
          className={`sidebar-item ${activeTab === "downloads" ? "active" : ""}`}
          onClick={() => activateTab("downloads")}
          onPointerEnter={() => preloadTab("downloads")}
          onFocus={() => preloadTab("downloads")}
          title="Downloads"
          aria-label="Abrir downloads"
        >
          <DownloadSimple size={22} />
        </button>
        <button
          className={`sidebar-item ${activeTab === "twitter" ? "active" : ""}`}
          onClick={() => activateTab("twitter")}
          onPointerEnter={() => preloadTab("twitter")}
          onFocus={() => preloadTab("twitter")}
          title="Twitter / X"
          aria-label="Abrir Twitter / X"
        >
          <At size={22} />
        </button>
        <div style={{ flex: 1 }} />
        <button
          className={`sidebar-item ${isSettingsOpen ? "active" : ""}`}
          onClick={onSettingsOpen}
          onPointerEnter={preloadSettings}
          onFocus={preloadSettings}
          onPointerDown={preloadSettings}
          title="Configurações"
          aria-label="Abrir configurações"
        >
          <SettingsIcon size={22} />
        </button>
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
            <div style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              height: "100%",
              background: "rgba(0,0,0,0.6)",
              backdropFilter: "blur(4px)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              zIndex: 100,
            }}>
              <TelegramSettings onClose={onSettingsClose} />
            </div>
          </Suspense>
        )}
      </div>
    </div>
  );
}
