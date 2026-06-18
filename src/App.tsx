import { useState, useEffect } from "react";
import { At, ChatCircle, DownloadSimple, Gear as SettingsIcon } from "@phosphor-icons/react";
import { Dashboard } from "./features/telegram/Dashboard";
import { Downloads } from "./features/telegram/Downloads";
import { Settings as TelegramSettings } from "./features/telegram/Settings";
import { telegramService } from "./features/telegram/TelegramService";
import { TwitterLibrary } from "./features/twitter/TwitterLibrary";
import appIcon from "../build/icon.png";
import "./styles/App.css";

function App() {
  const [activeTab, setActiveTab] = useState("telegram");
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [skipLogin, setSkipLogin] = useState(() => localStorage.getItem("skip_login") === "true");

  // Logger hook
  useEffect(() => {
    const sendLog = (level: string, ...args: any[]) => {
      const message = args.map(arg => {
        if (arg instanceof Error) {
          return `${arg.message}\n${arg.stack}`;
        }
        if (typeof arg === 'object') {
          try { return JSON.stringify(arg); } catch (_) { return String(arg); }
        }
        return String(arg);
      }).join(' ');

      fetch('http://127.0.0.1:1425/log', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          time: new Date().toLocaleTimeString(),
          level,
          message
        })
      }).catch(() => {});
    };

    const originalLog = console.log;
    const originalWarn = console.warn;
    const originalError = console.error;

    console.log = (...args) => {
      originalLog(...args);
      sendLog('LOG', ...args);
    };
    console.warn = (...args) => {
      originalWarn(...args);
      sendLog('WARN', ...args);
    };
    console.error = (...args) => {
      originalError(...args);
      sendLog('ERROR', ...args);
    };

    console.log("Webview logger initialized!");

    // Capture service worker messages
    if ('serviceWorker' in navigator) {
      const swListener = (event: MessageEvent) => {
        sendLog('SW_MSG', event.data);
      };
      navigator.serviceWorker.addEventListener('message', swListener);
      console.log("Service Worker message listener registered in App.tsx logger");
      return () => {
        console.log = originalLog;
        console.warn = originalWarn;
        console.error = originalError;
        navigator.serviceWorker.removeEventListener('message', swListener);
      };
    }

    return () => {
      console.log = originalLog;
      console.warn = originalWarn;
      console.error = originalError;
    };
  }, []);

  useEffect(() => {
    const onTwitterFakeChatCreated = () => {
      setActiveTab("telegram");
    };
    window.addEventListener("plasma-twitter-fake-chat-created", onTwitterFakeChatCreated);
    return () => window.removeEventListener("plasma-twitter-fake-chat-created", onTwitterFakeChatCreated);
  }, []);

  useEffect(() => {
    telegramService.skipLogin = skipLogin;
  }, [skipLogin]);

  const [countryCode, setCountryCode] = useState("+55");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [phoneCodeHash, setPhoneCodeHash] = useState<string | null>(null);
  const [isLoggedIn, setIsLoggedIn] = useState(() => localStorage.getItem("skip_login") === "true" || !!localStorage.getItem("telegram_session"));
  const [error, setError] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    const checkLoginStatus = async () => {
      if (skipLogin) return;
      setIsLoading(true);
      try {
        const res = await telegramService.checkAuth();
        setIsLoggedIn(res.isAuthorized);
      } catch (e) {
        console.error("Failed to check auth status", e);
        setIsLoggedIn(false);
      } finally {
        setIsLoading(false);
      }
    };
    checkLoginStatus();
  }, [skipLogin]);

  const handleSendCode = async () => {
    if (!phone || !countryCode) return;
    
    const fullPhone = countryCode + phone;
    const cleanDigits = fullPhone.replace(/[^0-9]/g, '');
    if (cleanDigits.length < 8 || cleanDigits.length > 15) {
      setError("Por favor, digite um número de telefone válido contendo DDI e DDD. (Ex: +55 11 999999999)");
      return;
    }

    setIsLoading(true);
    setError("");
    try {
      const res = await telegramService.sendCode(fullPhone);
      if (res.success && res.phoneCodeHash) {
        setPhoneCodeHash(res.phoneCodeHash);
      } else {
        setError(res.error || "Failed to send code");
      }
    } catch (e: any) {
      setError(e.message || "Unknown error");
    } finally {
      setIsLoading(false);
    }
  };

  const handleSignIn = async () => {
    if (!phoneCodeHash || !code) return;
    setIsLoading(true);
    setError("");
    const fullPhone = countryCode + phone;
    try {
      const res = await telegramService.signIn(fullPhone, phoneCodeHash, code);
      if (res.success) {
        localStorage.removeItem("skip_login");
        setSkipLogin(false);
        setIsLoggedIn(true);
      } else {
        setError(res.error || "Failed to sign in");
      }
    } catch (e: any) {
      setError(e.message || "Unknown error");
    } finally {
      setIsLoading(false);
    }
  };

  const handleEnterWithoutLogin = () => {
    localStorage.setItem("skip_login", "true");
    setSkipLogin(true);
    setIsLoggedIn(true);
    setActiveTab("telegram");
    setError("");
  };

  if (!isLoggedIn && !skipLogin) {
    return (
      <div className="App" style={{ height: "100vh", width: "100vw", display: "flex", alignItems: "center", justifyContent: "center", background: "radial-gradient(circle at 50% -20%, var(--bg-3), var(--bg-0))", color: "var(--text-0)" }}>
        <div className="login-container fade-in" style={{ backgroundColor: "var(--bg-panel)", padding: "40px", borderRadius: "16px", boxShadow: "0 8px 32px rgba(0,0,0,0.2)", width: "320px", textAlign: "center", maxWidth: "90vw" }}>
          <div className="login-header" style={{ marginBottom: "32px" }}>
            <div className="logo-placeholder" style={{ marginBottom: "16px" }}>
              <img src={appIcon} width="72" height="72" alt="Plasma Hub" style={{ borderRadius: "16px" }} />
            </div>
            <h1 style={{ fontSize: "24px", margin: "0 0 8px 0" }}>Plasma Hub</h1>
            <p style={{ color: "var(--text-muted)", margin: 0 }}>Entre com o seu Telegram</p>
          </div>
          
          {error && <div className="error-message shake" style={{ backgroundColor: "var(--accent-primary)", color: "white", padding: "12px", borderRadius: "8px", marginBottom: "20px" }}>{error}</div>}
          
          {!phoneCodeHash ? (
            <div className="login-form slide-up" style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
              <label style={{ textAlign: "left", fontSize: "14px", color: "var(--text-muted)" }}>País e Número de Telefone</label>
              <div style={{ display: "flex", gap: "8px" }}>
                <select 
                  value={countryCode}
                  onChange={e => setCountryCode(e.target.value)}
                  disabled={isLoading}
                  className="login-input"
                  style={{ width: "95px" }}
                >
                  <option value="+55">🇧🇷 +55</option>
                  <option value="+1">🇺🇸 +1</option>
                  <option value="+351">🇵🇹 +351</option>
                  <option value="+44">🇬🇧 +44</option>
                  <option value="+49">🇩🇪 +49</option>
                  <option value="+33">🇫🇷 +33</option>
                  <option value="+39">🇮🇹 +39</option>
                  <option value="+34">🇪🇸 +34</option>
                  <option value="+54">🇦🇷 +54</option>
                  <option value="+56">🇨🇱 +56</option>
                  <option value="+57">🇨🇴 +57</option>
                  <option value="+52">🇲🇽 +52</option>
                </select>
                <input 
                  type="text" 
                  inputMode="numeric"
                  placeholder="DDD + Num..." 
                  value={phone} 
                  onChange={e => setPhone(e.target.value.replace(/[^0-9]/g, ''))}
                  onKeyDown={e => { if (e.key === "Enter" && phone && !isLoading) handleSendCode(); }}
                  disabled={isLoading}
                  autoFocus
                  autoComplete="off"
                  className="login-input"
                  style={{ flex: 1, minWidth: 0 }}
                />
              </div>
              <button 
                onClick={handleSendCode} 
                disabled={isLoading || !phone || !countryCode}
                className="login-button"
              >
                {isLoading ? "Enviando..." : "Enviar Código"}
              </button>
              
              <div style={{ display: "flex", justifyContent: "center", gap: "12px", marginTop: "8px", flexWrap: "wrap" }}>
                <button
                  onClick={() => {
                    localStorage.removeItem("telegram_session");
                    localStorage.removeItem("skip_login");
                    window.location.reload();
                  }}
                  style={{
                    background: "none",
                    border: "none",
                    color: "var(--text-3)",
                    fontSize: "12px",
                    cursor: "pointer",
                    textDecoration: "underline"
                  }}
                >
                  Limpar Cache e Reiniciar
                </button>
                <button
                  onClick={handleEnterWithoutLogin}
                  style={{
                    background: "none",
                    border: "none",
                    color: "var(--accent)",
                    fontSize: "12px",
                    cursor: "pointer",
                    textDecoration: "underline"
                  }}
                >
                  Entrar sem login
                </button>
              </div>
            </div>
          ) : (
            <div className="login-form slide-up" style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
              <label style={{ textAlign: "left", fontSize: "14px", color: "var(--text-muted)" }}>Código recebido no Telegram</label>
              <input 
                type="text" 
                inputMode="numeric"
                placeholder="12345" 
                value={code} 
                onChange={e => setCode(e.target.value.replace(/[^0-9]/g, ''))}
                onKeyDown={e => { if (e.key === "Enter" && code && !isLoading) handleSignIn(); }}
                disabled={isLoading}
                autoFocus
                className="login-input"
                style={{ letterSpacing: "4px", textAlign: "center" }}
              />
              <button 
                onClick={handleSignIn} 
                disabled={isLoading || !code}
                className="login-button"
              >
                {isLoading ? "Entrando..." : "Entrar"}
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="app-container">
      {/* Sidebar */}
      <div className="sidebar">
        <button 
          className={`sidebar-item ${activeTab === "telegram" ? "active" : ""}`}
          onClick={() => setActiveTab("telegram")}
          title="Telegram"
        >
          <ChatCircle size={22} />
        </button>
        <button 
          className={`sidebar-item ${activeTab === "downloads" ? "active" : ""}`}
          onClick={() => setActiveTab("downloads")}
        >
          <DownloadSimple size={22} />
        </button>
        <button 
          className={`sidebar-item ${activeTab === "twitter" ? "active" : ""}`}
          onClick={() => setActiveTab("twitter")}
          title="Twitter / X"
        >
          <At size={22} />
        </button>
        <div style={{ flex: 1 }}></div>
        <button 
          className={`sidebar-item ${isSettingsOpen ? "active" : ""}`}
          onClick={() => setIsSettingsOpen(true)}
        >
          <SettingsIcon size={22} />
        </button>
      </div>

      {/* Main Content Area */}
      <div className="main-content" style={{ flex: 1, position: "relative", overflow: "hidden" }}>
        {activeTab === "telegram" && (
          <div style={{ width: "100%", height: "100%" }}>
            <Dashboard skipLogin={skipLogin} />
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
          <div style={{ position: "absolute", top: 0, left: 0, width: "100%", height: "100%", background: "rgba(0,0,0,0.6)", backdropFilter: "blur(4px)", display: "flex", alignItems: "center", justifyContent: "center", zIndex: 100 }}>
            <TelegramSettings onClose={() => setIsSettingsOpen(false)} />
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
