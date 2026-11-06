import { MutationCache, QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { justLostSession } from "./api";

declare module "@tanstack/react-query" {
  interface Register {
    /** `inline: true` marks mutations whose component already shows its own error, so no toast. */
    mutationMeta: {
      inline?: boolean;
    };
  }
}

/** The one query cache of the app. Routes use the SDK's query options and mutations through it. */
export const queryClient: QueryClient = new QueryClient({
  mutationCache: new MutationCache({
    onError: (_error, _variables, _context, mutation) => {
      if (mutation.meta?.inline || justLostSession()) return;
      toast.error("Couldn't save that. Check your connection and try again.");
    },
  }),
  defaultOptions: {
    // A failed request shows at once, and switching back to the tab does not reload every chart.
    queries: { retry: false, refetchOnWindowFocus: false },
  },
});
