import { useQuery } from "@tanstack/react-query";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { getStats } from "@/lib/api";

interface StatsViewProps {
  nodeId: number;
}

// 判断是否为对象数组（可以用表格展示）
function isObjectArray(data: unknown): data is Record<string, unknown>[] {
  return (
    Array.isArray(data) &&
    data.length > 0 &&
    typeof data[0] === "object" &&
    data[0] !== null
  );
}

export default function StatsView({ nodeId }: StatsViewProps) {
  const { data, isLoading, error } = useQuery({
    queryKey: ["stats", nodeId],
    queryFn: () => getStats(nodeId),
    refetchInterval: 5000, // 每 5 秒自动刷新
  });

  if (isLoading) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-8 w-full" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-center py-10 text-destructive text-sm">
        加载流量统计失败：{error instanceof Error ? error.message : "未知错误"}
      </div>
    );
  }

  if (data === null || data === undefined) {
    return (
      <div className="text-center py-10 text-muted-foreground text-sm">
        暂无流量统计数据
      </div>
    );
  }

  // 对象数组 → 表格展示
  if (isObjectArray(data)) {
    const keys = Object.keys(data[0] ?? {});
    return (
      <div className="space-y-2">
        <p className="text-xs text-muted-foreground">每 5 秒自动刷新</p>
        <Table>
          <TableHeader>
            <TableRow>
              {keys.map((k) => (
                <TableHead key={k}>{k}</TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {data.map((row, i) => (
              <TableRow key={i}>
                {keys.map((k) => (
                  <TableCell key={k} className="font-mono text-xs">
                    {String(row[k] ?? "—")}
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    );
  }

  // 其他格式 → JSON 展示
  return (
    <div className="space-y-2">
      <p className="text-xs text-muted-foreground">每 5 秒自动刷新</p>
      <pre className="rounded-lg bg-muted p-4 text-xs font-mono overflow-auto max-h-[60vh] whitespace-pre-wrap break-all">
        {JSON.stringify(data, null, 2)}
      </pre>
    </div>
  );
}
