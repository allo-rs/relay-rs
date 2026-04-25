import { Routes, Route, Navigate } from "react-router-dom";
import Login from "@/pages/Login";
import Overview from "@/pages/Overview";
import Settings from "@/pages/Settings";
import Nodes from "@/pages/Nodes";
import Segments from "@/pages/Segments";
import PrivateRoute from "@/routes/PrivateRoute";

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />

      <Route element={<PrivateRoute />}>
        <Route path="/"          element={<Overview />} />
        <Route path="/nodes"     element={<Nodes />} />
        <Route path="/segments"  element={<Segments />} />
        <Route path="/settings"  element={<Settings />} />
      </Route>

      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
