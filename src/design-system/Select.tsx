import React, { useEffect, useState } from 'react';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  ariaLabel?: string;
  className?: string;
  disabled?: boolean;
}

export const Select: React.FC<SelectProps> = React.memo(({ value, options, onChange, ariaLabel, className, disabled = false }) => {
  const [open, setOpen] = useState(false);
  const selected = options.find(option => option.value === value) || options[0];

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  }, [open]);

  return (
    <div className={`custom-select ds-select ${className || ''} ${open ? 'open' : ''}`}>
      <button type="button" className="custom-select-trigger" disabled={disabled} aria-label={ariaLabel} aria-haspopup="listbox" aria-expanded={open} onClick={event => { event.stopPropagation(); setOpen(current => !current); }}>
        <span>{selected?.label || ''}</span>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><polyline points="6 9 12 15 18 9" /></svg>
      </button>
      {open && (
        <div className="custom-select-options" role="listbox" aria-label={ariaLabel} onClick={event => event.stopPropagation()}>
          {options.map(option => (
            <button key={option.value} type="button" role="option" disabled={option.disabled} aria-selected={option.value === value} className={`custom-select-option ${option.value === value ? 'selected' : ''}`} onClick={() => { onChange(option.value); setOpen(false); }}>
              {option.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
});
