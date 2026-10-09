import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { api, useApi } from "../api";
import { Button, Card, ErrorText, Input } from "../components";

type Site = {
  id: number;
  hostname: string;
  honor_dnt: number;
  created_at: string;
};

export const Route = createFileRoute("/sites")({ component: SitesPage });

function SitesPage() {
  const { session } = Route.useRouteContext();
  const isAdmin = session.user?.is_admin === 1;
  const { data: sites, error, reload } = useApi<Site[]>("/sites");
  const navigate = useNavigate();
  const [hostname, setHostname] = useState("");
  const [formError, setFormError] = useState<string>();

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Sites</h1>
      <ErrorText>{error}</ErrorText>
      {isAdmin && (
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setFormError(undefined);
            try {
              await api("/sites", { body: { hostname } });
              setHostname("");
              reload();
            } catch (err) {
              setFormError(err instanceof Error ? err.message : "Failed");
            }
          }}
        >
          <Input
            className="max-w-sm"
            placeholder="blog.example.com"
            required
            value={hostname}
            onChange={(e) => setHostname(e.target.value)}
          />
          <Button type="submit">Add site</Button>
          <ErrorText>{formError}</ErrorText>
        </form>
      )}
      <Card>
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-slate-500">
              <th className="py-2 font-medium">Hostname</th>
              <th className="py-2 font-medium">Created</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-200">
            {sites?.length === 0 && (
              <tr>
                <td colSpan={2} className="py-2 text-slate-500">
                  No sites yet.
                </td>
              </tr>
            )}
            {sites?.map((s) => (
              <tr
                key={s.id}
                className="cursor-pointer hover:bg-slate-50"
                onClick={() =>
                  navigate({
                    to: "/sites/$siteId",
                    params: { siteId: String(s.id) },
                  })
                }
              >
                <td className="py-2 font-medium">{s.hostname}</td>
                <td className="py-2 text-slate-500">{s.created_at.slice(0, 10)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <p className="text-xs text-slate-500">Each subdomain is its own site.</p>
    </div>
  );
}
