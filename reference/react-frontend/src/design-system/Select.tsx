import React, { useEffect, useId, useRef, useState } from 'react';
import { IconChevronDown } from "./icons";

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
  const [focusedIndex, setFocusedIndex] = useState(-1);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const listboxId = useId();
  const selected = options.find(option => option.value === value) || options[0];

  const findEnabledIndex = (start: number, direction: 1 | -1) => {
    if (!options.length) return -1;
    for (let offset = 0; offset < options.length; offset += 1) {
      const index = (start + offset * direction + options.length) % options.length;
      if (!options[index]?.disabled) return index;
    }
    return -1;
  };

  const focusOption = (index: number) => {
    if (index < 0) return;
    setFocusedIndex(index);
    optionRefs.current[index]?.focus();
  };

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    document.addEventListener('click', close);
    return () => document.removeEventListener('click', close);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const selectedIndex = Math.max(0, options.findIndex(option => option.value === value));
    focusOption(findEnabledIndex(selectedIndex, 1));
  }, [open]);

  const closeAndRestoreFocus = () => {
    setOpen(false);
    triggerRef.current?.focus();
  };

  const handleOptionKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault();
      closeAndRestoreFocus();
      return;
    }
    if (event.key === 'Tab') {
      setOpen(false);
      return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    if (event.key === 'Home') focusOption(findEnabledIndex(0, 1));
    else if (event.key === 'End') focusOption(findEnabledIndex(options.length - 1, -1));
    else focusOption(findEnabledIndex(focusedIndex + (event.key === 'ArrowDown' ? 1 : -1), event.key === 'ArrowDown' ? 1 : -1));
  };

  return (
    <div className={`custom-select ds-select ${className || ''} ${open ? 'open' : ''}`}>
      <button
        ref={triggerRef}
        type="button"
        className="custom-select-trigger"
        disabled={disabled}
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listboxId : undefined}
        onKeyDown={event => {
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault();
            setOpen(true);
          }
        }}
        onClick={event => { event.stopPropagation(); setOpen(current => !current); }}
      >
        <span>{selected?.label || ''}</span>
        <IconChevronDown size={14} stroke={2} aria-hidden="true" />
      </button>
      {open && (
        <div id={listboxId} className="custom-select-options" role="listbox" aria-label={ariaLabel} onClick={event => event.stopPropagation()}>
          {options.map((option, index) => (
            <button
              ref={element => { optionRefs.current[index] = element; }}
              key={option.value}
              type="button"
              role="option"
              disabled={option.disabled}
              aria-selected={option.value === value}
              className={`custom-select-option ${option.value === value ? 'selected' : ''}`}
              onFocus={() => setFocusedIndex(index)}
              onKeyDown={handleOptionKeyDown}
              onClick={() => { onChange(option.value); closeAndRestoreFocus(); }}
            >
              {option.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
});
