import { NavLink, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  ArrowRightLeft,
  Server,
  Settings as SettingsIcon,
  LogOut,
  ShieldCheck,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { logout as apiLogout } from "@/lib/auth";
import { useCurrentUser } from "@/lib/CurrentUser";

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
  const { user, clear } = useCurrentUser();

  async function handleLogout() {
    try {
      await apiLogout();
    } catch {
      // 忽略网络错误，前端继续清理
    }
    clear();
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

        {/* 当前用户 */}
        {user && (
          <div className="flex items-center gap-2 pl-1 pr-2">
            {user.avatar ? (
              <img
                src={user.avatar}
                alt={user.username}
                className="h-6 w-6 rounded-full object-cover border border-white/20"
              />
            ) : (
              <div className="h-6 w-6 rounded-full bg-white/15 flex items-center justify-center text-[10px] font-semibold">
                {user.username.slice(0, 2).toUpperCase()}
              </div>
            )}
            <div className="flex items-center gap-1 text-xs">
              <span className="opacity-90">{user.username}</span>
              {user.admin && (
                <ShieldCheck
                  className="h-3 w-3 text-emerald-400"
                  aria-label="管理员"
                />
              )}
            </div>
          </div>
        )}

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
