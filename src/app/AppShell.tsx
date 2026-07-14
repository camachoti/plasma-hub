import { At, ChatCircle, DownloadSimple, Gear as SettingsIcon } from "@phosphor-icons/react";
import { Dashboard } from "../features/telegram/Dashboard";
import { Downloads } from "../features/telegram/Downloads";
import { Settings as TelegramSettings } from "../features/telegram/Settings";
import { TwitterLibrary } from "../features/twitter/TwitterLibrary";
import { runtimeCapabilities } from "../shared/platform/runtime";
import { RuntimeCompatibilityNotice } from "./RuntimeCompatibilityNotice";

export type AppTab = "telegram" | "downloads" | "twitter";

interface AppShellProps {
  activeTab: AppTab;
  isSettingsOpen: boolean;
  palette: string;
  density: string;
  skipLogin: boolean;
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
  onActiveTabChange,
  onSettingsOpen,
  onSettingsClose,
  onTelegramLoginRequest,
}: AppShellProps) {
  return (
    <div
      className="app-container"
      data-palette={palette}
      data-density={density}
      data-runtime={runtimeCapabilities.kind}
    >
      <div className="sidebar">
        <button
          className={`sidebar-item ${activeTab === "telegram" ? "active" : ""}`}
          onClick={() => onActiveTabChange("telegram")}
          title="Telegram"
        >
          <ChatCircle size={22} />
        </button>
        <button
          className={`sidebar-item ${activeTab === "downloads" ? "active" : ""}`}
          onClick={() => onActiveTabChange("downloads")}
          title="Downloads"
        >
          <DownloadSimple size={22} />
        </button>
        <button
          className={`sidebar-item ${activeTab === "twitter" ? "active" : ""}`}
          onClick={() => onActiveTabChange("twitter")}
          title="Twitter / X"
        >
          <At size={22} />
        </button>
        <div style={{ flex: 1 }} />
        <button
          className={`sidebar-item ${isSettingsOpen ? "active" : ""}`}
          onClick={onSettingsOpen}
          title="Configurações"
        >
          <SettingsIcon size={22} />
        </button>
      </div>

      <div className="main-content" style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        <RuntimeCompatibilityNotice />
        {activeTab === "telegram" && (
          <div style={{ width: "100%", height: "100%" }}>
            <Dashboard skipLogin={skipLogin} onTelegramLoginRequest={onTelegramLoginRequest} />
          </div>
        )}
        {activeTab === "downloads" && (
          <div style={{ width: "100%", height: "100%" }}>
            <Downloads />
          </div>
        )}
        {activeTab === "twitter" && (
          <div style={{ width: "100%", height: "100%" }}>
            <TwitterLibrary />
          </div>
        )}
        {isSettingsOpen && (
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
        )}
      </div>
    </div>
  );
}
