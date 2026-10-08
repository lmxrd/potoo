import { useState } from "react";
import { createFileRoute, Link } from "@tanstack/react-router";
import { api, useApi } from "../api";
import { Button, Card, ErrorText, Input } from "../components";

type Site = { id: number; hostname: string; honor_dnt: number };

export const Route = createFileRoute("/")({ component: Sites });

function Sites() {
  const { session } = Route.useRouteContext();
  const { data: sites, error, reload } = useApi<Site[]>("/sites");
  const [hostname, setHostname] = useState("");
  const [formError, setFormError] = useState<string>();

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Sites</h1>
      <ErrorText>{error}</ErrorText>
      <Card>
        <ul className="divide-y divide-slate-200">
          {sites?.length === 0 && <li className="py-2 text-slate-500">No sites yet.</li>}
          {sites?.map((s) => (
            <li key={s.id} className="py-2">
              <Link
                to="/sites/$siteId"
                params={{ siteId: String(s.id) }}
                className="font-medium hover:underline"
              >
                {s.hostname}
              </Link>
            </li>
          ))}
        </ul>
      </Card>
      {session.user?.is_admin === 1 && (
        <Card title="Add a site (each subdomain is its own site)">
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
              className="flex-1"
              placeholder="blog.example.com"
              required
              value={hostname}
              onChange={(e) => setHostname(e.target.value)}
            />
            <Button type="submit">Add</Button>
          </form>
          <ErrorText>{formError}</ErrorText>
        </Card>
      )}
    </div>
  );
}
