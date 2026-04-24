import { useRef, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Cpu,
  HardDrive,
  MemoryStick,
  ArrowDownToLine,
  ArrowUpToLine,
  Plug,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { getStats } from "@/lib/api";

// ── 数据格式定义 ──────────────────────────────────────────────────

/** relay_state 写入格式：key=监听地址，value=规则统计 */
interface RuleEntry {
  total_conns: number;
  bytes_in: number;
  bytes_out: number;
}
type RuleMap = Record<string, RuleEntry>;

/** 系统指标格式（sysinfo sidecar 或更新版本提供） */
interface SysStats {
  cpu: number;
  mem: number;
  disk: number;
  rx_kbps?: number;
  rx_mb?: number;
  tx_kbps?: number;
  tx_mb?: number;
  tcp?: number;
  udp?: number;
}

// ── 格式检测 ──────────────────────────────────────────────────────

function isRuleEntry(v: unknown): v is RuleEntry {
  return typeof v === "object" && v !== null && "bytes_in" in v && "bytes_out" in v;
}

function toRuleMap(data: unknown): RuleMap | null {
  if (typeof data !== "object" || data === null || Array.isArray(data)) return null;
  const obj = data as Record<string, unknown>;
  // 支持嵌套 rules 子字段
  if ("rules" in obj && typeof obj.rules === "object") return toRuleMap(obj.rules);
  // 过滤出符合 RuleEntry 的条目
  const entries = Object.entries(obj).filter(([, v]) => isRuleEntry(v));
  return entries.length > 0 ? (Object.fromEntries(entries) as RuleMap) : null;
}

function pickNum(obj: Record<string, unknown>, ...keys: string[]): number | undefined {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === "number") return v;
  }
  return undefined;
}

function toSysStats(data: unknown): SysStats | null {
  if (typeof data !== "object" || data === null || Array.isArray(data)) return null;
  const obj = data as Record<string, unknown>;

  // 支持嵌套 sys / net 字段
  const sysObj = (typeof obj.sys === "object" && obj.sys !== null)
    ? (obj.sys as Record<string, unknown>)
    : obj;
  const netObj = (typeof obj.net === "object" && obj.net !== null)
    ? (obj.net as Record<string, unknown>)
    : obj;

  const cpu  = pickNum(sysObj, "cpu", "cpu_pct", "cpu_usage", "cpu_percent");
  const mem  = pickNum(sysObj, "mem", "mem_pct", "mem_usage", "memory", "memory_percent", "mem_percent");
  const disk = pickNum(sysObj, "disk", "disk_pct", "disk_usage", "disk_percent");

  if (cpu === undefined && mem === undefined && disk === undefined) return null;

  return {
    cpu:  cpu  ?? 0,
    mem:  mem  ?? 0,
    disk: disk ?? 0,
    rx_kbps: pickNum(netObj, "rx_kbps",  "net_rx_kbps",  "net_in_kbps",  "in_kbps"),
    rx_mb:   pickNum(netObj, "rx_mb",    "net_rx_mb",    "net_in_mb",    "in_mb"),
    tx_kbps: pickNum(netObj, "tx_kbps",  "net_tx_kbps",  "net_out_kbps", "out_kbps"),
    tx_mb:   pickNum(netObj, "tx_mb",    "net_tx_mb",    "net_out_mb",   "out_mb"),
    tcp: pickNum(netObj, "tcp", "tcp_conns", "tcp_connections"),
    udp: pickNum(netObj, "udp", "udp_conns", "udp_connections"),
  };
}

// ── 工具函数 ──────────────────────────────────────────────────────

function fmtBytes(bytes: number): string {
  if (bytes < 1024)             return `${bytes} B`;
  if (bytes < 1024 ** 2)        return `${(bytes / 1024).toFixed(2)} KB`;
  if (bytes < 1024 ** 3)        return `${(bytes / 1024 ** 2).toFixed(2)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
}

function fmtRate(bps: number): string {
  if (bps < 1024)        return `${bps.toFixed(0)} B/s`;
  if (bps < 1024 ** 2)   return `${(bps / 1024).toFixed(2)} KB/s`;
  return `${(bps / 1024 ** 2).toFixed(2)} MB/s`;
}

function gaugeColor(pct: number): string {
  if (pct > 85) return "bg-destructive";
  if (pct > 65) return "bg-amber-500";
  return "bg-emerald-500";
}

// ── 子组件 ────────────────────────────────────────────────────────

function GaugeBar({ pct }: { pct: number }) {
  return (
    <div className="w-full h-1.5 rounded-full bg-muted mt-2 overflow-hidden">
      <div
        className={`h-full rounded-full transition-all duration-500 ${gaugeColor(pct)}`}
        style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
      />
    </div>
  );
}

function MetricCard({
  icon: Icon,
  label,
  value,
  pct,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
  pct: number;
}) {
  return (
    <Card>
      <CardContent className="px-4 pt-4 pb-3">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            <Icon className="h-3.5 w-3.5" />
            {label}
          </span>
          <span className="text-sm font-semibold tabular-nums">{value}</span>
        </div>
        <GaugeBar pct={pct} />
      </CardContent>
    </Card>
  );
}

function TrafficCard({
  icon: Icon,
  label,
  rate,
  total,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  rate: string;
  total?: string;
}) {
  return (
    <Card>
      <CardContent className="px-4 pt-4 pb-3">
        <p className="flex items-center gap-1 text-xs text-muted-foreground mb-1">
          <Icon className="h-3.5 w-3.5" />
          {label}
        </p>
        <p className="text-base font-semibold tabular-nums">{rate}</p>
        {total && <p className="text-xs text-muted-foreground tabular-nums mt-0.5">{total}</p>}
      </CardContent>
    </Card>
  );
}

// ── 主组件 ────────────────────────────────────────────────────────

export default function StatsView({ nodeId }: { nodeId: number }) {
  const prevRef = useRef<{ map: RuleMap; ts: number } | null>(null);

  const { data, isLoading, error } = useQuery({
    queryKey: ["stats", nodeId],
    queryFn: () => getStats(nodeId),
    refetchInterval: 5_000,
  });

  // 基于两次采样差值计算实时速率
  const { ruleMap, rxRate, txRate } = useMemo(() => {
    const map = toRuleMap(data);
    if (!map) return { ruleMap: null, rxRate: 0, txRate: 0 };

    const now = Date.now();
    const prev = prevRef.current;
    let rxRate = 0;
    let txRate = 0;

    if (prev) {
      const elapsed = (now - prev.ts) / 1000;
      if (elapsed > 0) {
        let curIn = 0, curOut = 0, prevIn = 0, prevOut = 0;
        for (const v of Object.values(map)) { curIn += v.bytes_in; curOut += v.bytes_out; }
        for (const v of Object.values(prev.map)) { prevIn += v.bytes_in; prevOut += v.bytes_out; }
        rxRate = Math.max(0, (curIn  - prevIn)  / elapsed);
        txRate = Math.max(0, (curOut - prevOut) / elapsed);
      }
    }

    prevRef.current = { map, ts: now };
    return { ruleMap: map, rxRate, txRate };
  }, [data]);

  const sysStats = useMemo(() => toSysStats(data), [data]);

  const aggregate = useMemo(() => {
    if (!ruleMap) return null;
    let totalConns = 0, totalIn = 0, totalOut = 0;
    for (const v of Object.values(ruleMap)) {
      totalConns += v.total_conns;
      totalIn    += v.bytes_in;
      totalOut   += v.bytes_out;
    }
    return { totalConns, totalIn, totalOut };
  }, [ruleMap]);

  // ── 状态渲染 ──────────────────────────────────────────────────

  if (isLoading) {
    return (
      <div className="space-y-3">
        <div className="grid grid-cols-3 gap-3">
          {[1, 2, 3].map((i) => <Skeleton key={i} className="h-20" />)}
        </div>
        <div className="grid grid-cols-2 gap-3">
          {[1, 2].map((i) => <Skeleton key={i} className="h-20" />)}
        </div>
        <Skeleton className="h-40" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-center py-10 text-destructive text-sm">
        加载失败：{error instanceof Error ? error.message : "未知错误"}
      </div>
    );
  }

  const isEmpty =
    !data ||
    (typeof data === "object" &&
      !Array.isArray(data) &&
      Object.keys(data as object).length === 0);

  if (isEmpty) {
    return (
      <div className="text-center py-10 text-muted-foreground text-sm">暂无流量统计数据</div>
    );
  }

  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground">每 5 秒自动刷新</p>

      {/* 系统资源（sysinfo 格式） */}
      {sysStats && (
        <>
          <div className="grid grid-cols-3 gap-3">
            <MetricCard icon={Cpu}         label="CPU" value={`${sysStats.cpu.toFixed(1)}%`}  pct={sysStats.cpu} />
            <MetricCard icon={MemoryStick} label="内存" value={`${sysStats.mem.toFixed(1)}%`}  pct={sysStats.mem} />
            <MetricCard icon={HardDrive}   label="磁盘" value={`${sysStats.disk.toFixed(1)}%`} pct={sysStats.disk} />
          </div>

          {(sysStats.rx_kbps !== undefined || sysStats.tx_kbps !== undefined) && (
            <div className="grid grid-cols-2 gap-3">
              <TrafficCard
                icon={ArrowDownToLine}
                label="入站"
                rate={sysStats.rx_kbps !== undefined ? `${sysStats.rx_kbps.toFixed(2)} KB/s` : "—"}
                total={sysStats.rx_mb !== undefined ? `${sysStats.rx_mb.toFixed(2)} MB` : undefined}
              />
              <TrafficCard
                icon={ArrowUpToLine}
                label="出站"
                rate={sysStats.tx_kbps !== undefined ? `${sysStats.tx_kbps.toFixed(2)} KB/s` : "—"}
                total={sysStats.tx_mb !== undefined ? `${sysStats.tx_mb.toFixed(2)} MB` : undefined}
              />
            </div>
          )}

          {(sysStats.tcp !== undefined || sysStats.udp !== undefined) && (
            <Card>
              <CardContent className="px-4 py-3 flex items-center gap-6">
                <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <Plug className="h-3.5 w-3.5" />
                  活跃连接
                </span>
                {sysStats.tcp !== undefined && (
                  <span className="text-sm tabular-nums">
                    TCP <strong className="font-semibold">{sysStats.tcp}</strong>
                  </span>
                )}
                {sysStats.udp !== undefined && (
                  <span className="text-sm tabular-nums">
                    UDP <strong className="font-semibold">{sysStats.udp}</strong>
                  </span>
                )}
              </CardContent>
            </Card>
          )}
        </>
      )}

      {/* 转发流量汇总（per-rule 格式） */}
      {aggregate && (
        <div className="grid grid-cols-2 gap-3">
          <TrafficCard
            icon={ArrowDownToLine}
            label="入站速率"
            rate={fmtRate(rxRate)}
            total={fmtBytes(aggregate.totalIn)}
          />
          <TrafficCard
            icon={ArrowUpToLine}
            label="出站速率"
            rate={fmtRate(txRate)}
            total={fmtBytes(aggregate.totalOut)}
          />
        </div>
      )}

      {/* 规则明细表 */}
      {ruleMap && Object.keys(ruleMap).length > 0 && (
        <div>
          <p className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
            规则流量明细
          </p>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>监听</TableHead>
                <TableHead className="text-right">连接数</TableHead>
                <TableHead className="text-right">入站</TableHead>
                <TableHead className="text-right">出站</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {Object.entries(ruleMap)
                .sort(([, a], [, b]) => (b.bytes_in + b.bytes_out) - (a.bytes_in + a.bytes_out))
                .map(([port, s]) => (
                  <TableRow key={port}>
                    <TableCell className="font-mono text-sm">{port}</TableCell>
                    <TableCell className="text-right font-mono text-sm">
                      {s.total_conns.toLocaleString()}
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs text-muted-foreground">
                      {fmtBytes(s.bytes_in)}
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs text-muted-foreground">
                      {fmtBytes(s.bytes_out)}
                    </TableCell>
                  </TableRow>
                ))}
            </TableBody>
          </Table>
        </div>
      )}
    </div>
  );
}
