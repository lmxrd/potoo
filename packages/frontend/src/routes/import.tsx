import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { importHitsMutation } from "potoo-sdk/react-query";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { ErrorText } from "../components/common/error-text";
import { errorMessage } from "../lib/api";
import { problemsOf } from "../lib/import";
import { Checkbox } from "../components/ui/checkbox";
import { Input } from "../components/ui/input";
import { Textarea } from "../components/ui/textarea";

export const Route = createFileRoute("/import")({
  beforeLoad: ({ context }) => {
    if (!context.session.user?.is_admin) throw redirect({ to: "/" });
  },
  component: Import,
});

function Import() {
  const queryClient = useQueryClient();
  const importHits = useMutation({ ...importHitsMutation(), meta: { inline: true } });
  const [file, setFile] = useState<File | null>(null);
  const [text, setText] = useState("");
  const [createSites, setCreateSites] = useState(false);
  const [done, setDone] = useState<{ imported: number; sitesCreated: number }>();
  const [failure, setFailure] = useState<unknown>();
  const problems = problemsOf(failure);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Import hits</h1>
      <Card>
        <CardContent>
          <form
            className="space-y-4"
            onSubmit={async (e) => {
              e.preventDefault();
              setDone(undefined);
              setFailure(undefined);
              // The pasted text is a file like any other to the API.
              const csv = file ?? (text.trim() ? new File([text], "hits.csv") : null);
              if (!csv) {
                setFailure(new Error("Choose a CSV file or paste its content"));
                return;
              }
              try {
                const result = await importHits.mutateAsync({
                  body: { file: csv },
                  query: { create_sites: createSites },
                  // The generated schema takes a binary part for a string.
                  requestValidator: async (data) => data,
                });
                setDone({ imported: result.imported, sitesCreated: result.sites_created });
                setFile(null);
                setText("");
                queryClient.invalidateQueries();
              } catch (err) {
                setFailure(err);
              }
            }}
          >
            <p className="text-sm text-muted-foreground">
              A CSV in the format of the export:{" "}
              <code>site,created_at,kind,path,name,referrer,country,device,browser,os</code>, with
              that header row, optionally followed by a <code>visitor</code> column. Every site must
              already exist, unless you allow creating them. Nothing is added if any row has a
              problem. Hits without a visitor count as page views but add no unique visitors, and
              rows are not checked for duplicates: importing a file twice adds its hits twice. Up to
              256 MB.
            </p>
            <label className="block space-y-1 text-sm">
              <span className="font-medium">CSV file</span>
              <Input
                type="file"
                accept=".csv,text/csv"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
            </label>
            <label className="block space-y-1 text-sm">
              <span className="font-medium">or paste CSV content</span>
              <Textarea
                value={text}
                onChange={(e) => setText(e.target.value)}
                disabled={file !== null}
                rows={8}
                spellCheck={false}
                className="font-mono text-xs md:text-xs"
              />
            </label>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={createSites}
                onCheckedChange={(checked) => setCreateSites(checked === true)}
              />
              Create the sites that do not exist
            </label>
            <Button type="submit" disabled={importHits.isPending}>
              {importHits.isPending ? "Importing…" : "Import"}
            </Button>
          </form>
        </CardContent>
      </Card>
      {done !== undefined && (
        <Card>
          <CardContent>
            <p className="text-sm">
              Imported {done.imported} {done.imported === 1 ? "hit" : "hits"}
              {done.sitesCreated > 0 &&
                ` and created ${done.sitesCreated} ${done.sitesCreated === 1 ? "site" : "sites"}`}
              .
            </p>
          </CardContent>
        </Card>
      )}
      {failure !== undefined && (
        <Card>
          <CardHeader>
            <CardTitle>Nothing was imported</CardTitle>
          </CardHeader>
          <CardContent>
            <ErrorText>{errorMessage(failure)}</ErrorText>
            {problems && (
              <>
                <ul className="mt-3 divide-y divide-border text-sm">
                  {problems.problems.map((p, i) => (
                    <li key={i} className="py-1.5">
                      <span className="text-muted-foreground">
                        {p.line > 0 ? `Line ${p.line}` : "File"}
                        {p.column ? `, ${p.column}` : ""}:
                      </span>{" "}
                      {p.message}
                    </li>
                  ))}
                </ul>
                {problems.total > problems.problems.length && (
                  <p className="mt-2 text-sm text-muted-foreground">
                    and {problems.total - problems.problems.length} more
                  </p>
                )}
              </>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
