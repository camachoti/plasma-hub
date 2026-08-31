import React from 'react';

export const ChatListSkeleton: React.FC<{ count?: number }> = ({ count = 10 }) => (
  <div className="skeleton-list" aria-label="Carregando conversas" role="status">
    {Array.from({ length: count }, (_, index) => (
      <div className={`skeleton-chat-row ${index === 1 ? 'active' : ''}`} key={index}>
        <div className="skeleton-shape skeleton-avatar" />
        <div className="skeleton-chat-copy">
          <div className="skeleton-line skeleton-line-title" style={{ width: `${68 - (index % 4) * 7}%` }} />
          <div className="skeleton-line skeleton-line-subtitle" style={{ width: `${82 - (index % 5) * 8}%` }} />
        </div>
        <div className="skeleton-chat-side">
          <div className="skeleton-line skeleton-line-meta" />
          {index % 3 === 0 && <div className="skeleton-shape skeleton-badge" />}
        </div>
      </div>
    ))}
  </div>
);

export const InitialDashboardSkeleton: React.FC = () => (
  <div className="initial-dashboard-skeleton" aria-label="Carregando Plasma Hub" role="status">
    <aside className="list skeleton-dashboard-list">
      <div className="list-header">
        <div className="skeleton-list-title">
          <div className="skeleton-line skeleton-heading" />
          <div className="skeleton-header-actions">
            <div className="skeleton-shape skeleton-icon-button" />
            <div className="skeleton-shape skeleton-icon-button" />
          </div>
        </div>
        <div className="skeleton-search">
          <div className="skeleton-shape skeleton-search-icon" />
          <div className="skeleton-line skeleton-search-line" />
        </div>
      </div>
      <div className="skeleton-folders">
        <div className="skeleton-folder active">
          <div className="skeleton-line" />
          <div className="skeleton-shape skeleton-folder-count" />
        </div>
        <div className="skeleton-folder">
          <div className="skeleton-line" />
          <div className="skeleton-shape skeleton-folder-count" />
        </div>
      </div>
      <ChatListSkeleton count={11} />
      <div className="skeleton-user-card">
        <div className="skeleton-shape skeleton-user-avatar" />
        <div className="skeleton-user-copy">
          <div className="skeleton-line skeleton-user-name" />
          <div className="skeleton-line skeleton-user-sub" />
        </div>
        <div className="skeleton-shape skeleton-icon-button" />
      </div>
    </aside>
    <section className="convo skeleton-dashboard-convo">
      <div className="convo-header">
        <div className="who">
          <div className="skeleton-shape skeleton-who-avatar" />
          <div className="skeleton-who-copy">
            <div className="skeleton-line skeleton-who-name" />
            <div className="skeleton-line skeleton-who-sub" />
          </div>
        </div>
        <div className="convo-header-actions">
          <div className="skeleton-shape skeleton-icon-button" />
          <div className="skeleton-shape skeleton-icon-button" />
          <div className="skeleton-shape skeleton-icon-button" />
        </div>
      </div>
      <MessageListSkeleton count={9} />
      <div className="skeleton-composer-wrap">
        <div className="skeleton-composer">
          <div className="skeleton-shape skeleton-icon-button" />
          <div className="skeleton-line skeleton-composer-input" />
          <div className="skeleton-shape skeleton-send-button" />
        </div>
      </div>
    </section>
  </div>
);

export const TopicListSkeleton: React.FC<{ count?: number }> = ({ count = 8 }) => (
  <div className="skeleton-list skeleton-topic-list" aria-label="Carregando tópicos" role="status">
    {Array.from({ length: count }, (_, index) => (
      <div className="skeleton-topic-row" key={index}>
        <div className="skeleton-shape skeleton-topic-avatar" />
        <div className="skeleton-chat-copy">
          <div className="skeleton-line skeleton-line-title" />
          <div className="skeleton-line skeleton-line-subtitle" />
        </div>
      </div>
    ))}
  </div>
);

export const MessageListSkeleton: React.FC<{ count?: number }> = ({ count = 8 }) => (
  <div className="skeleton-message-list" aria-label="Carregando mensagens" role="status">
    {Array.from({ length: count }, (_, index) => {
      const outgoing = index % 3 === 1;
      const hasMedia = index % 4 === 2;
      return (
        <div className={`skeleton-message-row ${outgoing ? 'out' : 'in'}`} key={index}>
          <div className={`skeleton-message-bubble ${hasMedia ? 'with-media' : ''}`}>
            {hasMedia && <div className="skeleton-media-block" />}
            <div className="skeleton-line skeleton-line-message" />
            <div className="skeleton-line skeleton-line-message short" />
          </div>
        </div>
      );
    })}
  </div>
);

export const SharedMediaSkeleton: React.FC<{ count?: number }> = ({ count = 6 }) => (
  <div className="info-media-grid skeleton-media-grid" aria-label="Carregando mídia compartilhada" role="status">
    {Array.from({ length: count }, (_, index) => (
      <div className="skeleton-shape skeleton-shared-media" key={index} />
    ))}
  </div>
);
