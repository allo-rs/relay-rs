import PageShell, { Placeholder } from "@/components/PageShell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export default function Settings() {
  return (
    <PageShell title="设置" subtitle="账号、密钥与主控信息">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">账号</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="修改密码、API Token 占位" />
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm">主控身份</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="Ed25519 公钥展示 / 轮换占位" />
          </CardContent>
        </Card>

        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle className="text-sm">节点注册</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="一次性 enroll token 生成 / 节点安装命令占位" />
          </CardContent>
        </Card>
      </div>
    </PageShell>
  );
}
