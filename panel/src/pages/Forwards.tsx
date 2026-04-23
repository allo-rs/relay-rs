import { Plus, Filter } from "lucide-react";
import PageShell, { Placeholder } from "@/components/PageShell";
import { Button } from "@/components/ui/button";

export default function Forwards() {
  return (
    <PageShell
      title="转发"
      subtitle="跨所有节点的端口转发规则"
      actions={
        <>
          <Button variant="outline" size="sm" className="gap-1.5">
            <Filter className="h-4 w-4" />
            筛选
          </Button>
          <Button size="sm" className="gap-1.5">
            <Plus className="h-4 w-4" />
            新建转发
          </Button>
        </>
      }
    >
      <Placeholder label="转发规则聚合列表（占位）：节点 / 监听 / 目标 / 协议 / 模式 / 限速 / 流量 / 状态" />
    </PageShell>
  );
}
