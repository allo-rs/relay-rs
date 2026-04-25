import { useState, useEffect } from "react";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { createSegment, type Node } from "@/lib/api";

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  defaultNode?: string;
  nodes: Node[];
}

export default function AddSegmentDialog({
  open,
  onOpenChange,
  defaultNode,
  nodes,
}: Props) {
  const qc = useQueryClient();
  const [node, setNode] = useState(defaultNode || "");
  const [listen, setListen] = useState("");
  const [upstream, setUpstream] = useState("");
  const [comment, setComment] = useState("");

  useEffect(() => {
    if (open) {
      setNode(defaultNode || nodes[0]?.id || "");
      setListen("");
      setUpstream("");
      setComment("");
    }
  }, [open, defaultNode, nodes]);

  const mut = useMutation({
    mutationFn: () =>
      createSegment({
        node,
        listen,
        upstream,
        comment: comment || undefined,
      }),
    onSuccess: () => {
      toast.success("已创建 segment，master 已推送给节点");
      qc.invalidateQueries({ queryKey: ["segments"] });
      onOpenChange(false);
    },
    onError: (e: Error) => toast.error(`创建失败：${e.message}`),
  });

  const valid = node && listen && upstream.includes(":");

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[460px]">
        <DialogHeader>
          <DialogTitle>新增 segment</DialogTitle>
          <DialogDescription>
            单口 TCP → upstream。保存后 master 会立即推 FullSync。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 py-2">
          <div>
            <Label>节点</Label>
            <select
              value={node}
              onChange={(e) => setNode(e.target.value)}
              className="w-full bg-background border border-border rounded h-9 px-2 text-sm"
            >
              {nodes.map((n) => (
                <option key={n.id} value={n.id}>
                  {n.name || n.id}
                </option>
              ))}
            </select>
          </div>
          <div>
            <Label htmlFor="listen">Listen 端口</Label>
            <Input
              id="listen"
              placeholder="例如 8080 或 0.0.0.0:8080"
              value={listen}
              onChange={(e) => setListen(e.target.value)}
            />
          </div>
          <div>
            <Label htmlFor="upstream">Upstream (host:port)</Label>
            <Input
              id="upstream"
              placeholder="例如 192.0.2.10:443"
              value={upstream}
              onChange={(e) => setUpstream(e.target.value)}
            />
          </div>
          <div>
            <Label htmlFor="comment">备注（可选）</Label>
            <Input
              id="comment"
              placeholder="用途说明"
              value={comment}
              onChange={(e) => setComment(e.target.value)}
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            onClick={() => mut.mutate()}
            disabled={!valid || mut.isPending}
          >
            {mut.isPending ? "创建中…" : "创建"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
