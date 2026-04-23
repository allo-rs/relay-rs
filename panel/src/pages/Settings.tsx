import { ShieldCheck, LogOut, Loader2, UserCircle, KeyRound, MessageSquare } from "lucide-react";
import PageShell, { Placeholder } from "@/components/PageShell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { useCurrentUser } from "@/lib/CurrentUser";
import { logout as apiLogout } from "@/lib/auth";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

export default function Settings() {
  const { user, loading, clear } = useCurrentUser();
  const navigate = useNavigate();

  async function handleLogout() {
    try {
      await apiLogout();
    } catch {
      // ignore
    }
    clear();
    toast.success("已退出登录");
    navigate("/login", { replace: true });
  }

  return (
    <PageShell title="设置" subtitle="当前登录身份、主控密钥与节点接入">
      <div className="grid gap-4 lg:grid-cols-2">
        {/* 当前用户 */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm flex items-center gap-2">
              <UserCircle className="h-4 w-4" />
              当前登录
            </CardTitle>
          </CardHeader>
          <CardContent>
            {loading && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                加载中...
              </div>
            )}
            {!loading && !user && (
              <p className="text-sm text-muted-foreground">未登录</p>
            )}
            {user && (
              <div className="flex items-start gap-3">
                {user.avatar ? (
                  <img
                    src={user.avatar}
                    alt={user.username}
                    className="h-14 w-14 rounded-full object-cover border"
                  />
                ) : (
                  <div className="h-14 w-14 rounded-full bg-muted flex items-center justify-center text-lg font-semibold">
                    {user.username.slice(0, 2).toUpperCase()}
                  </div>
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold">{user.username}</span>
                    {user.admin && (
                      <span className="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 px-2 py-0.5 text-xs font-medium">
                        <ShieldCheck className="h-3 w-3" />
                        管理员
                      </span>
                    )}
                  </div>
                  {user.name && (
                    <p className="text-sm text-muted-foreground mt-0.5">{user.name}</p>
                  )}
                  {user.email && (
                    <p className="text-xs text-muted-foreground font-mono mt-1 truncate">
                      {user.email}
                    </p>
                  )}
                  <p className="text-[11px] text-muted-foreground mt-1">
                    Discourse external_id: <span className="font-mono">{user.id}</span>
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-3 gap-1.5"
                    onClick={handleLogout}
                  >
                    <LogOut className="h-3.5 w-3.5" />
                    退出登录
                  </Button>
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        {/* Discourse 连接 */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm flex items-center gap-2">
              <MessageSquare className="h-4 w-4" />
              Discourse 接入
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="Discourse 站点 URL / SSO secret 显示（只读）· 占位" />
          </CardContent>
        </Card>

        {/* 主控身份 */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm flex items-center gap-2">
              <KeyRound className="h-4 w-4" />
              主控 Ed25519 公钥
            </CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="公钥展示 / 复制 / 轮换占位" />
          </CardContent>
        </Card>

        {/* 节点接入 */}
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">节点接入</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="一次性 enroll token / 节点安装命令占位" />
          </CardContent>
        </Card>
      </div>
    </PageShell>
  );
}
