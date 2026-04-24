import {
  ShieldCheck, LogOut, Loader2, UserCircle, KeyRound,
  MessageSquare, Save, Trash2, Copy, Check, Server,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import PageShell from "@/components/PageShell";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { useCurrentUser } from "@/lib/CurrentUser";
import {
  logout as apiLogout,
  getDiscourseSetting,
  putDiscourseSetting,
  deleteDiscourseSetting,
} from "@/lib/auth";
import { getMasterPubkey } from "@/lib/api";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";

export default function Settings() {
  const { user, configured, loading, refresh, clear } = useCurrentUser();
  const navigate = useNavigate();

  async function handleLogout() {
    try {
      await apiLogout();
    } catch {
      // ignore
    }
    clear();
    toast.success("已退出登录");
    navigate("/login", { replace: true });
  }

  return (
    <PageShell title="设置" subtitle="当前登录身份、主控密钥与节点接入">
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm flex items-center gap-2">
              <UserCircle className="h-4 w-4" />
              当前登录
            </CardTitle>
          </CardHeader>
          <CardContent>
            {loading && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="h-4 w-4 animate-spin" />
                加载中...
              </div>
            )}
            {!loading && !configured && (
              <p className="text-sm text-muted-foreground">
                当前为<strong>开放模式</strong>（未配置 Discourse）。右侧「Discourse 接入」完成配置后将启用登录。
              </p>
            )}
            {!loading && configured && !user && (
              <p className="text-sm text-muted-foreground">未登录</p>
            )}
            {!loading && configured && user && (
              <div className="flex items-start gap-3">
                {user.avatar ? (
                  <img
                    src={user.avatar}
                    alt={user.username}
                    className="h-14 w-14 rounded-full object-cover border"
                  />
                ) : (
                  <div className="h-14 w-14 rounded-full bg-muted flex items-center justify-center text-lg font-semibold">
                    {user.username.slice(0, 2).toUpperCase()}
                  </div>
                )}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-semibold">{user.username}</span>
                    {user.admin && (
                      <span className="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 px-2 py-0.5 text-xs font-medium">
                        <ShieldCheck className="h-3 w-3" />
                        管理员
                      </span>
                    )}
                  </div>
                  {user.name && (
                    <p className="text-sm text-muted-foreground mt-0.5">{user.name}</p>
                  )}
                  {user.email && (
                    <p className="text-xs text-muted-foreground font-mono mt-1 truncate">
                      {user.email}
                    </p>
                  )}
                  <p className="text-[11px] text-muted-foreground mt-1">
                    external_id: <span className="font-mono">{user.id}</span>
                  </p>
                  <Button
                    variant="outline"
                    size="sm"
                    className="mt-3 gap-1.5"
                    onClick={handleLogout}
                  >
                    <LogOut className="h-3.5 w-3.5" />
                    退出登录
                  </Button>
                </div>
              </div>
            )}
          </CardContent>
        </Card>

        <DiscourseSettingCard onChanged={refresh} />

        <MasterPubkeyCard />
        <NodeEnrollCard />
      </div>
    </PageShell>
  );
}

// ── Master Pubkey Card ─────────────────────────────────────────────

function useCopy(timeout = 2000) {
  const [copied, setCopied] = useState(false);
  function copy(text: string) {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), timeout);
    });
  }
  return { copied, copy };
}

function MasterPubkeyCard() {
  const { data, isLoading, error } = useQuery({
    queryKey: ["master-pubkey"],
    queryFn: getMasterPubkey,
    staleTime: Infinity,
  });

  const { copied: copiedPem, copy: copyPem } = useCopy();
  const { copied: copiedB64, copy: copyB64 } = useCopy();

  const pem = data?.pubkey ?? "";
  const b64 = pem ? btoa(pem) : "";

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm flex items-center gap-2">
          <KeyRound className="h-4 w-4" />
          主控 Ed25519 公钥
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {isLoading && (
          <div className="space-y-2">
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-4/5" />
          </div>
        )}
        {error && (
          <p className="text-sm text-destructive">
            加载失败：{error instanceof Error ? error.message : "未知错误"}
          </p>
        )}
        {pem && (
          <>
            <pre className="rounded-md bg-muted px-3 py-2.5 text-[11px] font-mono leading-relaxed overflow-x-auto whitespace-pre-wrap break-all select-all">
              {pem}
            </pre>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => copyPem(pem)}
              >
                {copiedPem ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
                {copiedPem ? "已复制 PEM" : "复制 PEM"}
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5"
                onClick={() => copyB64(b64)}
              >
                {copiedB64 ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
                {copiedB64 ? "已复制 Base64" : "复制 Base64"}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              安装脚本使用 Base64 格式（<code>--pubkey-b64</code>）。
            </p>
          </>
        )}
      </CardContent>
    </Card>
  );
}

// ── Node Enrollment Card ───────────────────────────────────────────

function NodeEnrollCard() {
  const { data, isLoading } = useQuery({
    queryKey: ["master-pubkey"],
    queryFn: getMasterPubkey,
    staleTime: Infinity,
  });

  const b64 = data?.pubkey ? btoa(data.pubkey) : "";
  const { copied, copy } = useCopy();

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm flex items-center gap-2">
          <Server className="h-4 w-4" />
          节点接入
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <ol className="space-y-1.5 text-muted-foreground text-xs leading-relaxed list-decimal list-inside">
          <li>前往「节点」页面，点击「添加节点」录入节点名称和 URL。</li>
          <li>进入节点详情页，点击「安装命令」获取完整安装脚本。</li>
          <li>在节点服务器以 <code>root</code> 执行安装脚本，完成后节点上线。</li>
        </ol>

        <div className="space-y-1.5">
          <p className="text-xs text-muted-foreground">
            公钥 Base64（安装脚本 <code>--pubkey-b64</code> 参数）：
          </p>
          {isLoading ? (
            <Skeleton className="h-8" />
          ) : b64 ? (
            <div className="flex gap-2">
              <code className="flex-1 min-w-0 rounded-md bg-muted px-2 py-1.5 text-[11px] font-mono truncate select-all">
                {b64}
              </code>
              <Button
                variant="outline"
                size="sm"
                className="shrink-0 gap-1.5"
                onClick={() => copy(b64)}
              >
                {copied ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
                {copied ? "已复制" : "复制"}
              </Button>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">加载失败</p>
          )}
        </div>

        <Button asChild size="sm" variant="outline" className="w-full gap-1.5 mt-1">
          <Link to="/nodes">
            <Server className="h-3.5 w-3.5" />
            前往节点管理
          </Link>
        </Button>
      </CardContent>
    </Card>
  );
}

// ── Discourse Setting Card ────────────────────────────────────────

function DiscourseSettingCard({ onChanged }: { onChanged: () => Promise<void> }) {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [url, setUrl] = useState("");
  const [secret, setSecret] = useState("");
  const [hasSecret, setHasSecret] = useState(false);

  async function load() {
    setLoading(true);
    try {
      const s = await getDiscourseSetting();
      setConfigured(s.configured);
      setUrl(s.url);
      setHasSecret(s.hasSecret);
      setSecret("");
    } catch (e) {
      toast.error(`读取失败: ${(e as Error).message}`);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void load();
  }, []);

  async function handleSave() {
    if (!url.trim()) {
      toast.error("请填写 Discourse URL");
      return;
    }
    if (!hasSecret && !secret.trim()) {
      toast.error("首次配置必须填写 secret");
      return;
    }
    setSaving(true);
    try {
      await putDiscourseSetting({
        url: url.trim(),
        secret: secret.trim() || undefined,
      });
      toast.success("已保存，即时生效");
      await load();
      await onChanged();
    } catch (e) {
      toast.error(`保存失败: ${(e as Error).message}`);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete() {
    if (!confirm("确认清除 Discourse 配置？清除后面板将回到「开放访问」模式。")) return;
    setSaving(true);
    try {
      await deleteDiscourseSetting();
      toast.success("已清除");
      await load();
      await onChanged();
    } catch (e) {
      toast.error(`清除失败: ${(e as Error).message}`);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm flex items-center gap-2">
          <MessageSquare className="h-4 w-4" />
          Discourse 接入
          {configured ? (
            <span className="inline-flex items-center rounded-full bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 px-2 py-0.5 text-xs font-medium">
              已启用
            </span>
          ) : (
            <span className="inline-flex items-center rounded-full bg-amber-500/15 text-amber-700 dark:text-amber-400 px-2 py-0.5 text-xs font-medium">
              未配置
            </span>
          )}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        {loading ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            加载中...
          </div>
        ) : (
          <>
            <div className="space-y-1.5">
              <Label htmlFor="discourse-url">Discourse 站点 URL</Label>
              <Input
                id="discourse-url"
                placeholder="https://forum.example.com"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                disabled={saving}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="discourse-secret">
                SSO Secret{" "}
                {hasSecret && (
                  <span className="text-xs text-muted-foreground font-normal">
                    （留空保持不变）
                  </span>
                )}
              </Label>
              <Input
                id="discourse-secret"
                type="password"
                placeholder={hasSecret ? "••••••••（不修改）" : "至少 10 字符"}
                value={secret}
                onChange={(e) => setSecret(e.target.value)}
                disabled={saving}
                autoComplete="new-password"
              />
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              需在 Discourse 后台开启 <code>enable discourse connect provider</code>，并在
              <code> discourse connect provider secrets</code> 中添加：
              <br />
              <code className="text-[11px]">{`${window.location.host}|<同样的 secret>`}</code>
            </p>
            <div className="flex items-center gap-2 pt-1">
              <Button size="sm" onClick={handleSave} disabled={saving} className="gap-1.5">
                {saving ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Save className="h-3.5 w-3.5" />
                )}
                保存
              </Button>
              {configured && (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handleDelete}
                  disabled={saving}
                  className="gap-1.5 text-destructive hover:text-destructive"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  清除配置
                </Button>
              )}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
