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
import { Switch } from "@/components/ui/switch";
import { NativeSelect } from "@/components/ui/select";
import { addBlockRule } from "@/lib/api";
import type { BlockRule } from "@/lib/types";

const schema = z.object({
  src: z.string().optional(),
  dst: z.string().optional(),
  port: z.coerce.number().int().min(1).max(65535).optional().or(z.literal("")),
  proto: z.enum(["all", "tcp", "udp"]),
  chain: z.enum(["input", "forward"]),
  ipv6: z.boolean(),
  comment: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

interface AddBlockDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nodeId: number;
  onSuccess: () => void;
}

export default function AddBlockDialog({
  open,
  onOpenChange,
  nodeId,
  onSuccess,
}: AddBlockDialogProps) {
  const [submitting, setSubmitting] = useState(false);

  const {
    register,
    handleSubmit,
    watch,
    setValue,
    reset,
    formState: { errors },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      proto: "all",
      chain: "input",
      ipv6: false,
    },
  });

  async function onSubmit(values: FormValues) {
    setSubmitting(true);
    try {
      const rule: BlockRule = {
        src: values.src || undefined,
        dst: values.dst || undefined,
        port: values.port || undefined,
        proto: values.proto,
        chain: values.chain,
        ipv6: values.ipv6,
        comment: values.comment || undefined,
      };
      await addBlockRule(nodeId, rule);
      toast.success("防火墙规则添加成功");
      reset();
      onOpenChange(false);
      onSuccess();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "添加失败");
    } finally {
      setSubmitting(false);
    }
  }

  function handleClose() {
    reset();
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[480px]">
        <DialogHeader>
          <DialogTitle>添加防火墙规则</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          {/* 源 IP */}
          <div className="space-y-1.5">
            <Label htmlFor="src">源 IP（可选）</Label>
            <Input id="src" placeholder="192.168.1.0/24" {...register("src")} />
          </div>

          {/* 目标 IP */}
          <div className="space-y-1.5">
            <Label htmlFor="dst">目标 IP（可选）</Label>
            <Input id="dst" placeholder="10.0.0.1" {...register("dst")} />
          </div>

          {/* 端口 */}
          <div className="space-y-1.5">
            <Label htmlFor="port">端口（可选）</Label>
            <Input
              id="port"
              type="number"
              min={1}
              max={65535}
              placeholder="8080"
              {...register("port")}
            />
            {errors.port && (
              <p className="text-xs text-destructive">{errors.port.message}</p>
            )}
          </div>

          {/* 协议 */}
          <div className="space-y-1.5">
            <Label htmlFor="proto">协议</Label>
            <NativeSelect id="proto" {...register("proto")}>
              <option value="all">全部</option>
              <option value="tcp">TCP</option>
              <option value="udp">UDP</option>
            </NativeSelect>
          </div>

          {/* 链 */}
          <div className="space-y-1.5">
            <Label htmlFor="chain">链</Label>
            <NativeSelect id="chain" {...register("chain")}>
              <option value="input">input（入站）</option>
              <option value="forward">forward（转发）</option>
            </NativeSelect>
          </div>

          {/* IPv6 */}
          <div className="flex items-center gap-3">
            <Switch
              id="ipv6"
              checked={watch("ipv6")}
              onCheckedChange={(v) => setValue("ipv6", v)}
            />
            <Label htmlFor="ipv6">启用 IPv6</Label>
          </div>

          {/* 备注 */}
          <div className="space-y-1.5">
            <Label htmlFor="comment">备注（可选）</Label>
            <Input id="comment" placeholder="备注信息" {...register("comment")} />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={handleClose}>
              取消
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "添加中..." : "确认添加"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
