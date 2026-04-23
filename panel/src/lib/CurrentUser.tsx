import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { fetchCurrentUser } from "@/lib/auth";
import type { CurrentUser } from "@/lib/types";

type Ctx = {
  user: CurrentUser | null;
  loading: boolean;
  /** 重新拉取当前登录信息 */
  refresh: () => Promise<void>;
  /** 本地清除（不调用后端 /logout） */
  clear: () => void;
};

const CurrentUserContext = createContext<Ctx>({
  user: null,
  loading: true,
  refresh: async () => {},
  clear: () => {},
});

export function CurrentUserProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<CurrentUser | null>(null);
  const [loading, setLoading] = useState(true);

  async function refresh() {
    setLoading(true);
    try {
      const u = await fetchCurrentUser();
      setUser(u);
    } catch {
      setUser(null);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();

    // 接收全局 401 事件，清空并由 PrivateRoute 跳转
    function onUnauthorized() {
      setUser(null);
    }
    window.addEventListener("app:unauthorized", onUnauthorized);
    return () => window.removeEventListener("app:unauthorized", onUnauthorized);
  }, []);

  return (
    <CurrentUserContext.Provider
      value={{ user, loading, refresh, clear: () => setUser(null) }}
    >
      {children}
    </CurrentUserContext.Provider>
  );
}

export function useCurrentUser() {
  return useContext(CurrentUserContext);
}
