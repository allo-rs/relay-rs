// Panel admin API。所有路径以 /api/... 为主，鉴权复用 /api/auth/*
// 所有请求依赖 HttpOnly cookie（Discourse SSO 签发的 relay_token）

async function apiFetch<T>(url: string, init: RequestInit = {}): Promise<T> {
  const res = await fetch(url, {
    ...init,
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...(init.headers as Record<string, string>),
    },
  });

  if (res.status === 401) {
    window.dispatchEvent(new CustomEvent("app:unauthorized"));
    throw new Error("未登录或会话已过期");
  }

  if (!res.ok) {
    let msg = `请求失败 (${res.status})`;
    try {
      const data = await res.json();
      if (typeof data === "object" && data !== null) {
        if ("error" in data) msg = String((data as { error: unknown }).error);
        else if ("message" in data) msg = String((data as { message: unknown }).message);
      }
    } catch {
      /* ignore */
    }
    throw new Error(msg);
  }

  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

// ── 类型定义 ─────────────────────────────────────────────
export interface Node {
  id: string;
  name: string;
  status: string;
  session_epoch: number;
  applied_revision: number;
  desired_revision: number;
  last_seen: string | null;
}

export interface Segment {
  id: string;
  chain_id: string;
  node_id: string;
  listen: string;
  proto: string;
  ipv6: boolean;
  next_kind: string; // "upstream" | "node"
  next_segment_id: string | null;
  upstream_host: string | null;
  upstream_port_start: number | null;
  upstream_port_end: number | null;
  comment: string | null;
}

export interface EnrollmentResp {
  token: string;
  node_name: string;
  install_cmd: string;
}

// ── Nodes ────────────────────────────────────────────────
export const listNodes = () => apiFetch<Node[]>("/api/nodes");
export const deleteNode = (id: string) =>
  apiFetch<void>(`/api/nodes/${encodeURIComponent(id)}`, { method: "DELETE" });

// ── Segments ─────────────────────────────────────────────
export const listSegments = (nodeId?: string) => {
  const q = nodeId ? `?node=${encodeURIComponent(nodeId)}` : "";
  return apiFetch<Segment[]>(`/api/segments${q}`);
};

export interface SegmentCreate {
  node: string;
  listen: string;
  upstream: string;
  chain?: string;
  proto?: string;
  ipv6?: boolean;
  comment?: string;
}
export const createSegment = (body: SegmentCreate) =>
  apiFetch<Segment>("/api/segments", {
    method: "POST",
    body: JSON.stringify(body),
  });

export const deleteSegment = (id: string) =>
  apiFetch<void>(`/api/segments/${encodeURIComponent(id)}`, { method: "DELETE" });

// ── Enrollment tokens ────────────────────────────────────
export const createEnrollmentToken = (name: string) =>
  apiFetch<EnrollmentResp>("/api/enrollment-tokens", {
    method: "POST",
    body: JSON.stringify({ name }),
  });
