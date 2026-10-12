import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";
import { importHitsMutation } from "../client/@tanstack/react-query.gen";
import type { ImportProblem } from "../client/types.gen";
import { Button } from "../components/button";
import { Card } from "../components/card";
import { ErrorText } from "../components/error-text";
import { errorMessage } from "../lib/api";

export const Route = createFileRoute("/import")({
  beforeLoad: ({ context }) => {
    if (!context.session.user?.is_admin) throw redirect({ to: "/" });
  },
  component: Import,
});

/** What a refused file looks like: the API says what is wrong with it. */
function problemsOf(error: unknown): { problems: ImportProblem[]; total: number } | null {
  if (typeof error !== "object" || error === null) return null;
  if (!("problems" in error) || !Array.isArray(error.problems)) return null;
  const total = "total_problems" in error ? Number(error.total_problems) : error.problems.length;
  return { problems: error.problems, total };
}

function Import() {
  const queryClient = useQueryClient();
  const importHits = useMutation(importHitsMutation());
  const [file, setFile] = useState<File | null>(null);
  const [text, setText] = useState("");
  const [done, setDone] = useState<number>();
  const [failure, setFailure] = useState<unknown>();
  const problems = problemsOf(failure);

  return (
    <div className="space-y-6">
      <h1 className="text-xl font-semibold">Import hits</h1>
      <Card>
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
                // The generated schema takes a binary part for a string.
                requestValidator: async (data) => data,
              });
              setDone(result.imported);
              setFile(null);
              setText("");
              queryClient.invalidateQueries();
            } catch (err) {
              setFailure(err);
            }
          }}
        >
          <p className="text-sm text-muted">
            A CSV in the format of the export:{" "}
            <code>site,created_at,kind,path,name,referrer,country,device,browser,os</code>, with
            that header row. Every site must already exist. Nothing is added if any row has a
            problem. Imported hits count as pageviews but add no unique visitors, and rows are not
            checked for duplicates: importing a file twice adds its hits twice. Up to 256 MB.
          </p>
          <label className="block space-y-1 text-sm">
            <span className="font-medium">CSV file</span>
            <input
              type="file"
              accept=".csv,text/csv"
              className="block text-sm"
              onChange={(e) => setFile(e.target.files?.[0] ?? null)}
            />
          </label>
          <label className="block space-y-1 text-sm">
            <span className="font-medium">or paste CSV content</span>
            <textarea
              value={text}
              onChange={(e) => setText(e.target.value)}
              disabled={file !== null}
              rows={8}
              spellCheck={false}
              className="block w-full rounded border border-line-strong px-3 py-2 font-mono text-xs disabled:opacity-50"
            />
          </label>
          <Button type="submit" disabled={importHits.isPending}>
            {importHits.isPending ? "Importing…" : "Import"}
          </Button>
        </form>
      </Card>
      {done !== undefined && (
        <Card>
          <p className="text-sm">
            Imported {done} {done === 1 ? "hit" : "hits"}.
          </p>
        </Card>
      )}
      {failure !== undefined && (
        <Card title="Nothing was imported">
          <ErrorText>{errorMessage(failure)}</ErrorText>
          {problems && (
            <>
              <ul className="mt-3 divide-y divide-line text-sm">
                {problems.problems.map((p, i) => (
                  <li key={i} className="py-1.5">
                    <span className="text-muted">
                      {p.line > 0 ? `Line ${p.line}` : "File"}
                      {p.column ? `, ${p.column}` : ""}:
                    </span>{" "}
                    {p.message}
                  </li>
                ))}
              </ul>
              {problems.total > problems.problems.length && (
                <p className="mt-2 text-sm text-muted">
                  and {problems.total - problems.problems.length} more
                </p>
              )}
            </>
          )}
        </Card>
      )}
    </div>
  );
}
