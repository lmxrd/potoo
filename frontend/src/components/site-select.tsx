/** The site selection chips: "All sites", or any subset of them. */
export function SiteSelect({
  sites,
  selected,
  onToggle,
  onClear,
}: {
  sites: { id: number; hostname: string }[] | null | undefined;
  selected: Set<number>;
  onToggle: (id: number) => void;
  onClear: () => void;
}) {
  return (
    <div className="flex flex-wrap gap-2 text-sm">
      <label className="flex items-center gap-2 rounded border border-line-strong bg-card px-3 py-1.5">
        <input type="checkbox" checked={selected.size === 0} onChange={onClear} />
        All sites
      </label>
      {sites?.map((s) => (
        <label
          key={s.id}
          className="flex items-center gap-2 rounded border border-line-strong bg-card px-3 py-1.5"
        >
          <input type="checkbox" checked={selected.has(s.id)} onChange={() => onToggle(s.id)} />
          {s.hostname}
        </label>
      ))}
    </div>
  );
}
