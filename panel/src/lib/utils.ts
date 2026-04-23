import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

// 合并 Tailwind class，解决样式冲突
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
