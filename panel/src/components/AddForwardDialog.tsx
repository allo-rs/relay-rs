import { useState } from "react";
import { useForm, useFieldArray } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { Plus, Trash2 } from "lucide-react";
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
import { addForwardRule } from "@/lib/api";
import type { ForwardRule } from "@/lib/types";

const schema = z.object({
  listen: z.string().min(1, "监听端口不能为空"),
  to: z
    .array(z.object({ value: z.string().min(1, "目标地址不能为空") }))
    .min(1, "至少需要一个目标地址"),
  proto: z.enum(["all", "tcp", "udp"]),
  ipv6: z.boolean(),
  balance: z.enum(["round-robin", "random"]).optional(),
  rate_limit: z.coerce.number().positive().optional().or(z.literal("")),
  comment: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

interface AddForwardDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nodeId: number;
  onSuccess: () => void;
}

export default function AddForwardDialog({
  open,
  onOpenChange,
  nodeId,
  onSuccess,
}: AddForwardDialogProps) {
  const [submitting, setSubmitting] = useState(false);

  const {
    register,
    handleSubmit,
    control,
    watch,
    reset,
    setValue,
    formState: { errors },
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      listen: "",
      to: [{ value: "" }],
      proto: "all",
      ipv6: false,
      comment: "",
    },
  });

  const { fields, append, remove } = useFieldArray({ control, name: "to" });
  const toValues = watch("to");
  const showBalance = toValues && toValues.length > 1;

  async function onSubmit(values: FormValues) {
    setSubmitting(true);
    try {
      const rule: ForwardRule = {
        listen: values.listen,
        to: values.to.map((t) => t.value),
        proto: values.proto,
        ipv6: values.ipv6,
        comment: values.comment || undefined,
        balance: showBalance ? values.balance : undefined,
        rate_limit: values.rate_limit || undefined,
      };
      await addForwardRule(nodeId, rule);
      toast.success("转发规则添加成功");
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
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle>添加转发规则</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          {/* 监听端口 */}
          <div className="space-y-1.5">
            <Label htmlFor="listen">监听端口 *</Label>
            <Input
              id="listen"
              placeholder="10000 或 10000-10100"
              {...register("listen")}
            />
            {errors.listen && (
              <p className="text-xs text-destructive">{errors.listen.message}</p>
            )}
          </div>

          {/* 目标地址（多个） */}
          <div className="space-y-1.5">
            <Label>目标地址 *</Label>
            <div className="space-y-2">
              {fields.map((field, idx) => (
                <div key={field.id} className="flex gap-2">
                  <Input
                    placeholder="host:port"
                    {...register(`to.${idx}.value`)}
                  />
                  {fields.length > 1 && (
                    <Button
                      type="button"
                      variant="outline"
                      size="icon"
                      onClick={() => remove(idx)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              ))}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => append({ value: "" })}
                className="gap-1"
              >
                <Plus className="h-4 w-4" />
                添加目标
              </Button>
            </div>
            {errors.to && (
              <p className="text-xs text-destructive">
                {errors.to.message ?? errors.to[0]?.value?.message}
              </p>
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

          {/* 负载均衡（仅多目标时显示） */}
          {showBalance && (
            <div className="space-y-1.5">
              <Label htmlFor="balance">负载均衡</Label>
              <NativeSelect id="balance" {...register("balance")}>
                <option value="">不启用</option>
                <option value="round-robin">轮询 (round-robin)</option>
                <option value="random">随机 (random)</option>
              </NativeSelect>
            </div>
          )}

          {/* IPv6 */}
          <div className="flex items-center gap-3">
            <Switch
              id="ipv6"
              checked={watch("ipv6")}
              onCheckedChange={(v) => setValue("ipv6", v)}
            />
            <Label htmlFor="ipv6">启用 IPv6</Label>
          </div>

          {/* 限速 */}
          <div className="space-y-1.5">
            <Label htmlFor="rate_limit">限速 (Mbps，可选)</Label>
            <Input
              id="rate_limit"
              type="number"
              min={1}
              placeholder="不限速"
              {...register("rate_limit")}
            />
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
