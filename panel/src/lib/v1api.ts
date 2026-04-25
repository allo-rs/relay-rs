// v1 API 封装。v1 endpoint 路径以 /api/v1/... 为主，鉴权复用 /api/auth/*
// 所有请求依赖 HttpOnly cookie（discourse SSO 签发的 relay_token）

async function v1Fetch<T>(url: string, init: RequestInit = {}): Promise<T> {
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
export interface V1Node {
  id: string;
  name: string;
  status: string;
  session_epoch: number;
  applied_revision: number;
  desired_revision: number;
  last_seen: string | null;
}

export interface V1Segment {
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

export interface V1EnrollmentResp {
  token: string;
  node_name: string;
  install_cmd: string;
}

export interface V1DiscourseSettings {
  configured: boolean;
  url: string | null;
  secret_set: boolean;
}

// ── Nodes ────────────────────────────────────────────────
export const listV1Nodes = () => v1Fetch<V1Node[]>("/api/v1/nodes");
export const deleteV1Node = (id: string) =>
  v1Fetch<void>(`/api/v1/nodes/${encodeURIComponent(id)}`, { method: "DELETE" });

// ── Segments ─────────────────────────────────────────────
export const listV1Segments = (nodeId?: string) => {
  const q = nodeId ? `?node=${encodeURIComponent(nodeId)}` : "";
  return v1Fetch<V1Segment[]>(`/api/v1/segments${q}`);
};

export interface V1SegmentCreate {
  node: string;
  listen: string;
  upstream: string;
  chain?: string;
  proto?: string;
  ipv6?: boolean;
  comment?: string;
}
export const createV1Segment = (body: V1SegmentCreate) =>
  v1Fetch<V1Segment>("/api/v1/segments", {
    method: "POST",
    body: JSON.stringify(body),
  });

export const deleteV1Segment = (id: string) =>
  v1Fetch<void>(`/api/v1/segments/${encodeURIComponent(id)}`, { method: "DELETE" });

// ── Enrollment tokens ────────────────────────────────────
export const createEnrollmentToken = (name: string) =>
  v1Fetch<V1EnrollmentResp>("/api/v1/enrollment-tokens", {
    method: "POST",
    body: JSON.stringify({ name }),
  });

// ── Settings ─────────────────────────────────────────────
export const getDiscourseSettings = () =>
  v1Fetch<V1DiscourseSettings>("/api/v1/settings/discourse");
export const putDiscourseSettings = (url: string, secret: string) =>
  v1Fetch<V1DiscourseSettings>("/api/v1/settings/discourse", {
    method: "PUT",
    body: JSON.stringify({ url, secret }),
  });
export const deleteDiscourseSettings = () =>
  v1Fetch<void>("/api/v1/settings/discourse", { method: "DELETE" });
