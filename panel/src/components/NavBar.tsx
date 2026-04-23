import { useNavigate } from "react-router-dom";
import { LogOut, Radio } from "lucide-react";
import { Button } from "@/components/ui/button";
import { clearToken } from "@/lib/auth";

export default function NavBar() {
  const navigate = useNavigate();

  function handleLogout() {
    clearToken();
    navigate("/login", { replace: true });
  }

  return (
    <header className="sticky top-0 z-40 w-full border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
      <div className="container flex h-14 items-center justify-between">
        <div className="flex items-center gap-2">
          <Radio className="h-5 w-5 text-primary" />
          <span className="font-semibold text-base tracking-tight">relay-rs panel</span>
        </div>
        <Button variant="ghost" size="sm" onClick={handleLogout}>
          <LogOut className="h-4 w-4" />
          退出登录
        </Button>
      </div>
    </header>
  );
}
