import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { addNode } from "@/lib/api";

const schema = z.object({
  name: z.string().min(1, "请输入节点名称"),
  host: z.string().min(1, "请输入节点 IP 或域名"),
  port: z.coerce.number().int().min(1).max(65535).default(9090),
});

type FormValues = z.infer<typeof schema>;

function buildInstallCmd(port: number, pubkey: string): string {
  const pubkeyB64 = pubkey ? btoa(pubkey) : "";
  return `curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh \\\n  | bash -s -- --port ${port} --pubkey-b64 ${pubkeyB64}`;
}

interface AddNodeDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess: () => void;
}

export default function AddNodeDialog({ open, onOpenChange, onSuccess }: AddNodeDialogProps) {
  const [submitting, setSubmitting] = useState(false);
  const [installCmd, setInstallCmd] = useState<string | null>(null);

  const { register, handleSubmit, reset, formState: { errors } } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: { port: 9090 },
  });

  async function onSubmit(values: FormValues) {
    setSubmitting(true);
    const url = `http://${values.host}:${values.port}`;
    try {
      const res = await addNode({ name: values.name, url });
      setInstallCmd(buildInstallCmd(values.port, res.pubkey ?? ""));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "添加失败");
    } finally {
      setSubmitting(false);
    }
  }

  function handleClose() {
    reset();
    setInstallCmd(null);
    onOpenChange(false);
    onSuccess();
  }

  function handleCopy() {
    if (!installCmd) return;
    navigator.clipboard.writeText(installCmd).then(() => toast.success("已复制到剪贴板"));
  }

  // ── 第二步：展示一键安装命令 ─────────────────────────────────────
  if (installCmd) {
    return (
      <Dialog open={open} onOpenChange={handleClose}>
        <DialogContent className="sm:max-w-[600px]">
          <DialogHeader>
            <DialogTitle>节点添加成功</DialogTitle>
          </DialogHeader>

          <div className="space-y-3 pt-2">
            <p className="text-sm text-muted-foreground">
              在节点服务器上以 <strong>root</strong> 执行以下命令，自动下载并安装 relay-rs：
            </p>
            <pre className="bg-muted rounded-md p-4 text-xs font-mono leading-relaxed select-all overflow-x-auto whitespace-pre-wrap break-all">
              {installCmd}
            </pre>
          </div>

          <DialogFooter className="gap-2 pt-2">
            <Button variant="outline" onClick={handleCopy}>复制命令</Button>
            <Button onClick={handleClose}>完成</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  // ── 第一步：填写节点信息 ──────────────────────────────────────────
  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[420px]">
        <DialogHeader>
          <DialogTitle>添加节点</DialogTitle>
        </DialogHeader>

        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4 pt-2">
          <div className="space-y-1.5">
            <Label htmlFor="name">节点名称</Label>
            <Input id="name" placeholder="如：香港01" {...register("name")} />
            {errors.name && <p className="text-xs text-destructive">{errors.name.message}</p>}
          </div>

          <div className="flex gap-3">
            <div className="flex-1 space-y-1.5">
              <Label htmlFor="host">节点 IP / 域名</Label>
              <Input id="host" placeholder="1.2.3.4" {...register("host")} />
              {errors.host && <p className="text-xs text-destructive">{errors.host.message}</p>}
            </div>
            <div className="w-24 space-y-1.5">
              <Label htmlFor="port">端口</Label>
              <Input id="port" type="number" placeholder="9090" {...register("port")} />
              {errors.port && <p className="text-xs text-destructive">{errors.port.message}</p>}
            </div>
          </div>

          <p className="text-xs text-muted-foreground">
            添加后会生成节点一键安装脚本，在节点服务器上以 root 运行即可。
          </p>

          <DialogFooter className="pt-2">
            <Button type="button" variant="outline" onClick={handleClose}>取消</Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "添加中..." : "添加节点"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
