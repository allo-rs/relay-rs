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
import {
  listNodes,
  listSegments,
  type Node as RelayNode,
  type Segment,
} from "@/lib/api";

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
  segByNode,
  loading,
}: {
  nodes?: RelayNode[];
  segByNode: Map<string, number>;
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
            {nodes.map((node) => {
              const online = node.status === "ok";
              const count = segByNode.get(node.id) ?? 0;
              return (
                <li key={node.id}>
                  <Link
                    to={`/segments?node=${encodeURIComponent(node.id)}`}
                    className="flex items-center justify-between rounded-lg px-2 py-2 hover:bg-muted/60 transition-colors group"
                  >
                    <span className="flex items-center gap-2 min-w-0">
                      {online ? (
                        <Wifi className="h-3.5 w-3.5 text-emerald-500 shrink-0" />
                      ) : (
                        <WifiOff className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                      )}
                      <span className="text-sm truncate group-hover:text-foreground">
                        {node.name || node.id}
                      </span>
                    </span>
                    <span className="text-xs text-muted-foreground shrink-0 ml-2 tabular-nums">
                      {online ? (
                        <span className="text-foreground font-medium">{count}</span>
                      ) : (
                        <span className="opacity-50">{count}</span>
                      )}{" "}
                      条 segment
                    </span>
                  </Link>
                </li>
              );
            })}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

// ── Segments Overview ──────────────────────────────────────────────

function segTarget(s: Segment): string {
  if (s.next_kind === "upstream" && s.upstream_host) {
    const p =
      s.upstream_port_end && s.upstream_port_end !== s.upstream_port_start
        ? `${s.upstream_port_start}-${s.upstream_port_end}`
        : `${s.upstream_port_start ?? "?"}`;
    return `${s.upstream_host}:${p}`;
  }
  if (s.next_kind === "node" && s.next_segment_id) {
    return `→ seg ${s.next_segment_id.slice(0, 8)}…`;
  }
  return "?";
}

function SegmentsCard({
  segments,
  nodeNameById,
  loading,
}: {
  segments?: Segment[];
  nodeNameById: Map<string, string>;
  loading: boolean;
}) {
  const visible = segments ?? [];
  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">转发段总览</CardTitle>
          <Link
            to="/segments"
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
            <p>暂无 segment</p>
            <Link to="/segments" className="text-primary hover:underline mt-1 inline-block text-xs">
              新建 segment →
            </Link>
          </div>
        ) : (
          <ul className="divide-y">
            {visible.slice(0, 12).map((s) => (
              <li key={s.id} className="py-2 flex items-start gap-2 min-w-0">
                <code className="font-mono text-xs font-medium shrink-0 pt-0.5">
                  :{s.listen}
                </code>
                <span className="text-muted-foreground text-xs pt-0.5">→</span>
                <code className="font-mono text-xs flex-1 min-w-0 truncate text-muted-foreground">
                  {segTarget(s)}
                </code>
                <Badge variant="outline" className="text-[10px] shrink-0 mt-0.5">
                  {nodeNameById.get(s.node_id) ?? s.node_id.slice(0, 8)}
                </Badge>
              </li>
            ))}
            {visible.length > 12 && (
              <li className="pt-2 text-center">
                <Link to="/segments" className="text-xs text-muted-foreground hover:text-foreground">
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
  const nodesQ = useQuery({
    queryKey: ["nodes"],
    queryFn: listNodes,
    refetchInterval: 30_000,
  });
  const segsQ = useQuery({
    queryKey: ["segments", ""],
    queryFn: () => listSegments(),
    refetchInterval: 30_000,
  });

  const loading = nodesQ.isLoading || segsQ.isLoading;
  const nodes = nodesQ.data ?? [];
  const segments = segsQ.data ?? [];

  const totalNodes = nodes.length;
  const onlineNodes = nodes.filter((n) => n.status === "ok").length;
  const totalSegments = segments.length;

  const segByNode = new Map<string, number>();
  for (const s of segments) {
    segByNode.set(s.node_id, (segByNode.get(s.node_id) ?? 0) + 1);
  }
  const nodeNameById = new Map(nodes.map((n) => [n.id, n.name || n.id]));

  return (
    <PageShell title="概览" subtitle="全局运行状态与关键指标">
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4 mb-6">
        <StatCard
          label="在线节点"
          value={`${onlineNodes} / ${totalNodes}`}
          hint={totalNodes === 0 ? "尚未接入节点" : `共 ${totalNodes} 个节点`}
          icon={Server}
          loading={loading}
        />
        <StatCard
          label="转发段"
          value={String(totalSegments)}
          hint="所有节点合计"
          icon={ArrowRightLeft}
          loading={loading}
        />
        <StatCard
          label="实时流量"
          value="—"
          hint="待接入指标"
          icon={Activity}
        />
        <StatCard
          label="异常节点"
          value={String(totalNodes - onlineNodes)}
          hint="非 ok 状态"
          icon={ShieldAlert}
          loading={loading}
        />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <NodeStatusCard nodes={nodes} segByNode={segByNode} loading={loading} />
        <SegmentsCard segments={segments} nodeNameById={nodeNameById} loading={loading} />
      </div>
    </PageShell>
  );
}
