import { useState } from "react";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, CheckCircle2 } from "lucide-react";
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
import { createEnrollmentToken, type EnrollmentResp } from "@/lib/api";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export default function EnrollDialog({ open, onOpenChange }: Props) {
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [resp, setResp] = useState<EnrollmentResp | null>(null);
  const [copied, setCopied] = useState(false);

  const mut = useMutation({
    mutationFn: () => createEnrollmentToken(name || `node-${Date.now()}`),
    onSuccess: (data) => {
      setResp(data);
      qc.invalidateQueries({ queryKey: ["nodes"] });
    },
    onError: (e: Error) => toast.error(`生成失败：${e.message}`),
  });

  const reset = () => {
    setName("");
    setResp(null);
    setCopied(false);
  };

  const close = () => {
    reset();
    onOpenChange(false);
  };

  const doCopy = async () => {
    if (!resp) return;
    try {
      await navigator.clipboard.writeText(resp.install_cmd);
      setCopied(true);
      toast.success("已复制");
      setTimeout(() => setCopied(false), 2000);
    } catch {
      toast.error("复制失败，请手动复制");
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(v) => {
        if (!v) reset();
        onOpenChange(v);
      }}
    >
      <DialogContent className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>新增节点</DialogTitle>
          <DialogDescription>
            {resp
              ? "在目标机器上以 root 执行以下命令完成注册。"
              : "生成一次性 enrollment token，复制安装命令到节点机器执行。"}
          </DialogDescription>
        </DialogHeader>

        {!resp && (
          <div className="space-y-3 py-2">
            <div>
              <Label htmlFor="node-name">节点名</Label>
              <Input
                id="node-name"
                placeholder="例如 hk-bgp-01"
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
              />
            </div>
          </div>
        )}

        {resp && (
          <div className="space-y-3 py-2">
            <div className="text-xs text-muted-foreground">
              node_name: <span className="font-mono">{resp.node_name}</span>
            </div>
            <pre className="text-[11px] bg-muted p-3 rounded overflow-x-auto max-h-60 whitespace-pre-wrap break-all">
              {resp.install_cmd}
            </pre>
            <Button variant="outline" size="sm" onClick={doCopy}>
              {copied ? (
                <>
                  <CheckCircle2 className="mr-1.5 h-3.5 w-3.5" /> 已复制
                </>
              ) : (
                <>
                  <Copy className="mr-1.5 h-3.5 w-3.5" /> 复制命令
                </>
              )}
            </Button>
          </div>
        )}

        <DialogFooter>
          {!resp ? (
            <>
              <Button variant="outline" onClick={close}>
                取消
              </Button>
              <Button onClick={() => mut.mutate()} disabled={mut.isPending}>
                {mut.isPending ? "生成中…" : "生成安装命令"}
              </Button>
            </>
          ) : (
            <Button onClick={close}>完成</Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
