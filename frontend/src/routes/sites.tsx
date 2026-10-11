import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import {
  createSiteMutation,
  listSitesOptions,
  listSitesQueryKey,
} from "../client/@tanstack/react-query.gen";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/input";
import { errorMessage } from "../lib/api";
import { formatDate } from "../lib/dates";

export const Route = createFileRoute("/sites")({ component: SitesPage });

function SitesPage() {
  const { session } = Route.useRouteContext();
  const isAdmin = session.user?.is_admin;
  const { data: sites, error } = useQuery(listSitesOptions());
  const queryClient = useQueryClient();
  const createSite = useMutation(createSiteMutation());
  const navigate = useNavigate();
  const [hostname, setHostname] = useState("");
  const [formError, setFormError] = useState<string>();

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Sites</h1>
      <ErrorText>{error && errorMessage(error)}</ErrorText>
      {isAdmin && (
        <form
          className="flex gap-2"
          onSubmit={async (e) => {
            e.preventDefault();
            setFormError(undefined);
            try {
              await createSite.mutateAsync({ body: { hostname } });
              setHostname("");
              await queryClient.invalidateQueries({ queryKey: listSitesQueryKey() });
            } catch (err) {
              setFormError(errorMessage(err, "Failed"));
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
            <tr className="text-left text-muted">
              <th className="py-2 font-medium">Hostname</th>
              <th className="py-2 font-medium">Created</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {sites?.length === 0 && (
              <tr>
                <td colSpan={2} className="py-2 text-muted">
                  No sites yet.
                </td>
              </tr>
            )}
            {sites?.map((s) => (
              <tr
                key={s.id}
                className="cursor-pointer hover:bg-subtle"
                onClick={() =>
                  navigate({
                    to: "/sites/$siteId",
                    params: { siteId: String(s.id) },
                  })
                }
              >
                <td className="py-2 font-medium">{s.hostname}</td>
                <td className="py-2 text-muted">
                  {formatDate(s.created_at, session.user?.date_format ?? "YYYY-MM-DD")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
      <p className="text-xs text-muted">Each subdomain is its own site.</p>
    </div>
  );
}
