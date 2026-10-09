import type { ComponentProps } from "react";

export const Input = (props: ComponentProps<"input">) => (
  <input
    {...props}
    className={`rounded border border-slate-300 px-3 py-2 text-sm ${props.className ?? ""}`}
  />
);
