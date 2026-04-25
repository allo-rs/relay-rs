import { Routes, Route, Navigate } from "react-router-dom";
import Login from "@/pages/Login";
import Overview from "@/pages/Overview";
import Forwards from "@/pages/Forwards";
import Nodes from "@/pages/Nodes";
import NodeDetail from "@/pages/NodeDetail";
import Settings from "@/pages/Settings";
import V1Nodes from "@/pages/V1Nodes";
import V1Segments from "@/pages/V1Segments";
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
        {/* v1 */}
        <Route path="/v1/nodes"     element={<V1Nodes />} />
        <Route path="/v1/segments"  element={<V1Segments />} />
      </Route>

      {/* 兜底重定向 */}
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
