// Parse and link every UI module without executing browser-only code.
import { readdir, readFile } from 'node:fs/promises';
import { resolve, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Script, SourceTextModule } from 'node:vm';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../app-ui');
async function files(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const nested = await Promise.all(entries.map((entry) => {
    const path = resolve(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  }));
  return nested.flat();
}

const modules = new Map();
async function load(path) {
  if (!modules.has(path)) {
    modules.set(path, new SourceTextModule(await readFile(path, 'utf8'), { identifier: path }));
  }
  return modules.get(path);
}
const paths = await files(root);
const js = paths.filter((path) => path.startsWith(resolve(root, 'js') + '/') && extname(path) === '.js');
const rust = await readFile(resolve(root, '../crates/cview-app/src/lib.rs'), 'utf8');
const handlers = rust.match(/tauri::generate_handler!\[([\s\S]*?)\]\)/)?.[1];
if (!handlers) throw new Error('Tauri command registration was not found');
const registered = new Set([...handlers.matchAll(/^\s*(?:\w+::)+(\w+)\s*,?\s*$/gm)].map((m) => m[1]));
const invoked = new Set();
for (const path of js) {
  const source = await readFile(path, 'utf8');
  for (const [, command] of source.matchAll(/\binvoke\s*\(\s*['"]([^'"]+)['"]/g)) {
    if (!registered.has(command)) throw new Error(`Unregistered IPC command ${command} in ${path}`);
    invoked.add(command);
  }
}
// Preload before linking so imports shared by multiple modules use the same instance.
for (const path of js) await load(path);
for (const path of js) {
  const module = await load(path);
  if (module.status !== 'unlinked') continue;
  await module.link((specifier, parent) => {
    if (!specifier.startsWith('./') && !specifier.startsWith('../')) {
      throw new Error(`Unsupported browser import: ${specifier} in ${parent.identifier}`);
    }
    return load(resolve(dirname(parent.identifier), specifier));
  });
}

let inline = 0;
for (const path of paths.filter((path) => extname(path) === '.html')) {
  const html = await readFile(path, 'utf8');
  for (const [, attributes, source] of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi)) {
    if (/\bsrc\s*=/i.test(attributes) || !source.trim()) continue;
    if (/\btype\s*=\s*["']module["']/i.test(attributes)) {
      new SourceTextModule(source, { identifier: path });
    } else {
      new Script(source, { filename: path });
    }
    inline += 1;
  }
}
console.log(`UI validation passed: ${modules.size} linked modules, ${inline} inline scripts, ${invoked.size} registered IPC commands.`);
