import { useParams, useNavigate, Link } from "react-router-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { RefreshCcw, ChevronRight, Home, Loader2 } from "lucide-react";
import { toast } from "sonner";
import NavBar from "@/components/NavBar";
import ForwardRuleTable from "@/components/ForwardRuleTable";
import BlockRuleTable from "@/components/BlockRuleTable";
import StatsView from "@/components/StatsView";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { getNodes, getNodeRules, reloadNode } from "@/lib/api";

export default function NodeDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const nodeId = Number(id);

  // 无效 id 处理
  if (isNaN(nodeId)) {
    navigate("/", { replace: true });
    return null;
  }

  // 获取节点基本信息（从节点列表中匹配）
  const { data: nodes } = useQuery({
    queryKey: ["nodes"],
    queryFn: getNodes,
    staleTime: 30_000,
  });
  const node = nodes?.find((n) => n.id === nodeId);

  // 获取节点规则
  const {
    data: rules,
    isLoading: rulesLoading,
    error: rulesError,
    refetch: refetchRules,
  } = useQuery({
    queryKey: ["rules", nodeId],
    queryFn: () => getNodeRules(nodeId),
  });

  // 重载规则 mutation
  const reloadMutation = useMutation({
    mutationFn: () => reloadNode(nodeId),
    onSuccess: () => {
      toast.success("规则重载成功");
      void queryClient.invalidateQueries({ queryKey: ["rules", nodeId] });
    },
    onError: (err) => {
      toast.error(err instanceof Error ? err.message : "重载失败");
    },
  });

  // 规则变更后刷新
  function handleRulesRefresh() {
    void queryClient.invalidateQueries({ queryKey: ["rules", nodeId] });
  }

  return (
    <div className="min-h-screen flex flex-col">
      <NavBar />

      <main className="flex-1 container py-6">
        {/* 面包屑 */}
        <nav className="flex items-center gap-1.5 text-sm text-muted-foreground mb-4">
          <Link
            to="/"
            className="flex items-center gap-1 hover:text-foreground transition-colors"
          >
            <Home className="h-3.5 w-3.5" />
            面板
          </Link>
          <ChevronRight className="h-3.5 w-3.5" />
          <span className="text-foreground font-medium">
            {node?.name ?? `节点 #${nodeId}`}
          </span>
        </nav>

        {/* 页面标题 + 操作 */}
        <div className="flex items-start justify-between mb-6 gap-4">
          <div>
            <h2 className="text-2xl font-semibold tracking-tight">
              {node?.name ?? `节点 #${nodeId}`}
            </h2>
            {node && (
              <p className="text-sm text-muted-foreground mt-1 font-mono">
                {node.url}
              </p>
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 shrink-0"
            onClick={() => reloadMutation.mutate()}
            disabled={reloadMutation.isPending}
          >
            {reloadMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCcw className="h-4 w-4" />
            )}
            重载规则
          </Button>
        </div>

        {/* 加载中 */}
        {rulesLoading && (
          <div className="space-y-3">
            <Skeleton className="h-10 w-64" />
            <Skeleton className="h-48 w-full" />
          </div>
        )}

        {/* 错误 */}
        {rulesError && !rulesLoading && (
          <div className="flex flex-col items-center py-16 gap-3 text-center">
            <p className="text-sm text-destructive">
              加载规则失败：{rulesError instanceof Error ? rulesError.message : "未知错误"}
            </p>
            <Button variant="outline" size="sm" onClick={() => refetchRules()}>
              重试
            </Button>
          </div>
        )}

        {/* 规则 Tabs */}
        {rules && !rulesLoading && (
          <Tabs defaultValue="forward">
            <TabsList className="mb-4">
              <TabsTrigger value="forward">
                转发规则
                {rules.forward.length > 0 && (
                  <span className="ml-1.5 rounded-full bg-primary/15 px-1.5 py-0.5 text-xs font-semibold text-primary">
                    {rules.forward.length}
                  </span>
                )}
              </TabsTrigger>
              <TabsTrigger value="block">
                防火墙规则
                {rules.block.length > 0 && (
                  <span className="ml-1.5 rounded-full bg-destructive/15 px-1.5 py-0.5 text-xs font-semibold text-destructive">
                    {rules.block.length}
                  </span>
                )}
              </TabsTrigger>
              <TabsTrigger value="stats">流量统计</TabsTrigger>
            </TabsList>

            <TabsContent value="forward">
              <ForwardRuleTable
                nodeId={nodeId}
                rules={rules.forward}
                onRefresh={handleRulesRefresh}
              />
            </TabsContent>

            <TabsContent value="block">
              <BlockRuleTable
                nodeId={nodeId}
                rules={rules.block}
                onRefresh={handleRulesRefresh}
              />
            </TabsContent>

            <TabsContent value="stats">
              <StatsView nodeId={nodeId} />
            </TabsContent>
          </Tabs>
        )}
      </main>
    </div>
  );
}
