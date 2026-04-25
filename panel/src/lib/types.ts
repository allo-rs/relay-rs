// 通用前端类型定义

export interface CurrentUser {
  id: string;
  username: string;
  name?: string | null;
  email?: string | null;
  avatar?: string | null;
  admin: boolean;
}

export interface MeResponse {
  ok: true;
  configured: boolean;
  user: CurrentUser;
}

export interface DiscourseSetting {
  configured: boolean;
  url: string;
  secret_set: boolean;
}
