import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { Link } from "react-router-dom";
import { RefreshCw, Plus, Trash2, Activity, Clock } from "lucide-react";
import PageShell from "@/components/PageShell";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import ConfirmDialog from "@/components/ConfirmDialog";
import V1EnrollDialog from "@/components/V1EnrollDialog";
import { listV1Nodes, deleteV1Node, type V1Node } from "@/lib/v1api";

function StatusBadge({ status }: { status: string }) {
  const color =
    status === "ok"
      ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
      : status === "degraded"
      ? "bg-amber-500/15 text-amber-700 dark:text-amber-300"
      : status === "offline"
      ? "bg-slate-500/15 text-slate-600 dark:text-slate-300"
      : "bg-rose-500/15 text-rose-700 dark:text-rose-300";
  return <Badge className={`${color} border-0`}>{status}</Badge>;
}

function formatTime(t: string | null): string {
  if (!t) return "-";
  try {
    return new Date(t).toLocaleString();
  } catch {
    return t;
  }
}

export default function V1Nodes() {
  const qc = useQueryClient();
  const [enrollOpen, setEnrollOpen] = useState(false);
  const [delTarget, setDelTarget] = useState<V1Node | null>(null);

  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ["v1", "nodes"],
    queryFn: listV1Nodes,
    refetchInterval: 30_000,
  });

  const delMut = useMutation({
    mutationFn: (id: string) => deleteV1Node(id),
    onSuccess: () => {
      toast.success("节点已删除");
      qc.invalidateQueries({ queryKey: ["v1", "nodes"] });
      qc.invalidateQueries({ queryKey: ["v1", "segments"] });
      setDelTarget(null);
    },
    onError: (e: Error) => toast.error(`删除失败：${e.message}`),
  });

  return (
    <PageShell
      title="节点 (v1)"
      subtitle="mTLS 注册的 relay-node 列表 · 同步状态、hash 一致性"
      actions={
        <>
          <Button
            variant="outline"
            size="sm"
            onClick={() => refetch()}
            disabled={isFetching}
          >
            <RefreshCw
              className={`mr-1.5 h-4 w-4 ${isFetching ? "animate-spin" : ""}`}
            />
            刷新
          </Button>
          <Button size="sm" onClick={() => setEnrollOpen(true)}>
            <Plus className="mr-1.5 h-4 w-4" />
            新增节点
          </Button>
        </>
      }
    >
      {error && (
        <Card className="border-rose-500/40">
          <CardHeader>
            <CardTitle className="text-rose-600">加载失败</CardTitle>
            <CardDescription>{(error as Error).message}</CardDescription>
          </CardHeader>
        </Card>
      )}

      {isLoading && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-40 w-full" />
          ))}
        </div>
      )}

      {!isLoading && data && data.length === 0 && (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            暂无节点。点击右上角「新增节点」生成 enrollment token。
          </CardContent>
        </Card>
      )}

      {!isLoading && data && data.length > 0 && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {data.map((n) => {
            return (
              <Card key={n.id} className="relative">
                <CardHeader>
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <CardTitle className="text-base">
                        {n.name || n.id}
                      </CardTitle>
                      <CardDescription className="font-mono text-[11px]">
                        {n.id}
                      </CardDescription>
                    </div>
                    <StatusBadge status={n.status} />
                  </div>
                </CardHeader>
                <CardContent className="space-y-2 text-sm">
                  <div className="flex items-center gap-2 text-muted-foreground">
                    <Activity className="h-3.5 w-3.5" />
                    <span>
                      revision applied {n.applied_revision} / desired{" "}
                      {n.desired_revision}
                    </span>
                  </div>
                  <div className="flex items-center gap-2 text-muted-foreground">
                    <Clock className="h-3.5 w-3.5" />
                    <span>last_seen {formatTime(n.last_seen)}</span>
                  </div>
                  <div className="flex items-center justify-between pt-2">
                    <Link
                      to={`/v1/segments?node=${encodeURIComponent(n.id)}`}
                      className="text-xs text-primary hover:underline"
                    >
                      查看 segments →
                    </Link>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 text-rose-600 hover:text-rose-700"
                      onClick={() => setDelTarget(n)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}

      <V1EnrollDialog open={enrollOpen} onOpenChange={setEnrollOpen} />

      <ConfirmDialog
        open={!!delTarget}
        onOpenChange={(v) => !v && setDelTarget(null)}
        title={`删除节点 ${delTarget?.name || delTarget?.id}？`}
        description="将同时删除该节点的所有 segments；已部署的 relay-node 会被断开。"
        confirmLabel="删除"
        destructive
        onConfirm={() => delTarget && delMut.mutate(delTarget.id)}
        loading={delMut.isPending}
      />
    </PageShell>
  );
}
