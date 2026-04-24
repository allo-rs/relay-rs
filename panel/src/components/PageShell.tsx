import { cn } from "@/lib/utils";

type Props = {
  title?: string;
  subtitle?: string;
  actions?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
};

/** 统一的页面内容骨架：留白 + 标题区 + 内容（DynamicIsland 由 PrivateRoute 统一渲染） */
export default function PageShell({ title, subtitle, actions, children, className }: Props) {
  return (
    <div className="min-h-screen flex flex-col bg-muted/30">
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

      <footer className="border-t border-border/40 mt-8">
        <div className="container flex items-center justify-between py-4 text-[11px] text-muted-foreground/60">
          <span>
            Powered by{" "}
            <a
              href="https://github.com/allo-rs/relay-rs"
              target="_blank"
              rel="noopener noreferrer"
              className="font-semibold hover:text-foreground transition-colors"
            >
              relay-rs
            </a>
          </span>
          <a
            href="https://github.com/allo-rs/relay-rs"
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1.5 hover:text-foreground transition-colors"
          >
            <GitHubIcon />
            allo-rs/relay-rs
          </a>
        </div>
      </footer>
    </div>
  );
}

function GitHubIcon() {
  return (
    <svg viewBox="0 0 16 16" className="h-3 w-3 fill-current" aria-hidden="true">
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
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
