// API 请求封装，使用原生 fetch + HttpOnly cookie

import type {
  NodeInfo,
  NodeRules,
  ForwardRule,
  BlockRule,
  OkResponse,
} from "./types";

// 统一请求封装，携带 cookie，401 时抛错由调用方处理
async function apiFetch<T>(url: string, options: RequestInit = {}): Promise<T> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...(options.headers as Record<string, string>),
  };

  const res = await fetch(url, {
    ...options,
    headers,
    credentials: "include",
  });

  if (res.status === 401) {
    // 触发全局未登录处理
    window.dispatchEvent(new CustomEvent("app:unauthorized"));
    throw new Error("未登录或会话已过期");
  }

  if (!res.ok) {
    let message = `请求失败 (${res.status})`;
    try {
      const data = await res.json();
      if (typeof data === "object" && data !== null && "message" in data) {
        message = String((data as { message: unknown }).message);
      } else if (typeof data === "object" && data !== null && "error" in data) {
        message = String((data as { error: unknown }).error);
      }
    } catch {
      // 忽略解析错误
    }
    throw new Error(message);
  }

  return res.json() as Promise<T>;
}

// 登录流程已改为 Discourse Connect（见 lib/auth.ts）——此处不再保留 login API。

interface RawNodeStatus {
  ok?: boolean;
  version?: string;
  mode?: string;
}

interface RawNodeItem {
  id: number;
  name: string;
  url: string;
  status?: RawNodeStatus | null;
}

interface NodeListResponse {
  ok: true;
  nodes: RawNodeItem[];
}

// 获取所有节点列表（仅 DB 数据，状态由 NodeCard 异步获取）
export function getNodes(): Promise<NodeInfo[]> {
  return apiFetch<NodeListResponse>("/api/nodes").then((res) =>
    res.nodes.map((n) => ({
      id: n.id,
      name: n.name,
      url: n.url,
      online: n.status?.ok === true,
      version: n.status?.version,
      mode: n.status?.mode as NodeInfo["mode"] | undefined,
    }))
  );
}

// 单独探活一个节点
export function getNodeStatus(id: number): Promise<{
  online: boolean;
  version?: string;
  mode?: NodeInfo["mode"];
}> {
  return apiFetch<RawNodeStatus>(`/api/nodes/${id}/status`)
    .then((s) => ({
      online: s.ok === true,
      version: s.version,
      mode: s.mode as NodeInfo["mode"] | undefined,
    }))
    .catch(() => ({ online: false }));
}

// 获取指定节点的规则
export function getNodeRules(id: number): Promise<NodeRules> {
  return apiFetch<NodeRules>(`/api/nodes/${id}/rules`);
}

// 全量替换节点规则
export function putNodeRules(id: number, rules: NodeRules): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/rules`, {
    method: "PUT",
    body: JSON.stringify(rules),
  });
}

// 添加转发规则
export function addForwardRule(id: number, rule: ForwardRule): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/rules/forward`, {
    method: "POST",
    body: JSON.stringify(rule),
  });
}

// 删除转发规则
export function deleteForwardRule(id: number, idx: number): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/rules/forward/${idx}`, {
    method: "DELETE",
  });
}

// 添加防火墙规则
export function addBlockRule(id: number, rule: BlockRule): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/rules/block`, {
    method: "POST",
    body: JSON.stringify(rule),
  });
}

// 删除防火墙规则
export function deleteBlockRule(id: number, idx: number): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/rules/block/${idx}`, {
    method: "DELETE",
  });
}

// 获取流量统计
export function getStats(id: number): Promise<unknown> {
  return apiFetch<unknown>(`/api/nodes/${id}/stats`);
}

// 重载节点规则
export function reloadNode(id: number): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}/reload`, {
    method: "POST",
  });
}

export interface AddNodePayload {
  name: string;
  url: string;
}

export interface AddNodeResponse {
  ok: true;
  pubkey: string;
}

export function addNode(payload: AddNodePayload): Promise<AddNodeResponse> {
  return apiFetch<AddNodeResponse>("/api/nodes", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export function deleteNode(id: number): Promise<OkResponse> {
  return apiFetch<OkResponse>(`/api/nodes/${id}`, {
    method: "DELETE",
  });
}

export function getMasterPubkey(): Promise<{ pubkey: string }> {
  return apiFetch<{ pubkey: string }>("/api/pubkey");
}
