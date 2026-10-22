import React from 'react';

type NativeButtonProps = React.ComponentPropsWithoutRef<'button'>;

const mergeClassNames = (...classNames: Array<string | undefined>) => (
  classNames.filter(Boolean).join(' ')
);

export const Button = React.forwardRef<HTMLButtonElement, NativeButtonProps>(
  ({ className, type = 'button', ...props }, ref) => (
    <button ref={ref} type={type} className={mergeClassNames('ds-button', className)} {...props} />
  ),
);

Button.displayName = 'Button';

export const IconButton = React.forwardRef<HTMLButtonElement, NativeButtonProps>(
  ({ className, type = 'button', ...props }, ref) => (
    <button ref={ref} type={type} className={mergeClassNames('ds-icon-button', className)} {...props} />
  ),
);

IconButton.displayName = 'IconButton';
