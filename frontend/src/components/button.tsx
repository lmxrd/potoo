import type { ComponentProps } from "react";

export const Button = ({
  variant,
  ...props
}: ComponentProps<"button"> & { variant?: "danger" }) => (
  <button
    {...props}
    className={`rounded px-3 py-2 text-sm font-medium text-white disabled:opacity-50 ${
      variant === "danger" ? "bg-red-600 hover:bg-red-700" : "bg-slate-900 hover:bg-slate-700"
    }`}
  />
);
