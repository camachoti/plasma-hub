import type { ReactNode } from 'react';
import '../styles/EmptyState.css';

interface EmptyStateProps {
  icon: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon, title, description, action, className = '' }: EmptyStateProps) {
  return (
    <div className={`workspace-empty ${className}`}>
      <div className="workspace-empty-icon" aria-hidden="true">{icon}</div>
      <h3>{title}</h3>
      {description && <p>{description}</p>}
      {action && <div className="workspace-empty-action">{action}</div>}
    </div>
  );
}
