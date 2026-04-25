import { NavLink, useNavigate } from "react-router-dom";
import {
  LayoutDashboard,
  ArrowRightLeft,
  Server,
  Settings as SettingsIcon,
  LogOut,
  ShieldCheck,
  Sun,
  Moon,
  Zap,
  Network,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { logout as apiLogout } from "@/lib/auth";
import { useCurrentUser } from "@/lib/CurrentUser";
import { useTheme } from "@/lib/useTheme";

type NavItem = {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  end?: boolean;
};

const NAV_ITEMS: NavItem[] = [
  { to: "/",             label: "概览",   icon: LayoutDashboard, end: true },
  { to: "/forwards",     label: "转发",   icon: ArrowRightLeft },
  { to: "/nodes",        label: "节点",   icon: Server },
  { to: "/v1/nodes",     label: "v1 节点", icon: Zap },
  { to: "/v1/segments",  label: "v1 段",  icon: Network },
  { to: "/settings",     label: "设置",   icon: SettingsIcon },
];

const btnCls = cn(
  "flex items-center justify-center rounded-full h-7 w-7",
  "text-neutral-100/70 hover:text-neutral-100 hover:bg-white/10",
  "transition-colors"
);

export default function DynamicIsland() {
  const navigate = useNavigate();
  const { user, configured, clear } = useCurrentUser();
  const { theme, toggle } = useTheme();

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
          "bg-neutral-900/90 text-neutral-100 shadow-lg shadow-black/30",
          "backdrop-blur-md border border-white/10",
          "px-2 py-1.5"
        )}
      >
        {/* 状态绿点 */}
        <div className="flex items-center pl-2 pr-3">
          <span className="relative flex h-2 w-2">
            <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-60" />
            <span className="relative inline-flex h-2 w-2 rounded-full bg-emerald-400" />
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
                      ? "bg-white text-neutral-900 font-semibold shadow-[0_0_12px_rgba(255,255,255,0.2)]"
                      : "text-neutral-400 hover:text-neutral-100 hover:bg-white/10"
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

        {/* 深色模式切换 */}
        <button onClick={toggle} title={theme === "dark" ? "切换亮色" : "切换暗色"} className={btnCls}>
          {theme === "dark"
            ? <Sun className="h-3.5 w-3.5" />
            : <Moon className="h-3.5 w-3.5" />}
        </button>

        {/* 未配置模式提示 */}
        {!configured && (
          <div className="flex items-center gap-1.5 pl-1 pr-2 text-xs text-amber-300">
            <span>开放模式</span>
          </div>
        )}

        {/* 当前用户 */}
        {configured && user && (
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
                <ShieldCheck className="h-3 w-3 text-emerald-400" aria-label="管理员" />
              )}
            </div>
          </div>
        )}

        {/* 退出 */}
        {configured && user && (
          <button onClick={handleLogout} title="退出登录" className={btnCls}>
            <LogOut className="h-3.5 w-3.5" />
          </button>
        )}
      </nav>
    </header>
  );
}
