import { Checkbox } from "../ui/checkbox";

/** The site selection chips: "All sites", or any subset of them. */
export function SiteSelect({
  sites,
  selected,
  onToggle,
  onClear,
}: {
  sites: { id: string; hostname: string }[] | null | undefined;
  /** The selected hostnames. */
  selected: Set<string>;
  onToggle: (hostname: string) => void;
  onClear: () => void;
}) {
  return (
    <div className="flex flex-wrap gap-2 text-sm">
      <label className="flex items-center gap-2 rounded border border-input bg-card px-3 py-1.5">
        <Checkbox checked={selected.size === 0} onCheckedChange={onClear} />
        All sites
      </label>
      {sites?.map((s) => (
        <label
          key={s.id}
          className="flex items-center gap-2 rounded border border-input bg-card px-3 py-1.5"
        >
          <Checkbox
            checked={selected.has(s.hostname)}
            onCheckedChange={() => onToggle(s.hostname)}
          />
          {s.hostname}
        </label>
      ))}
    </div>
  );
}
