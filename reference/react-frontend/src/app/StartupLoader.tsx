export function StartupLoader() {
  return (
    <div className="startup-screen" role="status" aria-label="Carregando Plasma Hub">
      <div className="startup-loader" aria-hidden="true">
        <span className="startup-loader__halo" />
        <span className="startup-loader__outer">
          <span className="startup-loader__inner">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M15 10l-4 4 6 6 4-16-18 7 4 2 2 6 3-4" />
            </svg>
          </span>
        </span>
      </div>
    </div>
  );
}
