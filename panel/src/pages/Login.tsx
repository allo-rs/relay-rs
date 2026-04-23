import { Radio, MessageSquare, Loader2 } from "lucide-react";
import { Navigate } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { redirectToDiscourseLogin } from "@/lib/auth";
import { useCurrentUser } from "@/lib/CurrentUser";

export default function Login() {
  const { configured, loading } = useCurrentUser();

  function handleLogin() {
    const params = new URLSearchParams(window.location.search);
    const next = params.get("next") ?? "/";
    redirectToDiscourseLogin(next);
  }

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!configured) {
    const params = new URLSearchParams(window.location.search);
    const next = params.get("next") ?? "/";
    return <Navigate to={next} replace />;
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-muted/30 px-4">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex flex-col items-center gap-2 text-center">
          <div className="flex items-center gap-2">
            <Radio className="h-8 w-8 text-primary" />
            <h1 className="text-3xl font-bold tracking-tight">relay-rs</h1>
          </div>
          <p className="text-sm text-muted-foreground">端口转发管理控制面板</p>
        </div>

        <Card>
          <CardHeader className="pb-4">
            <CardTitle className="text-center text-lg">登录</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <Button className="w-full gap-2" onClick={handleLogin}>
              <MessageSquare className="h-4 w-4" />
              使用 Discourse 账号登录
            </Button>
            <p className="text-xs text-muted-foreground text-center">
              将跳转到站点 SSO 页面完成登录
            </p>
          </CardContent>
        </Card>

        <details className="text-xs text-muted-foreground">
          <summary className="cursor-pointer hover:text-foreground transition-colors">
            无法登录？重置 Discourse 配置
          </summary>
          <div className="mt-2 space-y-2 rounded-md border bg-background/50 p-3">
            <p>在服务器执行下列命令清除 Discourse 配置，面板将回到开放模式，可重新在「设置」页配置：</p>
            <pre className="text-[11px] bg-muted px-2 py-1.5 rounded font-mono overflow-x-auto">
              relay-rs panel-reset-auth
            </pre>
          </div>
        </details>
      </div>
    </div>
  );
}
