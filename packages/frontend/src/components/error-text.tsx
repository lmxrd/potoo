import type { ReactNode } from "react";

export const ErrorText = ({ children }: { children?: ReactNode }) =>
  children ? (
    <p role="alert" className="text-sm text-destructive">
      {children}
    </p>
  ) : null;
