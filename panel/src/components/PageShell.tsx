import DynamicIsland from "@/components/DynamicIsland";
import { cn } from "@/lib/utils";

type Props = {
  title?: string;
  subtitle?: string;
  actions?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
};

/** 统一的页面骨架：顶部灵动岛 + 留白 + 标题区 + 内容 */
export default function PageShell({ title, subtitle, actions, children, className }: Props) {
  return (
    <div className="min-h-screen flex flex-col bg-muted/30">
      <DynamicIsland />
      {/* 给灵动岛留出空间 */}
      <main className={cn("flex-1 container pt-24 pb-12", className)}>
        {(title || actions) && (
          <div className="flex items-start justify-between gap-4 mb-6">
            <div>
              {title && (
                <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
              )}
              {subtitle && (
                <p className="text-sm text-muted-foreground mt-1">{subtitle}</p>
              )}
            </div>
            {actions && <div className="flex items-center gap-2">{actions}</div>}
          </div>
        )}
        {children}
      </main>
    </div>
  );
}

/** 占位块：用于未实现的内容区域 */
export function Placeholder({ label = "即将推出" }: { label?: string }) {
  return (
    <div className="rounded-xl border border-dashed border-muted-foreground/30 bg-background/50 p-12 text-center">
      <p className="text-sm text-muted-foreground">{label}</p>
    </div>
  );
}
