import { Activity, ArrowRightLeft, Server, ShieldAlert } from "lucide-react";
import PageShell, { Placeholder } from "@/components/PageShell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

type StatProps = {
  label: string;
  value: string;
  hint?: string;
  icon: React.ComponentType<{ className?: string }>;
};

function StatCard({ label, value, hint, icon: Icon }: StatProps) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-xs font-medium text-muted-foreground">
          {label}
        </CardTitle>
        <Icon className="h-4 w-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-semibold tabular-nums">{value}</div>
        {hint && <p className="text-xs text-muted-foreground mt-1">{hint}</p>}
      </CardContent>
    </Card>
  );
}

export default function Overview() {
  return (
    <PageShell title="概览" subtitle="全局运行状态与关键指标">
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4 mb-6">
        <StatCard label="在线节点" value="— / —" icon={Server} hint="接入节点总数" />
        <StatCard label="转发规则" value="—"     icon={ArrowRightLeft} hint="所有节点合计" />
        <StatCard label="实时流量" value="— Mbps" icon={Activity} hint="入站 + 出站" />
        <StatCard label="防火墙拒绝" value="—"    icon={ShieldAlert} hint="最近 1 小时" />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">流量趋势</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="时序图占位" />
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Top 转发规则</CardTitle>
          </CardHeader>
          <CardContent>
            <Placeholder label="按流量排名占位" />
          </CardContent>
        </Card>
      </div>
    </PageShell>
  );
}
