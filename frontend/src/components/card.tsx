import type { ReactNode } from "react";

export const Card = ({ title, children }: { title?: string; children: ReactNode }) => (
  <section className="rounded-lg border border-line bg-card p-4">
    {title && <h2 className="mb-3 text-sm font-medium text-muted">{title}</h2>}
    {children}
  </section>
);
