import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { fetchAuthState } from "@/lib/auth";
import type { CurrentUser } from "@/lib/types";

type Ctx = {
  user: CurrentUser | null;
  /** 系统是否已配置 Discourse；未配置时 panel 开放访问 */
  configured: boolean;
  loading: boolean;
  refresh: () => Promise<void>;
  clear: () => void;
};

const CurrentUserContext = createContext<Ctx>({
  user: null,
  configured: true,
  loading: true,
  refresh: async () => {},
  clear: () => {},
});

export function CurrentUserProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<CurrentUser | null>(null);
  const [configured, setConfigured] = useState(true);
  const [loading, setLoading] = useState(true);

  async function refresh() {
    setLoading(true);
    try {
      const s = await fetchAuthState();
      setUser(s.user);
      setConfigured(s.configured);
    } catch {
      setUser(null);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();

    function onUnauthorized() {
      setUser(null);
    }
    window.addEventListener("app:unauthorized", onUnauthorized);
    return () => window.removeEventListener("app:unauthorized", onUnauthorized);
  }, []);

  return (
    <CurrentUserContext.Provider
      value={{ user, configured, loading, refresh, clear: () => setUser(null) }}
    >
      {children}
    </CurrentUserContext.Provider>
  );
}

export function useCurrentUser() {
  return useContext(CurrentUserContext);
}
