// Lists crates in Cargo.lock whose locked version was published on crates.io after a cutoff.
// usage: node check-crates-age.mjs <repo-dir> [days | cutoffISO]   (default: 7 days)
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
const dir = process.argv[2] ?? ".";
const arg = process.argv[3];
const cutoff = arg && /^\d+$/.test(arg) ? new Date(Date.now() - Number(arg) * 86400000) : new Date(arg ?? Date.now() - 7 * 86400000);
const lockPath = join(dir, "Cargo.lock");
if (!existsSync(lockPath)) { console.log("no Cargo.lock"); process.exit(2); }
const text = readFileSync(lockPath, "utf8");
const pkgs = [];
for (const block of text.split("[[package]]").slice(1)) {
  const name = /name = "([^"]+)"/.exec(block)?.[1];
  const version = /version = "([^"]+)"/.exec(block)?.[1];
  const source = /source = "([^"]+)"/.exec(block)?.[1];
  if (name && version && source) pkgs.push({ name, version, source });
}
const bad = [];
for (const p of pkgs) {
  if (!p.source.startsWith("registry+https://github.com/rust-lang/crates.io-index") && !p.source.startsWith("sparse+https://index.crates.io/")) { bad.push({ ...p, published: "NON-CRATES.IO SOURCE " + p.source }); continue; }
  let created = null;
  for (let a = 0; a < 3 && !created; a++) {
    const r = await fetch(`https://crates.io/api/v1/crates/${p.name}/${p.version}`, { headers: { "user-agent": "check-crates-age (kienphan.dev@gmail.com)" } });
    if (r.ok) created = (await r.json()).version?.created_at ?? null;
    await new Promise((res) => setTimeout(res, 1100)); // crates.io asks for <= 1 request/second
  }
  if (!created) bad.push({ ...p, published: "UNKNOWN" });
  else if (new Date(created) > cutoff) bad.push({ ...p, published: created });
}
console.log(`${dir}: ${pkgs.length} locked crates, cutoff ${cutoff.toISOString()}, too new: ${bad.length}`);
for (const b of bad) console.log(`  ${b.name}@${b.version}  ${b.published}`);
process.exit(bad.length ? 1 : 0);
