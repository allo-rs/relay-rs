import { $ } from "bun";
import { mkdirSync, writeFileSync } from "fs";

const outDir = "../dist";
mkdirSync(`${outDir}/assets`, { recursive: true });

// 1. Tailwind CSS
await $`bunx tailwindcss -i ./src/index.css -o ${outDir}/index.css --minify`;

// 2. Bundle JS with Bun
const result = await Bun.build({
  entrypoints: ["./src/main.tsx"],
  outdir: `${outDir}/assets`,
  minify: true,
  target: "browser",
  naming: "[name]-[hash].[ext]",
  define: { "process.env.NODE_ENV": '"production"' },
  alias: { "@": "./src" },
});

if (!result.success) {
  console.error("Build failed:", result.logs);
  process.exit(1);
}

const jsFile = result.outputs.find((o) => o.path.endsWith(".js"));
const jsName = jsFile?.path.split("/").pop() ?? "main.js";

// 3. Generate index.html
writeFileSync(
  `${outDir}/index.html`,
  `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>relay-rs 控制面板</title>
    <link rel="stylesheet" href="/index.css" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/assets/${jsName}"></script>
  </body>
</html>`
);

console.log("Build complete →", outDir);
