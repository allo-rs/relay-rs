import { useNavigate } from "react-router-dom";
import { Wifi, WifiOff, ArrowRight, Trash2 } from "lucide-react";
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
import type { NodeInfo } from "@/lib/types";

interface NodeCardProps {
  node: NodeInfo;
  onDelete?: () => void;
}

export default function NodeCard({ node, onDelete }: NodeCardProps) {
  const navigate = useNavigate();

  return (
    <Card
      className="cursor-pointer hover:shadow-md transition-shadow"
      onClick={() => navigate(`/nodes/${node.id}`)}
    >
      <CardHeader className="pb-2">
        <div className="flex items-start justify-between gap-2">
          <CardTitle className="text-lg truncate">{node.name}</CardTitle>
          <Badge variant={node.online ? "success" : "destructive"} className="shrink-0">
            {node.online ? (
              <><Wifi className="h-3 w-3 mr-1" />在线</>
            ) : (
              <><WifiOff className="h-3 w-3 mr-1" />离线</>
            )}
          </Badge>
        </div>
        <CardDescription className="truncate text-xs">{node.url}</CardDescription>
      </CardHeader>
      <CardContent className="pb-2">
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-sm text-muted-foreground">
          {node.version && (
            <span>版本: <span className="text-foreground font-medium">{node.version}</span></span>
          )}
          {node.mode && (
            <span>模式: <span className="text-foreground font-medium">{node.mode}</span></span>
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
