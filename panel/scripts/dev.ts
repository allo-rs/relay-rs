import { watch } from "fs";
import { mkdirSync } from "fs";

const API_TARGET = "http://127.0.0.1:9090";
const DEV_PORT = 5173;
const OUT = "./dist-dev";

mkdirSync(`${OUT}/assets`, { recursive: true });

async function buildCSS() {
  const proc = Bun.spawn(
    ["bunx", "tailwindcss", "-i", "./src/index.css", "-o", `${OUT}/index.css`],
    { stdout: "inherit", stderr: "inherit" }
  );
  await proc.exited;
}

async function buildJS() {
  const result = await Bun.build({
    entrypoints: ["./src/main.tsx"],
    outdir: `${OUT}/assets`,
    naming: "[name].[ext]",
    target: "browser",
    define: { "process.env.NODE_ENV": '"development"' },
    sourcemap: "inline",
    alias: { "@": "./src" },
  });
  if (!result.success) console.error("JS build error:", result.logs);
}

function writeIndex() {
  Bun.write(
    `${OUT}/index.html`,
    `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>relay-rs 控制面板 [dev]</title>
    <link rel="stylesheet" href="/index.css" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/assets/main.js"></script>
  </body>
</html>`
  );
}

async function proxyToBackend(req: Request, path: string): Promise<Response> {
  const target = `${API_TARGET}${path}`;
  const noBody = req.method === "GET" || req.method === "HEAD";
  try {
    return await fetch(target, {
      method: req.method,
      headers: req.headers,
      body: noBody ? undefined : req.body,
      redirect: "manual",
    });
  } catch (e) {
    console.error("[dev] proxy error:", e);
    return new Response(`proxy error: ${e}`, { status: 502 });
  }
}

// 初始构建
await buildCSS();
await buildJS();
writeIndex();

// 监听源码变化重新构建
let rebuilding = false;
watch("./src", { recursive: true }, async (_event, filename) => {
  if (rebuilding) return;
  rebuilding = true;
  console.log(`[dev] changed: ${filename}`);
  if (String(filename).endsWith(".css")) {
    await buildCSS();
  } else {
    await buildJS();
  }
  rebuilding = false;
});

// 开发服务器：静态文件 + API 反代
const server = Bun.serve({
  port: DEV_PORT,
  idleTimeout: 30,
  async fetch(req) {
    const url = new URL(req.url);

    if (url.pathname.startsWith("/api/")) {
      return proxyToBackend(req, url.pathname + url.search);
    }

    const candidates =
      url.pathname === "/" || url.pathname === ""
        ? [`${OUT}/index.html`]
        : [`${OUT}${url.pathname}`, `${OUT}/index.html`];

    for (const p of candidates) {
      const f = Bun.file(p);
      if (await f.exists()) return new Response(f);
    }
    return new Response("Not Found", { status: 404 });
  },
});

console.log(`[dev] http://localhost:${server.port}  →  API: ${API_TARGET}`);
