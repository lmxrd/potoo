import type { ReactNode } from "react";

export const ErrorText = ({ children }: { children?: ReactNode }) =>
  children ? <p className="text-sm text-danger">{children}</p> : null;
