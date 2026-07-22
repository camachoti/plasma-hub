import { useEffect, useState } from "react";
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
import { debugWarn } from "../shared/debug/logger";
import "../styles/App.css";

const SKIP_LOGIN_KEY = "skip_login";
const TELEGRAM_TDLIB_SESSION_KEY = "telegram_tdlib_session";

async function getTelegramService() {
  const { telegramService } = await import("../features/telegram/TelegramService");
  return telegramService;
}

function App() {
  const [activeTab, setActiveTab] = useState<AppTab>("telegram");
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [skipLogin, setSkipLogin] = useState(() => appStorage.getBoolean(SKIP_LOGIN_KEY));
  const { palette, density } = useAppearance();

  useWebviewLogger();

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
  const [phoneCodeHash, setPhoneCodeHash] = useState<string | null>(null);
  const [isLoggedIn, setIsLoggedIn] = useState(
    () => appStorage.getBoolean(SKIP_LOGIN_KEY) || Boolean(appStorage.get(TELEGRAM_TDLIB_SESSION_KEY)),
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
      if (skipLogin) return;
      const savedSession = appStorage.get(TELEGRAM_TDLIB_SESSION_KEY);
      if (!savedSession) {
        setIsLoggedIn(false);
        setIsLoading(false);
        return;
      }
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
      }
    };
    checkLoginStatus();
    return () => {
      cancelled = true;
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
      } else {
        setError(res.error || "Failed to sign in");
      }
    } catch (caughtError: any) {
      setError(caughtError.message || "Unknown error");
    } finally {
      setIsLoading(false);
    }
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
    setIsLoading(false);
    setError("");
  };

  if (!isLoggedIn && !skipLogin) {
    return (
      <LoginScreen
        palette={palette}
        density={density}
        countryCode={countryCode}
        phone={phone}
        code={code}
        phoneCodeHash={phoneCodeHash}
        error={error}
        isLoading={isLoading}
        onCountryCodeChange={setCountryCode}
        onPhoneChange={setPhone}
        onCodeChange={setCode}
        onSendCode={handleSendCode}
        onSignIn={handleSignIn}
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
