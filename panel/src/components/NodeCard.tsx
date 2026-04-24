import { useNavigate } from "react-router-dom";
import { Wifi, WifiOff, Loader2, ArrowRight, Trash2, ArrowRightLeft } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardHeader,
  CardTitle,
  CardDescription,
  CardContent,
  CardFooter,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { getNodeStatus, getAllForwards } from "@/lib/api";
import type { NodeInfo } from "@/lib/types";

interface NodeCardProps {
  node: NodeInfo;
  onDelete?: () => void;
}

export default function NodeCard({ node, onDelete }: NodeCardProps) {
  const navigate = useNavigate();

  const { data: status, isLoading: statusLoading } = useQuery({
    queryKey: ["node-status", node.id],
    queryFn: () => getNodeStatus(node.id),
    refetchInterval: 30_000,
    staleTime: 10_000,
  });

  // 从已缓存的 forwards-aggregate 读取规则条数，不产生新请求
  const { data: aggregate } = useQuery({
    queryKey: ["forwards-aggregate"],
    queryFn: getAllForwards,
    staleTime: 30_000,
    refetchInterval: 30_000,
  });
  const ruleCount = aggregate?.nodes.find((n) => n.id === node.id)?.rule_count;

  const online = status?.online ?? false;
  const version = status?.version;
  const mode = status?.mode;

  return (
    <Card
      className="cursor-pointer hover:shadow-md transition-shadow"
      onClick={() => navigate(`/nodes/${node.id}`)}
    >
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between gap-2">
          <CardTitle className="text-lg truncate">{node.name}</CardTitle>
          {statusLoading ? (
            <Badge variant="outline" className="shrink-0 gap-1">
              <Loader2 className="h-3 w-3 animate-spin" />
              检测中
            </Badge>
          ) : (
            <Badge variant={online ? "success" : "destructive"} className="shrink-0">
              {online ? (
                <><Wifi className="h-3 w-3 mr-1" />在线</>
              ) : (
                <><WifiOff className="h-3 w-3 mr-1" />离线</>
              )}
            </Badge>
          )}
        </div>
        <CardDescription className="truncate text-xs">{node.url}</CardDescription>
      </CardHeader>
      <CardContent className="pb-2">
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm text-muted-foreground">
          {version && (
            <span>版本: <span className="text-foreground font-medium">{version}</span></span>
          )}
          {mode && (
            <span>模式: <span className="text-foreground font-medium">{mode}</span></span>
          )}
          {ruleCount !== undefined && (
            <span className="flex items-center gap-1">
              <ArrowRightLeft className="h-3 w-3" />
              <span className="text-foreground font-medium">{ruleCount}</span> 条规则
            </span>
          )}
        </div>
      </CardContent>
      <CardFooter className="justify-between">
        {onDelete && (
          <Button
            variant="ghost"
            size="sm"
            className="text-destructive hover:text-destructive gap-1"
            onClick={(e) => { e.stopPropagation(); onDelete(); }}
          >
            <Trash2 className="h-4 w-4" />
            删除
          </Button>
        )}
        <Button variant="ghost" size="sm" className="ml-auto gap-1">
          管理
          <ArrowRight className="h-4 w-4" />
        </Button>
      </CardFooter>
    </Card>
  );
}
