import { Radio, MessageSquare } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { redirectToDiscourseLogin } from "@/lib/auth";

export default function Login() {
  function handleLogin() {
    const params = new URLSearchParams(window.location.search);
    const next = params.get("next") ?? "/";
    redirectToDiscourseLogin(next);
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
      </div>
    </div>
  );
}
