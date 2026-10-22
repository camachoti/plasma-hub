import type { HTMLAttributes } from "react";

export type LoadingIndicatorSize = "xs" | "sm" | "md" | "lg" | "xl";

interface LoadingIndicatorProps extends HTMLAttributes<HTMLSpanElement> {
  size?: LoadingIndicatorSize;
  label?: string;
}

/** Shared animated activity mark. Use a nearby text label for visible context. */
export function LoadingIndicator({
  size = "md",
  label,
  className = "",
  ...props
}: LoadingIndicatorProps) {
  return (
    <span
      {...props}
      className={`loading-indicator loading-indicator--${size}${className ? ` ${className}` : ""}`}
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      <span className="loading-indicator__orbit" />
    </span>
  );
}
