import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  ArrowRightLeft,
  Server,
  ShieldAlert,
  Wifi,
  WifiOff,
  ArrowRight,
} from "lucide-react";
import PageShell from "@/components/PageShell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { getAllForwards } from "@/lib/api";

// ── Stat Card ──────────────────────────────────────────────────────

type StatProps = {
  label: string;
  value: string;
  hint?: string;
  icon: React.ComponentType<{ className?: string }>;
  loading?: boolean;
};

function StatCard({ label, value, hint, icon: Icon, loading }: StatProps) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-xs font-medium text-muted-foreground">{label}</CardTitle>
        <Icon className="h-4 w-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        {loading ? (
          <Skeleton className="h-8 w-24" />
        ) : (
          <div className="text-2xl font-semibold tabular-nums">{value}</div>
        )}
        {hint && <p className="text-xs text-muted-foreground mt-1">{hint}</p>}
      </CardContent>
    </Card>
  );
}

// ── Node Status Panel ──────────────────────────────────────────────

function NodeStatusCard({
  nodes,
  loading,
}: {
  nodes?: { id: number; name: string; online: boolean; rule_count: number }[];
  loading: boolean;
}) {
  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">节点状态</CardTitle>
          <Link
            to="/nodes"
            className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
          >
            管理节点
            <ArrowRight className="h-3 w-3" />
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="space-y-2">
            {[1, 2, 3].map((i) => <Skeleton key={i} className="h-10 rounded-lg" />)}
          </div>
        ) : !nodes || nodes.length === 0 ? (
          <div className="text-center py-8 text-sm text-muted-foreground">
            <p>暂无节点</p>
            <Link to="/nodes" className="text-primary hover:underline mt-1 inline-block text-xs">
              添加第一个节点 →
            </Link>
          </div>
        ) : (
          <ul className="space-y-1">
            {nodes.map((node) => (
              <li key={node.id}>
                <Link
                  to={`/nodes/${node.id}`}
                  className="flex items-center justify-between rounded-lg px-2 py-2 hover:bg-muted/60 transition-colors group"
                >
                  <span className="flex items-center gap-2 min-w-0">
                    {node.online ? (
                      <Wifi className="h-3.5 w-3.5 text-emerald-500 shrink-0" />
                    ) : (
                      <WifiOff className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                    )}
                    <span className="text-sm truncate group-hover:text-foreground">
                      {node.name}
                    </span>
                  </span>
                  <span className="text-xs text-muted-foreground shrink-0 ml-2 tabular-nums">
                    {node.online ? (
                      <span className="text-foreground font-medium">{node.rule_count}</span>
                    ) : (
                      <span className="opacity-50">{node.rule_count}</span>
                    )}{" "}
                    条规则
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

// ── Forward Rules Overview ─────────────────────────────────────────

interface RuleItem {
  node_id: number;
  node_name: string;
  node_online: boolean;
  idx: number | null;
  rule: { listen: string; to: string[]; proto: string } | null;
}

function ForwardsCard({
  items,
  loading,
}: {
  items?: RuleItem[];
  loading: boolean;
}) {
  const visible = items?.filter((i) => i.rule) ?? [];

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">转发总览</CardTitle>
          <Link
            to="/forwards"
            className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-1 transition-colors"
          >
            查看全部
            <ArrowRight className="h-3 w-3" />
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="space-y-2">
            {[1, 2, 3, 4].map((i) => <Skeleton key={i} className="h-8 rounded" />)}
          </div>
        ) : visible.length === 0 ? (
          <div className="text-center py-8 text-sm text-muted-foreground">
            <p>暂无转发规则</p>
            <Link to="/forwards" className="text-primary hover:underline mt-1 inline-block text-xs">
              新建转发规则 →
            </Link>
          </div>
        ) : (
          <ul className="divide-y">
            {visible.slice(0, 12).map((item) => {
              const r = item.rule!;
              return (
                <li
                  key={`${item.node_id}-${item.idx}`}
                  className="py-2 flex items-start gap-2 min-w-0"
                >
                  <code className="font-mono text-xs font-medium shrink-0 pt-0.5">
                    {r.listen}
                  </code>
                  <span className="text-muted-foreground text-xs pt-0.5">→</span>
                  <div className="flex-1 min-w-0">
                    {r.to.map((t, i) => (
                      <code key={i} className="font-mono text-xs block truncate text-muted-foreground">
                        {t}
                      </code>
                    ))}
                  </div>
                  <Badge
                    variant={item.node_online ? "outline" : "secondary"}
                    className="text-[10px] shrink-0 mt-0.5"
                  >
                    {item.node_name}
                  </Badge>
                </li>
              );
            })}
            {visible.length > 12 && (
              <li className="pt-2 text-center">
                <Link to="/forwards" className="text-xs text-muted-foreground hover:text-foreground">
                  还有 {visible.length - 12} 条 →
                </Link>
              </li>
            )}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

// ── Page ──────────────────────────────────────────────────────────

export default function Overview() {
  const { data, isLoading } = useQuery({
    queryKey: ["forwards-aggregate"],
    queryFn: getAllForwards,
    refetchInterval: 30_000,
  });

  const totalNodes  = data?.nodes.length ?? 0;
  const onlineNodes = data?.nodes.filter((n) => n.online).length ?? 0;
  const totalRules  = data?.nodes.reduce((s, n) => s + n.rule_count, 0) ?? 0;

  return (
    <PageShell title="概览" subtitle="全局运行状态与关键指标">
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4 mb-6">
        <StatCard
          label="在线节点"
          value={`${onlineNodes} / ${totalNodes}`}
          hint={totalNodes === 0 ? "尚未接入节点" : `共 ${totalNodes} 个节点`}
          icon={Server}
          loading={isLoading}
        />
        <StatCard
          label="转发规则"
          value={String(totalRules)}
          hint="所有在线节点合计"
          icon={ArrowRightLeft}
          loading={isLoading}
        />
        <StatCard
          label="实时流量"
          value="—"
          hint="需节点 sysinfo 支持"
          icon={Activity}
        />
        <StatCard
          label="防火墙拒绝"
          value="—"
          hint="需节点 sysinfo 支持"
          icon={ShieldAlert}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <NodeStatusCard nodes={data?.nodes} loading={isLoading} />
        <ForwardsCard items={data?.items} loading={isLoading} />
      </div>
    </PageShell>
  );
}
