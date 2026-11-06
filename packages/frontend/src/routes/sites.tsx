import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { createSiteMutation, listSitesOptions, listSitesQueryKey } from "potoo-sdk/react-query";
import { Button } from "../components/ui/button";
import { Card, CardContent } from "../components/ui/card";
import { ErrorText } from "../components/error-text";
import { Input } from "../components/ui/input";
import { errorMessage } from "../lib/api";
import { formatDate } from "../lib/dates";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../components/ui/table";

export const Route = createFileRoute("/sites")({ component: SitesPage });

function SitesPage() {
  const { session } = Route.useRouteContext();
  const isAdmin = session.user?.is_admin;
  const { data: sites, error } = useQuery(listSitesOptions());
  const queryClient = useQueryClient();
  const createSite = useMutation({ ...createSiteMutation(), meta: { inline: true } });
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
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Hostname</TableHead>
                <TableHead>Created</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sites?.length === 0 && (
                <TableRow>
                  <TableCell colSpan={2} className="text-muted-foreground">
                    No sites yet.
                  </TableCell>
                </TableRow>
              )}
              {sites?.map((s) => (
                <TableRow
                  key={s.id}
                  className="cursor-pointer"
                  onClick={() =>
                    navigate({
                      to: "/sites/$siteId",
                      params: { siteId: s.id },
                    })
                  }
                >
                  <TableCell className="font-medium">{s.hostname}</TableCell>
                  <TableCell className="text-muted-foreground">
                    {formatDate(s.created_at, session.user?.date_format ?? "YYYY-MM-DD")}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>
      <p className="text-xs text-muted-foreground">Each subdomain is its own site.</p>
    </div>
  );
}
