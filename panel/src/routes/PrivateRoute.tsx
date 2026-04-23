import { Navigate, Outlet } from "react-router-dom";
import { isAuthenticated } from "@/lib/auth";

// 需要登录的路由守卫，未登录则跳转 /login
export default function PrivateRoute() {
  if (!isAuthenticated()) {
    return <Navigate to="/login" replace />;
  }
  return <Outlet />;
}
