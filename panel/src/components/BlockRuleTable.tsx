import { useState } from "react";
import { Trash2, Plus } from "lucide-react";
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
import { deleteBlockRule } from "@/lib/api";
import AddBlockDialog from "./AddBlockDialog";
import type { BlockRule } from "@/lib/types";

interface BlockRuleTableProps {
  nodeId: number;
  rules: BlockRule[];
  onRefresh: () => void;
}

export default function BlockRuleTable({
  nodeId,
  rules,
  onRefresh,
}: BlockRuleTableProps) {
  const [dialogOpen, setDialogOpen] = useState(false);
  const [deletingIdx, setDeletingIdx] = useState<number | null>(null);

  async function handleDelete(idx: number) {
    setDeletingIdx(idx);
    try {
      await deleteBlockRule(nodeId, idx);
      toast.success("防火墙规则已删除");
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
        <Button size="sm" className="gap-1" onClick={() => setDialogOpen(true)}>
          <Plus className="h-4 w-4" />
          添加规则
        </Button>
      </div>

      {rules.length === 0 ? (
        <div className="text-center py-10 text-muted-foreground text-sm">
          暂无防火墙规则
        </div>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>源 IP</TableHead>
              <TableHead>目标 IP</TableHead>
              <TableHead>端口</TableHead>
              <TableHead>协议</TableHead>
              <TableHead>链</TableHead>
              <TableHead>IPv6</TableHead>
              <TableHead>备注</TableHead>
              <TableHead className="w-16">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rules.map((rule, idx) => (
              <TableRow key={idx}>
                <TableCell className="font-mono text-xs">
                  {rule.src || <span className="text-muted-foreground">—</span>}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {rule.dst || <span className="text-muted-foreground">—</span>}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {rule.port ?? <span className="text-muted-foreground">—</span>}
                </TableCell>
                <TableCell>
                  <Badge variant="outline" className="uppercase">
                    {rule.proto}
                  </Badge>
                </TableCell>
                <TableCell>
                  <Badge variant="secondary">{rule.chain}</Badge>
                </TableCell>
                <TableCell>
                  <Badge variant={rule.ipv6 ? "default" : "secondary"}>
                    {rule.ipv6 ? "是" : "否"}
                  </Badge>
                </TableCell>
                <TableCell className="text-muted-foreground text-xs max-w-[100px] truncate">
                  {rule.comment || "—"}
                </TableCell>
                <TableCell>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 text-destructive hover:text-destructive"
                    disabled={deletingIdx === idx}
                    onClick={() => handleDelete(idx)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      <AddBlockDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        nodeId={nodeId}
        onSuccess={onRefresh}
      />
    </div>
  );
}
