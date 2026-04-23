import { useParams, useNavigate, Link } from "react-router-dom";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { RefreshCcw, ChevronRight, Server, Loader2, Copy, Check, Terminal } from "lucide-react";
import { toast } from "sonner";
import PageShell from "@/components/PageShell";
import ForwardRuleTable from "@/components/ForwardRuleTable";
import BlockRuleTable from "@/components/BlockRuleTable";
import StatsView from "@/components/StatsView";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { getNodes, getNodeStatus, getNodeRules, reloadNode, getMasterPubkey } from "@/lib/api";

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

  // 独立查询节点在线状态（复用 NodeCard 的 cache key，避免重复请求）
  const { data: statusData } = useQuery({
    queryKey: ["node-status", nodeId],
    queryFn: () => getNodeStatus(nodeId),
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
  const nodeOnline = statusData?.online === true;
  const [copied, setCopied] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);

  // 始终拉取主控公钥，在线/离线都能查看安装命令
  const { data: pubkeyData } = useQuery({
    queryKey: ["master-pubkey"],
    queryFn: getMasterPubkey,
    enabled: nodes !== undefined,
    staleTime: Infinity,
  });

  function buildInstallCmd(): string {
    if (!node || !pubkeyData?.pubkey) return "";
    const url = new URL(node.url);
    const port = url.port || "9090";
    const b64 = btoa(pubkeyData.pubkey);
    return `bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \\\n  --port ${port} --pubkey-b64 ${b64}`;
  }

  function handleCopy() {
    const cmd = buildInstallCmd();
    if (!cmd) return;
    navigator.clipboard.writeText(cmd).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }

  // 获取节点规则（仅节点在线时请求）
  const {
    data: rules,
    isLoading: rulesLoading,
    error: rulesError,
    refetch: refetchRules,
  } = useQuery({
    queryKey: ["rules", nodeId],
    queryFn: () => getNodeRules(nodeId),
    enabled: nodeOnline,
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
    <PageShell>
      {/* 面包屑 */}
      <nav className="flex items-center gap-1.5 text-sm text-muted-foreground mb-4">
        <Link
          to="/nodes"
          className="flex items-center gap-1 hover:text-foreground transition-colors"
        >
          <Server className="h-3.5 w-3.5" />
          节点
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
        <div className="flex gap-2 shrink-0">
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={() => setInstallOpen(true)}
          >
            <Terminal className="h-4 w-4" />
            安装命令
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={() => reloadMutation.mutate()}
            disabled={reloadMutation.isPending || !nodeOnline}
          >
            {reloadMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <RefreshCcw className="h-4 w-4" />
            )}
            重载规则
          </Button>
        </div>
      </div>

      {/* 安装命令 Dialog */}
      <Dialog open={installOpen} onOpenChange={setInstallOpen}>
        <DialogContent className="sm:max-w-[600px]">
          <DialogHeader>
            <DialogTitle>节点安装命令</DialogTitle>
          </DialogHeader>
          <div className="space-y-3 pt-2">
            <p className="text-sm text-muted-foreground">
              在节点服务器上以 <strong>root</strong> 执行以下命令，自动下载并安装 relay-rs：
            </p>
            {pubkeyData?.pubkey ? (
              <>
                <pre className="bg-muted rounded-md p-4 text-xs font-mono leading-relaxed overflow-x-auto whitespace-pre-wrap break-all select-all">
                  {buildInstallCmd()}
                </pre>
                <Button variant="outline" size="sm" className="gap-1.5" onClick={handleCopy}>
                  {copied ? <Check className="h-4 w-4 text-green-500" /> : <Copy className="h-4 w-4" />}
                  {copied ? "已复制" : "复制命令"}
                </Button>
              </>
            ) : (
              <p className="text-xs text-muted-foreground">加载中...</p>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {/* 节点离线提示 */}
      {!nodeOnline && nodes !== undefined && (
        <div className="flex flex-col items-center py-12 gap-2 text-center text-muted-foreground">
          <p className="text-sm">节点当前离线</p>
          <p className="text-xs">点击右上角「安装命令」在节点服务器上重新安装</p>
        </div>
      )}

      {/* 加载中 */}
      {nodeOnline && rulesLoading && (
        <div className="space-y-3">
          <Skeleton className="h-10 w-64" />
          <Skeleton className="h-48 w-full" />
        </div>
      )}

      {/* 错误 */}
      {nodeOnline && rulesError && !rulesLoading && (
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
      {nodeOnline && rules && !rulesLoading && (
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
    </PageShell>
  );
}
