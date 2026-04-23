import { Navigate, Outlet, useLocation } from "react-router-dom";
import { Loader2 } from "lucide-react";
import { useCurrentUser } from "@/lib/CurrentUser";

// 需要登录的路由守卫，未登录则跳转 /login
export default function PrivateRoute() {
  const { user, configured, loading } = useCurrentUser();
  const loc = useLocation();

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  // 未配置 Discourse：开放模式，直接放行（后端也不校验）
  if (!configured) {
    return <Outlet />;
  }

  if (!user) {
    const next = encodeURIComponent(loc.pathname + loc.search);
    return <Navigate to={`/login?next=${next}`} replace />;
  }

  return <Outlet />;
}
