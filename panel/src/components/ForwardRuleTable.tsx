import { useState } from "react";
import { Trash2, Plus, Pencil } from "lucide-react";
import { toast } from "sonner";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { deleteForwardRule } from "@/lib/api";
import AddForwardDialog from "./AddForwardDialog";
import EditForwardDialog from "./EditForwardDialog";
import type { ForwardRule } from "@/lib/types";

interface ForwardRuleTableProps {
  nodeId: number;
  rules: ForwardRule[];
  onRefresh: () => void;
}

export default function ForwardRuleTable({ nodeId, rules, onRefresh }: ForwardRuleTableProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<{ idx: number; rule: ForwardRule } | null>(null);
  const [deletingIdx, setDeletingIdx] = useState<number | null>(null);

  async function handleDelete(idx: number) {
    setDeletingIdx(idx);
    try {
      await deleteForwardRule(nodeId, idx);
      toast.success("转发规则已删除");
      onRefresh();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "删除失败");
    } finally {
      setDeletingIdx(null);
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button size="sm" className="gap-1" onClick={() => setAddOpen(true)}>
          <Plus className="h-4 w-4" />
          添加规则
        </Button>
      </div>

      {rules.length === 0 ? (
        <div className="text-center py-10 text-muted-foreground text-sm">暂无转发规则</div>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>监听端口</TableHead>
              <TableHead>目标地址</TableHead>
              <TableHead>协议</TableHead>
              <TableHead>IPv6</TableHead>
              <TableHead>负载均衡</TableHead>
              <TableHead>限速</TableHead>
              <TableHead>备注</TableHead>
              <TableHead className="w-24">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rules.map((rule, idx) => (
              <TableRow key={idx}>
                <TableCell className="font-mono font-medium">{rule.listen}</TableCell>
                <TableCell>
                  <div className="flex flex-col gap-0.5">
                    {rule.to.map((t, i) => (
                      <span key={i} className="font-mono text-xs">{t}</span>
                    ))}
                  </div>
                </TableCell>
                <TableCell>
                  <Badge variant="outline" className="uppercase">{rule.proto}</Badge>
                </TableCell>
                <TableCell>
                  <Badge variant={rule.ipv6 ? "default" : "secondary"}>
                    {rule.ipv6 ? "是" : "否"}
                  </Badge>
                </TableCell>
                <TableCell>
                  {rule.balance
                    ? <span className="text-xs">{rule.balance}</span>
                    : <span className="text-muted-foreground text-xs">—</span>}
                </TableCell>
                <TableCell>
                  {rule.rate_limit
                    ? <span className="text-xs">{rule.rate_limit} Mbps</span>
                    : <span className="text-muted-foreground text-xs">—</span>}
                </TableCell>
                <TableCell className="text-muted-foreground text-xs max-w-[120px] truncate">
                  {rule.comment || "—"}
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      onClick={() => setEditTarget({ idx, rule })}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-destructive hover:text-destructive"
                      disabled={deletingIdx === idx}
                      onClick={() => handleDelete(idx)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      <AddForwardDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        nodeId={nodeId}
        onSuccess={onRefresh}
      />

      {editTarget && (
        <EditForwardDialog
          open={editTarget !== null}
          onOpenChange={(open) => { if (!open) setEditTarget(null); }}
          nodeId={nodeId}
          idx={editTarget.idx}
          rule={editTarget.rule}
          onSuccess={() => { setEditTarget(null); onRefresh(); }}
        />
      )}
    </div>
  );
}
