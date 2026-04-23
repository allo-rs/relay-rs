// 认证状态（基于 HttpOnly cookie）
//
// Cookie 对前端 JS 不可见，所以无法在本地判断登录态；
// 改由 PrivateRoute 调用 `/api/auth/me`，401 即未登录跳转。

import type { CurrentUser, MeResponse, DiscourseSetting } from "./types";

const ME_PATH = "/api/auth/me";
const LOGOUT_PATH = "/api/auth/logout";
const DISCOURSE_SETTING_PATH = "/api/settings/discourse";

export interface AuthState {
  configured: boolean;
  user: CurrentUser | null;
}

/** 拉取当前登录信息 + 系统是否已配置 Discourse */
export async function fetchAuthState(): Promise<AuthState> {
  const res = await fetch(ME_PATH, { credentials: "include" });
  if (res.status === 401) {
    // 已配置 Discourse 但未登录
    return { configured: true, user: null };
  }
  if (!res.ok) throw new Error(`auth check failed: ${res.status}`);
  const data = (await res.json()) as MeResponse;
  return { configured: data.configured, user: data.user };
}

export async function logout(): Promise<void> {
  await fetch(LOGOUT_PATH, { method: "POST", credentials: "include" });
}

/** 重定向到 Discourse 登录页，登录后回跳 current path */
export function redirectToDiscourseLogin(next?: string): void {
  const target = next ?? window.location.pathname + window.location.search;
  const url = `/api/auth/discourse/login?next=${encodeURIComponent(target)}`;
  window.location.href = url;
}

export async function getDiscourseSetting(): Promise<DiscourseSetting> {
  const res = await fetch(DISCOURSE_SETTING_PATH, { credentials: "include" });
  if (!res.ok) throw new Error(`get discourse setting failed: ${res.status}`);
  return (await res.json()) as DiscourseSetting;
}

export async function putDiscourseSetting(body: { url: string; secret?: string }): Promise<DiscourseSetting> {
  const res = await fetch(DISCOURSE_SETTING_PATH, {
    method: "PUT",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data?.error ?? `put discourse setting failed: ${res.status}`);
  return data as DiscourseSetting;
}

export async function deleteDiscourseSetting(): Promise<void> {
  const res = await fetch(DISCOURSE_SETTING_PATH, {
    method: "DELETE",
    credentials: "include",
  });
  if (!res.ok) throw new Error(`delete discourse setting failed: ${res.status}`);
}
