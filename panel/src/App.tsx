import { Routes, Route, Navigate } from "react-router-dom";
import Login from "@/pages/Login";
import Overview from "@/pages/Overview";
import Forwards from "@/pages/Forwards";
import Nodes from "@/pages/Nodes";
import NodeDetail from "@/pages/NodeDetail";
import Settings from "@/pages/Settings";
import PrivateRoute from "@/routes/PrivateRoute";

export default function App() {
  return (
    <Routes>
      {/* 公开路由 */}
      <Route path="/login" element={<Login />} />

      {/* 受保护路由 */}
      <Route element={<PrivateRoute />}>
        <Route path="/"             element={<Overview />} />
        <Route path="/forwards"     element={<Forwards />} />
        <Route path="/nodes"        element={<Nodes />} />
        <Route path="/nodes/:id"    element={<NodeDetail />} />
        <Route path="/settings"     element={<Settings />} />
      </Route>

      {/* 兜底重定向 */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
