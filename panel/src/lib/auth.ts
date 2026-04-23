// JWT token 的 localStorage 管理

const TOKEN_KEY = "relay_rs_token";

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY);
}

export function isAuthenticated(): boolean {
  const token = getToken();
  if (!token) return false;
  // 简单检查 token 格式（JWT 有三段）
  return token.split(".").length === 3;
}
