import { useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { RefreshCw, Plus, Trash2 } from "lucide-react";
import PageShell from "@/components/PageShell";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import ConfirmDialog from "@/components/ConfirmDialog";
import AddSegmentDialog from "@/components/AddSegmentDialog";
import {
  listSegments,
  deleteSegment,
  listNodes,
  type Segment,
} from "@/lib/api";

function nextTarget(s: Segment): string {
  if (s.next_kind === "upstream" && s.upstream_host) {
    const p =
      s.upstream_port_end && s.upstream_port_end !== s.upstream_port_start
        ? `${s.upstream_port_start}-${s.upstream_port_end}`
        : `${s.upstream_port_start ?? "?"}`;
    return `${s.upstream_host}:${p}`;
  }
  if (s.next_kind === "node" && s.next_segment_id) {
    return `→ seg ${s.next_segment_id}`;
  }
  return "?";
}

export default function Segments() {
  const qc = useQueryClient();
  const [params, setParams] = useSearchParams();
  const nodeFilter = params.get("node") || "";
  const [addOpen, setAddOpen] = useState(false);
  const [delTarget, setDelTarget] = useState<Segment | null>(null);

  const nodesQ = useQuery({ queryKey: ["nodes"], queryFn: listNodes });

  const { data, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ["segments", nodeFilter],
    queryFn: () => listSegments(nodeFilter || undefined),
    refetchInterval: 30_000,
  });

  const delMut = useMutation({
    mutationFn: (id: string) => deleteSegment(id),
    onSuccess: () => {
      toast.success("已删除");
      qc.invalidateQueries({ queryKey: ["segments"] });
      qc.invalidateQueries({ queryKey: ["nodes"] });
      setDelTarget(null);
    },
    onError: (e: Error) => toast.error(`删除失败：${e.message}`),
  });

  return (
    <PageShell
      title="转发段"
      subtitle="单口 TCP / UDP 转发规则 · 修改后 master 通过 LISTEN/NOTIFY 秒级推给 node"
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
          <Button size="sm" onClick={() => setAddOpen(true)}>
            <Plus className="mr-1.5 h-4 w-4" />
            新增 segment
          </Button>
        </>
      }
    >
      <div className="mb-4 flex items-center gap-2 text-sm">
        <span className="text-muted-foreground">筛选节点：</span>
        <select
          value={nodeFilter}
          onChange={(e) => {
            const v = e.target.value;
            if (v) setParams({ node: v });
            else setParams({});
          }}
          className="bg-background border border-border rounded px-2 py-1 text-xs"
        >
          <option value="">全部</option>
          {(nodesQ.data || []).map((n) => (
            <option key={n.id} value={n.id}>
              {n.name || n.id}
            </option>
          ))}
        </select>
      </div>

      {error && (
        <Card className="border-rose-500/40">
          <CardHeader>
            <CardTitle className="text-rose-600">加载失败</CardTitle>
            <CardDescription>{(error as Error).message}</CardDescription>
          </CardHeader>
        </Card>
      )}

      {isLoading && <Skeleton className="h-64 w-full" />}

      {!isLoading && data && data.length === 0 && (
        <Card>
          <CardContent className="py-10 text-center text-sm text-muted-foreground">
            {nodeFilter
              ? "该节点暂无 segment，点击「新增 segment」创建"
              : "暂无 segment"}
          </CardContent>
        </Card>
      )}

      {!isLoading && data && data.length > 0 && (
        <Card>
          <CardContent className="p-0">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>Chain</TableHead>
                  <TableHead>节点</TableHead>
                  <TableHead>Listen</TableHead>
                  <TableHead>Proto</TableHead>
                  <TableHead>Target</TableHead>
                  <TableHead>备注</TableHead>
                  <TableHead className="w-16"></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {data.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell className="font-mono text-[11px]">
                      {s.id.slice(0, 20)}…
                    </TableCell>
                    <TableCell className="font-mono text-[11px]">
                      {s.chain_id.slice(0, 14)}…
                    </TableCell>
                    <TableCell className="font-mono text-[11px]">
                      {s.node_id.slice(0, 14)}…
                    </TableCell>
                    <TableCell>:{s.listen}</TableCell>
                    <TableCell>
                      <Badge variant="secondary">{s.proto}</Badge>
                    </TableCell>
                    <TableCell className="font-mono text-xs">
                      {nextTarget(s)}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-xs">
                      {s.comment || "-"}
                    </TableCell>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 text-rose-600 hover:text-rose-700"
                        onClick={() => setDelTarget(s)}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      <AddSegmentDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        defaultNode={nodeFilter || undefined}
        nodes={nodesQ.data || []}
      />

      <ConfirmDialog
        open={!!delTarget}
        onOpenChange={(v) => !v && setDelTarget(null)}
        title="删除 segment？"
        description={
          delTarget
            ? `将删除 listen :${delTarget.listen} → ${nextTarget(delTarget)}`
            : ""
        }
        confirmLabel="删除"
        destructive
        onConfirm={() => delTarget && delMut.mutate(delTarget.id)}
        loading={delMut.isPending}
      />
    </PageShell>
  );
}
