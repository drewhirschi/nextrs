"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";

export function Providers({ children }: { children: React.ReactNode }) {
  // staleTime > 0 so the server-hydrated data renders without an immediate
  // refetch on mount — same setting react-todos's entry uses.
  const [client] = useState(
    () =>
      new QueryClient({
        defaultOptions: { queries: { staleTime: 30_000 } },
      }),
  );
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
