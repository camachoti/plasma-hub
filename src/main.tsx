import React from "react";
import ReactDOM from "react-dom/client";
import { registerAppServiceWorker } from "./shared/platform/serviceWorker";
import "./styles/index.css";
import "./styles/design-system.css";

class ErrorBoundary extends React.Component<{children: React.ReactNode}, {hasError: boolean, error: Error | null}> {
  constructor(props: {children: React.ReactNode}) {
    super(props);
    this.state = { hasError: false, error: null };
  }
  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }
  render() {
    if (this.state.hasError) {
      return (
        <div className="fatal-error-screen" role="alert">
          <div className="fatal-error-card ds-surface">
            <span className="fatal-error-icon" aria-hidden="true">!</span>
            <h1>Não foi possível abrir o Plasma Hub</h1>
            <p>Recarregue o aplicativo para tentar novamente. Seus downloads concluídos e preferências serão preservados.</p>
            <button type="button" className="fatal-error-reload" onClick={() => window.location.reload()}>
              Recarregar aplicativo
            </button>
            <details className="fatal-error-details">
              <summary>Detalhes técnicos</summary>
              <pre>{this.state.error?.stack || this.state.error?.toString() || 'Erro desconhecido'}</pre>
            </details>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}

registerAppServiceWorker();

const rootElement = document.getElementById("root");
if (!rootElement) throw new Error("Elemento raiz do Plasma Hub não encontrado.");

const root = ReactDOM.createRoot(rootElement);
let appMounted = false;

const renderBootstrapError = (reason: unknown) => {
  if (appMounted) return;
  const error = reason instanceof Error ? reason : new Error(String(reason));
  root.render(
    <div role="alert" style={{ minHeight: "100vh", display: "grid", placeItems: "center", padding: 24, background: "#18232c", color: "#f2f5f6", fontFamily: "system-ui, sans-serif" }}>
      <section style={{ width: "min(720px, 100%)", padding: 24, border: "1px solid #52616b", borderRadius: 12, background: "#1e2a34" }}>
        <h1 style={{ marginTop: 0 }}>Falha ao iniciar a interface</h1>
        <p>O processo Tauri abriu, mas não conseguiu carregar o frontend.</p>
        <pre style={{ maxHeight: "50vh", overflow: "auto", whiteSpace: "pre-wrap", overflowWrap: "anywhere", color: "#ffb4a9" }}>{error.stack || error.message}</pre>
      </section>
    </div>,
  );
};

window.addEventListener("error", event => renderBootstrapError(event.error || event.message));
window.addEventListener("unhandledrejection", event => renderBootstrapError(event.reason));

void import("./App")
  .then(({ default: App }) => {
    appMounted = true;
    root.render(
      <React.StrictMode>
        <ErrorBoundary>
          <App />
        </ErrorBoundary>
      </React.StrictMode>,
    );
  })
  .catch(renderBootstrapError);
