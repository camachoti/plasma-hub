import React, { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { IconCornerUpLeft, IconCopy, IconDownload, IconShare3 } from "../design-system/icons";

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

export const IcoDownload = () => <IconDownload size={16} stroke={2} />;
export const IcoCopy = () => <IconCopy size={16} stroke={2} />;
export const IcoForward = () => <IconShare3 size={16} stroke={2} />;
export const IcoReply = () => <IconCornerUpLeft size={16} stroke={2} />;
