import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';

interface ContextMenuSeparator {
  separator: true;
}

interface ContextMenuAction {
  label: string;
  icon?: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
  danger?: boolean;
}

type ContextMenuItem = ContextMenuAction | ContextMenuSeparator;

interface ContextMenuProps {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
  palette?: string;
  density?: string;
}

export const ContextMenu: React.FC<ContextMenuProps> = ({ x, y, items, onClose, palette, density }) => {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleEsc = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    document.addEventListener('mousedown', handleClick);
    document.addEventListener('keydown', handleEsc);
    return () => {
      document.removeEventListener('mousedown', handleClick);
      document.removeEventListener('keydown', handleEsc);
    };
  }, [onClose]);

  useEffect(() => {
    if (!menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    if (x + rect.width > vw) menuRef.current.style.left = `${Math.max(8, vw - rect.width - 8)}px`;
    if (y + rect.height > vh) menuRef.current.style.top = `${Math.max(8, vh - rect.height - 8)}px`;
  }, [x, y]);

  const menu = (
    <div
      ref={menuRef}
      className="context-menu"
      data-palette={palette}
      data-density={density}
      style={{ position: 'fixed', left: x, top: y, zIndex: 99999 }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, index) => {
        if ('separator' in item) {
          return <div key={index} className="context-menu-sep" />;
        }
        return (
          <button
            key={index}
            type="button"
            className={`context-menu-item ${item.danger ? 'danger' : ''}`}
            onClick={() => { if (!item.disabled) { item.onClick(); onClose(); } }}
            disabled={item.disabled}
          >
            {item.icon && <span className="context-menu-icon">{item.icon}</span>}
            <span className="context-menu-label">{item.label}</span>
          </button>
        );
      })}
    </div>
  );

  return createPortal(menu, document.body);
};

/* SVG icons used in context menus */
export const IcoDownload = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/>
  </svg>
);
export const IcoCopy = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
  </svg>
);
export const IcoForward = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="15 17 20 12 15 7"/><path d="M4 18v-2a4 4 0 0 1 4-4h12"/>
  </svg>
);
export const IcoReply = () => (
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
    <polyline points="9 17 4 12 9 7"/><path d="M20 18v-2a4 4 0 0 0-4-4H4"/>
  </svg>
);
