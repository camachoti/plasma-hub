import appIcon from "../../build/icon.png";

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
    <div className="login-screen" data-palette={palette} data-density={density}>
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
              <select
                value={countryCode}
                onChange={event => onCountryCodeChange(event.target.value)}
                disabled={isLoading}
                className="login-input"
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
            <input
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
