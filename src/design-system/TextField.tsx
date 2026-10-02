import React, { forwardRef } from 'react';

export interface TextFieldProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'size'> {
  /** Use inline inside a designed wrapper that owns the border and focus ring. */
  appearance?: 'standard' | 'inline';
}

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { appearance = 'standard', className = '', ...props },
  ref,
) {
  const appearanceClass = appearance === 'inline' ? 'ds-input-inline' : 'ds-input';
  return <input ref={ref} className={`${appearanceClass} ${className}`.trim()} {...props} />;
});

export interface TextAreaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  appearance?: 'standard' | 'inline';
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
  { appearance = 'standard', className = '', ...props },
  ref,
) {
  const appearanceClass = appearance === 'inline' ? 'ds-textarea-inline' : 'ds-textarea';
  return <textarea ref={ref} className={`${appearanceClass} ${className}`.trim()} {...props} />;
});
