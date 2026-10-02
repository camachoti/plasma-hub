import appIcon from "../assets/plasma-hub-logo.png";
import { runtimeCapabilities } from "../shared/platform/runtime";
import { Select, TextField } from "../design-system";

interface LoginScreenProps {
  palette: string;
  density: string;
  countryCode: string;
  phone: string;
  code: string;
  phoneCodeHash: string | null;
  error: string;
  isLoading: boolean;
  onCountryCodeChange: (value: string) => void;
  onPhoneChange: (value: string) => void;
  onCodeChange: (value: string) => void;
  onSendCode: () => void;
  onSignIn: () => void;
  onEnterWithoutLogin: () => void;
  onClearCache: () => void;
}

export function LoginScreen({
  palette,
  density,
  countryCode,
  phone,
  code,
  phoneCodeHash,
  error,
  isLoading,
  onCountryCodeChange,
  onPhoneChange,
  onCodeChange,
  onSendCode,
  onSignIn,
  onEnterWithoutLogin,
  onClearCache,
}: LoginScreenProps) {
  return (
    <div
      className="login-screen"
      data-palette={palette}
      data-density={density}
      data-runtime={runtimeCapabilities.kind}
    >
      <div className="login-card fade-in">
        <div className="login-header">
          <div className="logo-placeholder">
            <img src={appIcon} width="72" height="72" alt="Plasma Hub" />
          </div>
          <h1>Plasma Hub</h1>
          <p>Entre com o seu Telegram</p>
        </div>

        {error && <div className="error-message shake">{error}</div>}

        {!phoneCodeHash ? (
          <div className="login-form slide-up">
            <label>País e Número de Telefone</label>
            <div className="login-phone-row">
              <Select value={countryCode} onChange={onCountryCodeChange} disabled={isLoading} className="login-input" ariaLabel="Código do país" options={[['+55','🇧🇷 +55'],['+1','🇺🇸 +1'],['+351','🇵🇹 +351'],['+44','🇬🇧 +44'],['+49','🇩🇪 +49'],['+33','🇫🇷 +33'],['+39','🇮🇹 +39'],['+34','🇪🇸 +34'],['+54','🇦🇷 +54'],['+56','🇨🇱 +56'],['+57','🇨🇴 +57'],['+52','🇲🇽 +52']].map(([value, label]) => ({ value, label }))} />
              <TextField
                type="text"
                inputMode="numeric"
                placeholder="DDD + Num..."
                value={phone}
                onChange={event => onPhoneChange(event.target.value.replace(/[^0-9]/g, ""))}
                onKeyDown={event => {
                  if (event.key === "Enter" && phone && !isLoading) onSendCode();
                }}
                disabled={isLoading}
                autoFocus
                autoComplete="off"
                className="login-input"
              />
            </div>
            <button
              onClick={onSendCode}
              disabled={isLoading || !phone || !countryCode}
              className="login-button"
            >
              {isLoading ? "Enviando..." : "Enviar Código"}
            </button>

            <div className="login-links">
              <button onClick={onClearCache}>Limpar Cache e Reiniciar</button>
              <button onClick={onEnterWithoutLogin} className="accent">
                Entrar sem login
              </button>
            </div>
          </div>
        ) : (
          <div className="login-form slide-up">
            <label>Código recebido no Telegram</label>
            <TextField
              type="text"
              inputMode="numeric"
              placeholder="12345"
              value={code}
              onChange={event => onCodeChange(event.target.value.replace(/[^0-9]/g, ""))}
              onKeyDown={event => {
                if (event.key === "Enter" && code && !isLoading) onSignIn();
              }}
              disabled={isLoading}
              autoFocus
              className="login-input login-code-input"
            />
            <button
              onClick={onSignIn}
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
