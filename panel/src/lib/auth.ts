// 认证状态（基于 HttpOnly cookie）
//
// Cookie 对前端 JS 不可见，所以无法在本地判断登录态；
// 改由 PrivateRoute 调用 `/api/auth/me`，401 即未登录跳转。

import type { CurrentUser } from "./types";

const ME_PATH = "/api/auth/me";
const LOGOUT_PATH = "/api/auth/logout";

export async function fetchCurrentUser(): Promise<CurrentUser | null> {
  const res = await fetch(ME_PATH, { credentials: "include" });
  if (res.status === 401) return null;
  if (!res.ok) throw new Error(`auth check failed: ${res.status}`);
  const data = (await res.json()) as { ok: true; user: CurrentUser };
  return data.user;
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
