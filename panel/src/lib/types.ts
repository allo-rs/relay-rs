// relay-rs 数据类型定义

export interface ForwardRule {
  listen: string;
  to: string[];
  proto: "all" | "tcp" | "udp";
  ipv6: boolean;
  balance?: "round-robin" | "random";
  rate_limit?: number; // Mbps
  comment?: string;
}

export interface BlockRule {
  src?: string;
  dst?: string;
  port?: number;
  proto: "all" | "tcp" | "udp";
  chain: "input" | "forward";
  ipv6: boolean;
  comment?: string;
}

export interface NodeRules {
  forward: ForwardRule[];
  block: BlockRule[];
}

export interface NodeInfo {
  id: number;
  name: string;
  url: string;
  online: boolean;
  version?: string;
  mode?: "nat" | "relay";
}

export interface CurrentUser {
  id: string;
  username: string;
  name?: string | null;
  email?: string | null;
  avatar?: string | null;
  admin: boolean;
}

export interface OkResponse {
  ok: true;
}
