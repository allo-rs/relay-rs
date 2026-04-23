import { NavLink, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  ArrowRightLeft,
  Server,
  Settings as SettingsIcon,
  LogOut,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { clearToken } from "@/lib/auth";

type NavItem = {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  end?: boolean;
};

const NAV_ITEMS: NavItem[] = [
  { to: "/",          label: "概览", icon: LayoutDashboard, end: true },
  { to: "/forwards",  label: "转发", icon: ArrowRightLeft },
  { to: "/nodes",     label: "节点", icon: Server },
  { to: "/settings",  label: "设置", icon: SettingsIcon },
];

export default function DynamicIsland() {
  const navigate = useNavigate();

  function handleLogout() {
    clearToken();
    navigate("/login", { replace: true });
  }

  return (
    <header className="fixed top-4 inset-x-0 z-50 flex justify-center pointer-events-none">
      <nav
        className={cn(
          "pointer-events-auto flex items-center gap-1 rounded-full",
          "bg-foreground/90 text-background shadow-lg shadow-black/20",
          "backdrop-blur-md border border-white/10",
          "px-2 py-1.5"
        )}
      >
        {/* 左侧 Logo 圆点 */}
        <div className="flex items-center gap-2 pl-2 pr-3">
          <span className="relative flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-400" />
          </span>
          <span className="text-xs font-medium tracking-wide opacity-80 hidden sm:inline">
            relay-rs
          </span>
        </div>

        <div className="h-4 w-px bg-white/15" />

        {/* 导航项 */}
        <ul className="flex items-center">
          {NAV_ITEMS.map(({ to, label, icon: Icon, end }) => (
            <li key={to}>
              <NavLink
                to={to}
                end={end}
                className={({ isActive }) =>
                  cn(
                    "group flex items-center gap-1.5 rounded-full px-3 py-1.5",
                    "text-xs font-medium transition-all duration-200",
                    isActive
                      ? "bg-background text-foreground shadow-sm"
                      : "text-background/70 hover:text-background hover:bg-white/10"
                  )
                }
              >
                <Icon className="h-3.5 w-3.5" />
                <span>{label}</span>
              </NavLink>
            </li>
          ))}
        </ul>

        <div className="h-4 w-px bg-white/15 mx-1" />

        {/* 退出 */}
        <button
          onClick={handleLogout}
          title="退出登录"
          className={cn(
            "flex items-center justify-center rounded-full h-7 w-7",
            "text-background/70 hover:text-background hover:bg-white/10",
            "transition-colors"
          )}
        >
          <LogOut className="h-3.5 w-3.5" />
        </button>
      </nav>
    </header>
  );
}
