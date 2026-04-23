import { Navigate, Outlet, useLocation } from "react-router-dom";
import { Loader2 } from "lucide-react";
import { useCurrentUser } from "@/lib/CurrentUser";
import DynamicIsland from "@/components/DynamicIsland";

// 需要登录的路由守卫，未登录则跳转 /login
// DynamicIsland 放在这里，路由切换时不会被卸载重建
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

  if (!configured) {
    return (
      <>
        <DynamicIsland />
        <Outlet />
      </>
    );
  }

  if (!user) {
    const next = encodeURIComponent(loc.pathname + loc.search);
    return <Navigate to={`/login?next=${next}`} replace />;
  }

  return (
    <>
      <DynamicIsland />
      <Outlet />
    </>
  );
}
