import { useEffect } from "react";
import { useForm, useFieldArray } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useState } from "react";
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
import { getNodeRules, putNodeRules } from "@/lib/api";
import type { ForwardRule } from "@/lib/types";

const schema = z.object({
  listen: z.string().min(1, "监听端口不能为空"),
  to: z.array(z.object({ value: z.string().min(1, "目标地址不能为空") })).min(1),
  proto: z.enum(["all", "tcp", "udp"]),
  ipv6: z.boolean(),
  balance: z.enum(["round-robin", "random"]).optional(),
  rate_limit: z.coerce.number().positive().optional().or(z.literal("")),
  comment: z.string().optional(),
});

type FormValues = z.infer<typeof schema>;

interface EditForwardDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  nodeId: number;
  idx: number;
  rule: ForwardRule;
  onSuccess: () => void;
}

export default function EditForwardDialog({
  open,
  onOpenChange,
  nodeId,
  idx,
  rule,
  onSuccess,
}: EditForwardDialogProps) {
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
    defaultValues: ruleToForm(rule),
  });

  // 每次打开时用最新 rule 填充表单
  useEffect(() => {
    if (open) reset(ruleToForm(rule));
  }, [open, rule, reset]);

  const { fields, append, remove } = useFieldArray({ control, name: "to" });
  const toValues = watch("to");
  const showBalance = toValues && toValues.length > 1;

  async function onSubmit(values: FormValues) {
    setSubmitting(true);
    try {
      // 拉取完整规则集，只替换目标索引，保留其余规则
      const current = await getNodeRules(nodeId);
      const updated: ForwardRule = {
        listen: values.listen,
        to: values.to.map((t) => t.value),
        proto: values.proto,
        ipv6: values.ipv6,
        balance: showBalance ? values.balance : undefined,
        rate_limit: values.rate_limit || undefined,
        comment: values.comment || undefined,
      };
      const newForward = [...current.forward];
      newForward[idx] = updated;
      await putNodeRules(nodeId, { forward: newForward, block: current.block });
      toast.success("转发规则已更新");
      onOpenChange(false);
      onSuccess();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "更新失败");
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle>编辑转发规则</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="listen">监听端口 *</Label>
            <Input id="listen" placeholder="10000 或 10000-10100" {...register("listen")} />
            {errors.listen && (
              <p className="text-xs text-destructive">{errors.listen.message}</p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label>目标地址 *</Label>
            <div className="space-y-2">
              {fields.map((field, i) => (
                <div key={field.id} className="flex gap-2">
                  <Input placeholder="host:port" {...register(`to.${i}.value`)} />
                  {fields.length > 1 && (
                    <Button type="button" variant="outline" size="icon" onClick={() => remove(i)}>
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              ))}
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="gap-1"
                onClick={() => append({ value: "" })}
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

          <div className="space-y-1.5">
            <Label htmlFor="proto">协议</Label>
            <NativeSelect id="proto" {...register("proto")}>
              <option value="all">全部</option>
              <option value="tcp">TCP</option>
              <option value="udp">UDP</option>
            </NativeSelect>
          </div>

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

          <div className="flex items-center gap-3">
            <Switch
              id="ipv6"
              checked={watch("ipv6")}
              onCheckedChange={(v) => setValue("ipv6", v)}
            />
            <Label htmlFor="ipv6">启用 IPv6</Label>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="rate_limit">限速（Mbps，可选）</Label>
            <Input
              id="rate_limit"
              type="number"
              min={1}
              placeholder="不限速"
              {...register("rate_limit")}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="comment">备注（可选）</Label>
            <Input id="comment" placeholder="备注信息" {...register("comment")} />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              取消
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "保存中..." : "保存修改"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ruleToForm(rule: ForwardRule): FormValues {
  return {
    listen: rule.listen,
    to: rule.to.map((v) => ({ value: v })),
    proto: rule.proto,
    ipv6: rule.ipv6,
    balance: rule.balance,
    rate_limit: rule.rate_limit ?? "",
    comment: rule.comment ?? "",
  };
}
