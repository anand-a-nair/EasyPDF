// Copies static assets into dist/ after tsc.
// Deliberately not a bundler: TD-006 keeps the frontend dependency-free, and
// the webview loads ES modules natively. Node rather than `cp` so this works
// on Windows CI too.
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
mkdirSync(join(root, "dist"), { recursive: true });

for (const file of ["index.html", "styles.css"]) {
  copyFileSync(join(root, "static", file), join(root, "dist", file));
}
