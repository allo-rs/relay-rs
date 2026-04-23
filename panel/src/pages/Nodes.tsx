import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { RefreshCw, ServerCrash, Plus } from "lucide-react";
import { toast } from "sonner";
import PageShell from "@/components/PageShell";
import NodeCard from "@/components/NodeCard";
import AddNodeDialog from "@/components/AddNodeDialog";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { getNodes, deleteNode } from "@/lib/api";

export default function Nodes() {
  const queryClient = useQueryClient();
  const [addOpen, setAddOpen] = useState(false);

  const { data: nodes, isLoading, error, refetch, isFetching } = useQuery({
    queryKey: ["nodes"],
    queryFn: getNodes,
    refetchInterval: 30_000,
  });

  const delMutation = useMutation({
    mutationFn: (id: number) => deleteNode(id),
    onSuccess: () => {
      toast.success("节点已删除");
      queryClient.invalidateQueries({ queryKey: ["nodes"] });
    },
    onError: (err) => toast.error(err instanceof Error ? err.message : "删除失败"),
  });

  function handleDelete(id: number, name: string) {
    if (!window.confirm(`确认删除节点「${name}」？`)) return;
    delMutation.mutate(id);
  }

  return (
    <PageShell
      title="节点"
      subtitle="管理所有接入的转发节点"
      actions={
        <>
          <Button
            variant="outline"
            size="sm"
            onClick={() => refetch()}
            disabled={isFetching}
            className="gap-1.5"
          >
            <RefreshCw className={`h-4 w-4 ${isFetching ? "animate-spin" : ""}`} />
            刷新
          </Button>
          <Button size="sm" onClick={() => setAddOpen(true)} className="gap-1.5">
            <Plus className="h-4 w-4" />
            添加节点
          </Button>
        </>
      }
    >
      {isLoading && (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {Array.from({ length: 6 }).map((_, i) => (
            <div key={i} className="rounded-xl border p-6 space-y-3 bg-background">
              <Skeleton className="h-5 w-1/2" />
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-1/3" />
            </div>
          ))}
        </div>
      )}

      {error && !isLoading && (
        <div className="flex flex-col items-center justify-center py-20 gap-3 text-center">
          <ServerCrash className="h-12 w-12 text-muted-foreground" />
          <p className="text-sm text-destructive font-medium">加载节点列表失败</p>
          <p className="text-xs text-muted-foreground max-w-xs">
            {error instanceof Error ? error.message : "未知错误"}
          </p>
          <Button variant="outline" size="sm" onClick={() => refetch()}>重试</Button>
        </div>
      )}

      {!isLoading && !error && nodes?.length === 0 && (
        <div className="flex flex-col items-center justify-center py-20 gap-3 text-center">
          <p className="text-sm text-muted-foreground">暂无节点</p>
          <Button size="sm" onClick={() => setAddOpen(true)} className="gap-1.5">
            <Plus className="h-4 w-4" />
            添加第一个节点
          </Button>
        </div>
      )}

      {!isLoading && nodes && nodes.length > 0 && (
        <>
          <div className="text-xs text-muted-foreground mb-3">
            共 {nodes.length} 个节点
          </div>
          <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {nodes.map((node) => (
              <NodeCard
                key={node.id}
                node={node}
                onDelete={() => handleDelete(node.id, node.name)}
              />
            ))}
          </div>
        </>
      )}

      <AddNodeDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSuccess={() => queryClient.invalidateQueries({ queryKey: ["nodes"] })}
      />
    </PageShell>
  );
}
