import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { registerAppServiceWorker } from "./shared/platform/serviceWorker";
import "./styles/index.css";

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
        <div style={{ color: 'red', padding: '20px', background: '#222', height: '100vh', width: '100vw', whiteSpace: 'pre-wrap' }}>
          <h2>Algo deu errado!</h2>
          <p>{this.state.error?.toString()}</p>
          <p>{this.state.error?.stack}</p>
        </div>
      );
    }
    return this.props.children;
  }
}

registerAppServiceWorker();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
