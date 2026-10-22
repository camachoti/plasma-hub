import { useEffect, useLayoutEffect, useState } from "react";
import { useAppearance } from "../features/appearance/AppearanceStore";
import { appStorage } from "../shared/storage/appStorage";
import { AppShell, type AppTab } from "./AppShell";
import { LoginScreen } from "./LoginScreen";
import { useWebviewLogger } from "./useWebviewLogger";
import {
  SHARED_DOWNLOAD_URL_EVENT,
  queueSharedDownloadUrl,
  takePendingSharedDownloadUrl,
} from "../shared/platform/sharedDownloadIntent";
import { debugLog, debugWarn } from "../shared/debug/logger";
import { runtimeCapabilities } from "../shared/platform/runtime";
import { StartupLoader } from "./StartupLoader";
import "../styles/App.css";

const SKIP_LOGIN_KEY = "skip_login";
const TELEGRAM_TDLIB_SESSION_KEY = "telegram_tdlib_session";
const LOGIN_REQUIRED_STATES = new Set(["wait_phone_number", "wait_code", "wait_password"]);
const SESSION_CLOSED_STATES = new Set(["logging_out", "closing", "closed"]);

async function getTelegramService() {
  const { telegramService } = await import("../features/telegram/TelegramService");
  return telegramService;
}

function App() {
  const [activeTab, setActiveTab] = useState<AppTab>("telegram");
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [skipLogin, setSkipLogin] = useState(() => appStorage.getBoolean(SKIP_LOGIN_KEY));
  const { palette, density } = useAppearance();

  // Portals live under body, outside AppShell: keep their theme in sync too.
  useLayoutEffect(() => {
    const root = document.documentElement;
    const previousPalette = root.getAttribute('data-palette');
    const previousDensity = root.getAttribute('data-density');
    root.dataset.palette = palette;
    root.dataset.density = density;
    return () => {
      if (previousPalette === null) root.removeAttribute('data-palette');
      else root.dataset.palette = previousPalette;
      if (previousDensity === null) root.removeAttribute('data-density');
      else root.dataset.density = previousDensity;
    };
  }, [palette, density]);

  useWebviewLogger();

  useEffect(() => {
    if (!runtimeCapabilities.isTauri || runtimeCapabilities.isAndroid) return;
    const suppressWebviewMenu = (event: MouseEvent) => event.preventDefault();
    document.addEventListener('contextmenu', suppressWebviewMenu, true);
    return () => document.removeEventListener('contextmenu', suppressWebviewMenu, true);
  }, []);

  useEffect(() => {
    const onTwitterFakeChatCreated = () => {
      setActiveTab("telegram");
    };
    window.addEventListener("plasma-twitter-fake-chat-created", onTwitterFakeChatCreated);
    return () => window.removeEventListener("plasma-twitter-fake-chat-created", onTwitterFakeChatCreated);
  }, []);

  useEffect(() => {
    getTelegramService().then(service => {
      service.skipLogin = skipLogin;
    });
  }, [skipLogin]);

  const [countryCode, setCountryCode] = useState("+55");
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [phoneCodeHash, setPhoneCodeHash] = useState<string | null>(null);
  const [loginStep, setLoginStep] = useState<"phone" | "code" | "password">("phone");
  const [isLoggedIn, setIsLoggedIn] = useState(
    () => appStorage.getBoolean(SKIP_LOGIN_KEY) || Boolean(appStorage.get(TELEGRAM_TDLIB_SESSION_KEY)),
  );
  const [isCheckingAuth, setIsCheckingAuth] = useState(
    () => !appStorage.getBoolean(SKIP_LOGIN_KEY) && Boolean(appStorage.get(TELEGRAM_TDLIB_SESSION_KEY)),
  );
  const [error, setError] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    const openSharedDownloadUrl = (url?: string | null) => {
      if (url) queueSharedDownloadUrl(url);
      if (!isLoggedIn && !skipLogin) {
        appStorage.setBoolean(SKIP_LOGIN_KEY, true);
        setSkipLogin(true);
        setIsLoggedIn(true);
      }
      setIsSettingsOpen(false);
      setActiveTab("downloads");
    };

    const pendingUrl = takePendingSharedDownloadUrl();
    if (pendingUrl) openSharedDownloadUrl(pendingUrl);

    const onSharedDownloadUrl = (event: Event) => {
      const url = (event as CustomEvent<{ url?: string }>).detail?.url;
      openSharedDownloadUrl(url);
    };
    window.addEventListener(SHARED_DOWNLOAD_URL_EVENT, onSharedDownloadUrl);
    return () => window.removeEventListener(SHARED_DOWNLOAD_URL_EVENT, onSharedDownloadUrl);
  }, [isLoggedIn, skipLogin]);

  useEffect(() => {
    let cancelled = false;
    const checkLoginStatus = async () => {
      if (skipLogin) {
        setIsCheckingAuth(false);
        return;
      }
      const savedSession = appStorage.get(TELEGRAM_TDLIB_SESSION_KEY);
      if (!savedSession) {
        setIsLoggedIn(false);
        setIsCheckingAuth(false);
        return;
      }
      setIsCheckingAuth(true);
      const startedAt = performance.now();
      try {
        const telegramService = await getTelegramService();
        telegramService.skipLogin = skipLogin;
        const res = await telegramService.checkAuth();
        if (!cancelled && appStorage.get(TELEGRAM_TDLIB_SESSION_KEY) === savedSession) {
          setIsLoggedIn(res.isAuthorized);
        }
      } catch (caughtError) {
        debugWarn("Failed to check auth status", caughtError);
        if (!cancelled) setIsLoggedIn(false);
      } finally {
        debugLog("[Startup] Verificação da sessão Telegram:", Math.round(performance.now() - startedAt), "ms");
        if (!cancelled) setIsCheckingAuth(false);
      }
    };
    checkLoginStatus();
    return () => {
      cancelled = true;
    };
  }, [skipLogin]);

  useEffect(() => {
    if (!runtimeCapabilities.isTauri || skipLogin) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    getTelegramService().then(async service => {
      const handleAuthState = (state: string) => {
        if (disposed || skipLogin) return;
        if (state === "wait_tdlib_parameters") return;
        if (state === "ready") {
          setPassword("");
          setIsLoggedIn(true);
          setIsCheckingAuth(false);
          appStorage.set(TELEGRAM_TDLIB_SESSION_KEY, "ready");
          return;
        }
        if (LOGIN_REQUIRED_STATES.has(state) || SESSION_CLOSED_STATES.has(state)) {
          if (state === "wait_password") setLoginStep("password");
          else if (state === "wait_code") {
            setPhoneCodeHash("tdlib");
            setLoginStep("code");
          }
          else if (state === "wait_phone_number" || SESSION_CLOSED_STATES.has(state)) setLoginStep("phone");
          setIsLoggedIn(false);
          setIsCheckingAuth(false);
          appStorage.remove(TELEGRAM_TDLIB_SESSION_KEY);
        }
      };
      unlisten = await service.onTdlibAuthState(handleAuthState);
      await service.tdlibInit(8000);
      const current = await service.tdlibStatus();
      handleAuthState(current.state);
    }).catch(error => debugWarn("Failed to subscribe to TDLib auth state", error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [skipLogin]);

  const handleSendCode = async () => {
    if (!phone || !countryCode) return;

    const fullPhone = countryCode + phone;
    const cleanDigits = fullPhone.replace(/[^0-9]/g, "");
    if (cleanDigits.length < 8 || cleanDigits.length > 15) {
      setError("Por favor, digite um número de telefone válido contendo DDI e DDD. (Ex: +55 11 999999999)");
      return;
    }

    setIsLoading(true);
    setError("");
    try {
      const telegramService = await getTelegramService();
      telegramService.skipLogin = skipLogin;
      const res = await telegramService.sendCode(fullPhone);
      if (res.success && res.phoneCodeHash) {
        setPhoneCodeHash(res.phoneCodeHash);
        setLoginStep("code");
      } else {
        setError(res.error || "Failed to send code");
      }
    } catch (caughtError: any) {
      setError(caughtError.message || "Unknown error");
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
      const telegramService = await getTelegramService();
      telegramService.skipLogin = skipLogin;
      const res = await telegramService.signIn(fullPhone, phoneCodeHash, code);
      if (res.success) {
        appStorage.remove(SKIP_LOGIN_KEY);
        setSkipLogin(false);
        setIsLoggedIn(true);
      } else if (res.requiresPassword) {
        setLoginStep("password");
        setCode("");
      } else {
        setError(res.error || "Failed to sign in");
      }
    } catch (caughtError: any) {
      setError(caughtError.message || "Unknown error");
    } finally {
      setIsLoading(false);
    }
  };

  const handlePasswordSignIn = async () => {
    if (!password) return;
    setIsLoading(true);
    setError("");
    try {
      const telegramService = await getTelegramService();
      const res = await telegramService.signInWithPassword(password);
      if (res.success) {
        setPassword("");
        appStorage.remove(SKIP_LOGIN_KEY);
        setSkipLogin(false);
        setIsLoggedIn(true);
      } else {
        setError(res.error || "Não foi possível validar a senha.");
      }
    } catch (caughtError) {
      setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
    } finally {
      setIsLoading(false);
    }
  };

  const handleChangePhone = () => {
    setLoginStep("phone");
    setPhoneCodeHash(null);
    setCode("");
    setPassword("");
    setError("");
  };

  const handleEnterWithoutLogin = () => {
    appStorage.setBoolean(SKIP_LOGIN_KEY, true);
    setSkipLogin(true);
    setIsLoggedIn(true);
    setActiveTab("telegram");
    setError("");
  };

  const handleClearCache = () => {
    appStorage.remove(TELEGRAM_TDLIB_SESSION_KEY);
    appStorage.remove(SKIP_LOGIN_KEY);
    window.location.reload();
  };

  const handleTelegramLoginRequest = async () => {
    const telegramService = await getTelegramService();
    telegramService.resetSession();
    appStorage.remove(SKIP_LOGIN_KEY);
    setSkipLogin(false);
    setIsLoggedIn(false);
    setPhoneCodeHash(null);
    setCode("");
    setPassword("");
    setLoginStep("phone");
    setIsLoading(false);
    setIsCheckingAuth(false);
    setError("");
  };

  if (isCheckingAuth && !skipLogin) {
    return (
      <div className="app-container" data-palette={palette} data-density={density}>
        <StartupLoader />
      </div>
    );
  }

  if (!isLoggedIn && !skipLogin) {
    return (
      <LoginScreen
        palette={palette}
        density={density}
        countryCode={countryCode}
        phone={phone}
        code={code}
        password={password}
        loginStep={loginStep}
        error={error}
        isLoading={isLoading}
        onCountryCodeChange={setCountryCode}
        onPhoneChange={setPhone}
        onCodeChange={setCode}
        onPasswordChange={setPassword}
        onSendCode={handleSendCode}
        onSignIn={handleSignIn}
        onPasswordSignIn={handlePasswordSignIn}
        onChangePhone={handleChangePhone}
        onEnterWithoutLogin={handleEnterWithoutLogin}
        onClearCache={handleClearCache}
      />
    );
  }

  return (
    <AppShell
      activeTab={activeTab}
      isSettingsOpen={isSettingsOpen}
      palette={palette}
      density={density}
      skipLogin={skipLogin}
      onActiveTabChange={setActiveTab}
      onSettingsOpen={() => setIsSettingsOpen(true)}
      onSettingsClose={() => setIsSettingsOpen(false)}
      onTelegramLoginRequest={handleTelegramLoginRequest}
    />
  );
}

export default App;
