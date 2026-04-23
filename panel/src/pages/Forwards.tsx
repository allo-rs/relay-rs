import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, Trash2, AlertCircle, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import PageShell from "@/components/PageShell";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { NativeSelect } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/table";
import AddForwardDialog from "@/components/AddForwardDialog";
import { getAllForwards, deleteForwardRule } from "@/lib/api";
import type { AggregatedForward } from "@/lib/api";

export default function Forwards() {
  const qc = useQueryClient();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [keyword, setKeyword] = useState("");
  const [protoFilter, setProtoFilter] = useState<"all" | "tcp" | "udp" | "">("");
  const [nodeFilter, setNodeFilter] = useState<string>("");
  const [deletingKey, setDeletingKey] = useState<string | null>(null);

  const { data, isLoading, isError, refetch, isFetching } = useQuery({
    queryKey: ["forwards-aggregate"],
    queryFn: getAllForwards,
    refetchInterval: 30_000,
  });

  const items = data?.items ?? [];
  const nodes = data?.nodes ?? [];

  const nodeOptions = useMemo(
    () => nodes.map((n) => ({ id: n.id, name: n.name, online: n.online })),
    [nodes]
  );

  const offlineNodes = useMemo(
    () => nodes.filter((n) => !n.online),
    [nodes]
  );

  const filtered = useMemo(() => {
    return items.filter((it) => {
      if (!it.rule) return false;
      if (nodeFilter && String(it.node_id) !== nodeFilter) return false;
      if (protoFilter && it.rule.proto !== protoFilter) return false;
      if (keyword) {
        const k = keyword.toLowerCase();
        const hay = [
          it.node_name,
          it.rule.listen,
          ...it.rule.to,
          it.rule.comment ?? "",
        ]
          .join(" ")
          .toLowerCase();
        if (!hay.includes(k)) return false;
      }
      return true;
    });
  }, [items, nodeFilter, protoFilter, keyword]);

  async function handleDelete(item: AggregatedForward) {
    if (item.idx === null || !item.rule) return;
    const key = `${item.node_id}-${item.idx}`;
    setDeletingKey(key);
    try {
      await deleteForwardRule(item.node_id, item.idx);
      toast.success("转发规则已删除");
      qc.invalidateQueries({ queryKey: ["forwards-aggregate"] });
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "删除失败");
    } finally {
      setDeletingKey(null);
    }
  }

  const totalRules = items.filter((it) => it.rule).length;

  return (
    <PageShell
      title="转发"
      subtitle={`跨所有节点的端口转发规则（共 ${totalRules} 条）`}
      actions={
        <>
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={() => refetch()}
            disabled={isFetching}
          >
            <RefreshCw className={`h-4 w-4 ${isFetching ? "animate-spin" : ""}`} />
            刷新
          </Button>
          <Button
            size="sm"
            className="gap-1.5"
            onClick={() => setDialogOpen(true)}
            disabled={nodeOptions.filter((n) => n.online).length === 0}
          >
            <Plus className="h-4 w-4" />
            新建转发
          </Button>
        </>
      }
    >
      {/* 筛选条 */}
      <div className="flex flex-wrap gap-2 mb-4">
        <Input
          placeholder="搜索节点 / 端口 / 目标 / 备注"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
          className="max-w-xs"
        />
        <NativeSelect
          value={nodeFilter}
          onChange={(e) => setNodeFilter(e.target.value)}
          className="max-w-[180px]"
        >
          <option value="">全部节点</option>
          {nodeOptions.map((n) => (
            <option key={n.id} value={n.id}>
              {n.name}
            </option>
          ))}
        </NativeSelect>
        <NativeSelect
          value={protoFilter}
          onChange={(e) => setProtoFilter(e.target.value as typeof protoFilter)}
          className="max-w-[140px]"
        >
          <option value="">全部协议</option>
          <option value="all">all</option>
          <option value="tcp">tcp</option>
          <option value="udp">udp</option>
        </NativeSelect>
      </div>

      {/* 离线节点提示 */}
      {offlineNodes.length > 0 && (
        <div className="mb-4 flex items-start gap-2 rounded-md border border-amber-300/60 bg-amber-50 dark:bg-amber-950/30 px-3 py-2 text-sm">
          <AlertCircle className="h-4 w-4 mt-0.5 text-amber-600 shrink-0" />
          <div>
            <span className="font-medium text-amber-800 dark:text-amber-200">
              {offlineNodes.length} 个节点离线，规则无法显示：
            </span>
            <span className="ml-1 text-amber-700 dark:text-amber-300">
              {offlineNodes.map((n) => n.name).join("、")}
            </span>
          </div>
        </div>
      )}

      {/* 表格 */}
      {isLoading ? (
        <div className="space-y-2">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : isError ? (
        <div className="text-center py-10 text-sm text-destructive">
          加载失败，请稍后重试
        </div>
      ) : filtered.length === 0 ? (
        <div className="text-center py-10 text-sm text-muted-foreground">
          {totalRules === 0 ? "暂无转发规则" : "无匹配项"}
        </div>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>节点</TableHead>
              <TableHead>监听</TableHead>
              <TableHead>目标</TableHead>
              <TableHead>协议</TableHead>
              <TableHead>IPv6</TableHead>
              <TableHead>负载均衡</TableHead>
              <TableHead>限速</TableHead>
              <TableHead>备注</TableHead>
              <TableHead className="w-16">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.map((it) => {
              const r = it.rule!;
              const key = `${it.node_id}-${it.idx}`;
              return (
                <TableRow key={key}>
                  <TableCell>
                    <Link
                      to={`/nodes/${it.node_id}`}
                      className="text-primary hover:underline"
                    >
                      {it.node_name}
                    </Link>
                  </TableCell>
                  <TableCell className="font-mono font-medium">{r.listen}</TableCell>
                  <TableCell>
                    <div className="flex flex-col gap-0.5">
                      {r.to.map((t, i) => (
                        <span key={i} className="font-mono text-xs">
                          {t}
                        </span>
                      ))}
                    </div>
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="uppercase">
                      {r.proto}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    <Badge variant={r.ipv6 ? "default" : "secondary"}>
                      {r.ipv6 ? "是" : "否"}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    {r.balance ? (
                      <span className="text-xs">{r.balance}</span>
                    ) : (
                      <span className="text-muted-foreground text-xs">—</span>
                    )}
                  </TableCell>
                  <TableCell>
                    {r.rate_limit ? (
                      <span className="text-xs">{r.rate_limit} Mbps</span>
                    ) : (
                      <span className="text-muted-foreground text-xs">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-muted-foreground text-xs max-w-[160px] truncate">
                    {r.comment || "—"}
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-destructive hover:text-destructive"
                      disabled={deletingKey === key}
                      onClick={() => handleDelete(it)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}

      <AddForwardDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        onSuccess={() =>
          qc.invalidateQueries({ queryKey: ["forwards-aggregate"] })
        }
      />
    </PageShell>
  );
}
