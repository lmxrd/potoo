import type { ComponentProps } from "react";

export const Button = ({
  variant,
  ...props
}: ComponentProps<"button"> & { variant?: "danger" }) => (
  <button
    {...props}
    className={`rounded px-3 py-2 text-sm font-medium text-accent-ink disabled:opacity-50 ${
      variant === "danger" ? "bg-danger hover:bg-danger-hover" : "bg-accent hover:bg-accent-hover"
    }`}
  />
);
